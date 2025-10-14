use crate::backoff::*;
use crate::stream::AsyncStream;
#[cfg(feature = "trace_log")]
use crate::tokio_task_id;
use crate::{channel::*, trace_log, MRx, Rx};
use std::cell::Cell;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::task::{Context, Poll};

/// A single consumer (receiver) that works in an async context.
///
/// Additional methods in [ChannelShared] can be accessed through `Deref`.
///
/// `AsyncRx` can be converted into `Rx` via the `From` trait,
/// which means you can have two types of receivers, both within async and
/// blocking contexts, for the same channel.

///
/// **NOTE**: `AsyncRx` is not `Clone` or `Sync`.
/// If you need concurrent access, use [MAsyncRx] instead.
///
/// `AsyncRx` has a `Send` marker and can be moved to other coroutines.
/// The following code is OK:
///
/// ``` rust
/// use crossfire::*;
/// async fn foo() {
///     let (tx, rx) = mpsc::bounded_async::<usize>(100);
///     tokio::spawn(async move {
///         let _ = rx.recv().await;
///     });
///     drop(tx);
/// }
/// ```
///
/// Because `AsyncRx` does not have a `Sync` marker, using `Arc<AsyncRx>` will lose the `Send` marker.
///
/// For your safety, the following code **should not compile**:
///
/// ``` compile_fail
/// use crossfire::*;
/// use std::sync::Arc;
/// async fn foo() {
///     let (tx, rx) = mpsc::bounded_async::<usize>(100);
///     let rx = Arc::new(rx);
///     tokio::spawn(async move {
///         let _ = rx.recv().await;
///     });
///     drop(tx);
/// }
/// ```
pub struct AsyncRx<T> {
    pub(crate) shared: Arc<ChannelShared<T>>,
    // Remove the Sync marker to prevent being put in Arc
    _phan: PhantomData<Cell<()>>,
    backoff: AtomicU32,
}

unsafe impl<T: Send> Send for AsyncRx<T> {}

impl<T> fmt::Debug for AsyncRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncRx")
    }
}

impl<T> fmt::Display for AsyncRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncRx")
    }
}

impl<T> Drop for AsyncRx<T> {
    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl<T> From<Rx<T>> for AsyncRx<T> {
    fn from(value: Rx<T>) -> Self {
        value.add_rx();
        Self::new(value.shared.clone())
    }
}

impl<T> AsyncRx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self { shared, _phan: Default::default(), backoff: AtomicU32::new(0) }
    }

    #[inline(always)]
    pub(crate) fn get_backoff_cfg(&self) -> BackoffConfig {
        let backoff = self.backoff.load(Ordering::Relaxed);
        if backoff == 0 {
            let backoff_limit = self.shared.detect_async_backoff_rx();
            let config = BackoffConfig { spin_limit: SPIN_LIMIT, limit: backoff_limit };
            self.backoff.store(config.to_u32(), Ordering::Release);
            return config;
        } else {
            return BackoffConfig::from_u32(backoff);
        }
    }

    /// Receives a message from the channel. This method will await until a message is received or the channel is closed.
    ///
    /// This function is cancellation-safe, so it's safe to use with `timeout()` and the `select!` macro.
    /// When a [RecvFuture] is dropped, no message will be received from the channel.
    ///
    /// For timeout scenarios, there's an alternative: [AsyncRx::recv_timeout()].
    ///
    /// Returns `Ok(T)` on success.
    ///
    /// Returns Err([RecvError]) if the sender has been dropped.
    #[inline(always)]
    pub fn recv<'a>(&'a self) -> RecvFuture<'a, T> {
        return RecvFuture { rx: self, waker: None };
    }

    /// Receives a message from the channel with a timeout.
    /// Will await when channel is empty.
    ///
    /// The behavior is atomic: the message is either received successfully or the operation is canceled due to a timeout.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// Returns Err([RecvTimeoutError::Disconnected]) if the sender has been dropped and the channel is empty.
    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    #[inline]
    pub fn recv_timeout<'a>(
        &'a self, duration: std::time::Duration,
    ) -> RecvTimeoutFuture<'a, T, ()> {
        let sleep = {
            #[cfg(feature = "tokio")]
            {
                tokio::time::sleep(duration)
            }
            #[cfg(feature = "async_std")]
            {
                async_std::task::sleep(duration)
            }
        };
        self.recv_with_timer(sleep)
    }

    /// Receives a message from the channel with a custom timer function (from other async runtime).
    ///
    /// The behavior is atomic: the message is either received successfully or the operation is canceled due to a timeout.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// Returns Err([RecvTimeoutError::Disconnected]) if the sender has been dropped and the channel is empty.
    ///
    /// # Argument:
    ///
    /// * `fut`: The sleep function. It's possible to wrap this function with cancelable handle,
    /// you can control when to stop polling. the return value of `fut` is ignore.
    /// We add generic `R` just in order to support smol::Timer
    ///
    /// # Example:
    ///
    /// ```ignore
    /// extern crate smol;
    /// use std::time::Duration;
    /// use crossfire::*;
    /// async fn foo() {
    ///     let (tx, rx) = mpmc::bounded_async::<usize>(10);
    ///     match rx.recv_with_timer(smol::Timer::after(Duration::from_secs(1))).await {
    ///         Ok(_item)=>{
    ///             println!("message recv");
    ///         }
    ///         Err(RecvTimeoutError::Timeout)=>{
    ///             println!("timeout");
    ///         }
    ///         Err(RecvTimeoutError::Disconnected)=>{
    ///             println!("sender-side closed");
    ///         }
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn recv_with_timer<'a, F, R>(&'a self, fut: F) -> RecvTimeoutFuture<'a, T, R>
    where
        F: Future<Output = R> + 'static,
    {
        return RecvTimeoutFuture { rx: self, waker: None, sleep: Box::pin(fut) };
    }

    /// Attempts to receive a message from the channel without blocking.
    ///
    /// Returns `Ok(T)` on successful.
    ///
    /// Returns Err([TryRecvError::Empty]) if the channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) if the sender has been dropped and the channel is empty.
    #[inline(always)]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        if let Some(item) = self.shared.try_recv() {
            self.shared.on_recv();
            return Ok(item);
        } else {
            if self.shared.is_disconnected() {
                return Err(TryRecvError::Disconnected);
            }
            return Err(TryRecvError::Empty);
        }
    }

    /// Internal function might change in the future. For public version, use AsyncStream::poll_item() instead
    ///
    /// Returns `Ok(T)` on successful.
    ///
    /// Return Err([TryRecvError::Empty]) for Poll::Pending case.
    ///
    /// Return Err([TryRecvError::Disconnected]) when all Tx dropped and channel is empty.
    #[inline(always)]
    pub(crate) fn poll_item(
        &self, ctx: &mut Context, o_waker: &mut Option<RecvWaker>, stream: bool,
    ) -> Result<T, TryRecvError> {
        let shared = &self.shared;
        // When the result is not TryRecvError::Empty,
        // make sure always take the o_waker out and abandon,
        // to skip the timeout cleaning logic in Drop.
        macro_rules! try_recv {
            ($state: expr) => {
                if let Some(item) = shared.try_recv() {
                    shared.on_recv();
                    if let Some(waker) = o_waker.take() {
                        trace_log!("rx{:?}: recv {:?} {}", tokio_task_id!(), waker, $state);
                        if $state < WakerState::Waked as u8 {
                            shared.recvs.cancel_waker(&waker);
                        }
                    } else {
                        trace_log!("rx{:?}: recv", tokio_task_id!());
                    }
                    return Ok(item);
                }
            };
        }
        loop {
            try_recv!(WakerState::Waked as u8);
            if let Some(waker) = o_waker.as_ref() {
                match waker.try_change_state(WakerState::Waked, WakerState::Init) {
                    Ok(_) => {
                        if !waker.will_wake(ctx) {
                            let _ = o_waker.take();
                        }
                    }
                    Err(state) => {
                        if state < WakerState::Waked as u8 {
                            // ARM based processors on tokio are not reliable,
                            // so we need to treat this as if the waker will not wake.
                            #[cfg(target_arch = "aarch64")]
                            {
                                // Spurious waked by runtime, waker can not be re-used (issue 38)
                                shared.recvs.cancel_waker(&waker);
                                trace_log!("rx{:?}: drop waker {:?}", tokio_task_id!(), waker);
                                let _ = o_waker.take(); // waker cannot be used again
                            }
                            #[cfg(not(target_arch = "aarch64"))]
                            {
                                if waker.will_wake(ctx) {
                                    // Spurious waked by runtime, or
                                    // Normally only selection or multiplex future will get here.
                                    // No need to reg again, since waker is not consumed.
                                    trace_log!("rx{:?}: will_wake {:?}", tokio_task_id!(), waker);
                                    break;
                                } else {
                                    // Spurious waked by runtime, waker can not be re-used (issue 38)
                                    shared.recvs.cancel_waker(&waker);
                                    trace_log!("rx{:?}: drop waker {:?}", tokio_task_id!(), waker);
                                    let _ = o_waker.take(); // waker cannot be used again
                                }
                            }
                        } else if state == WakerState::Closed as u8 {
                            break;
                        }
                    }
                }
            } else {
                // First call
                let cfg = self.get_backoff_cfg();
                if cfg.limit > 0 {
                    let mut backoff = Backoff::new(cfg);
                    loop {
                        backoff.spin();
                        if let Some(item) = shared.try_recv() {
                            shared.on_recv();
                            trace_log!("rx{:?}: recv", tokio_task_id!());
                            return Ok(item);
                        }
                        if backoff.is_completed() {
                            break;
                        }
                    }
                }
            }
            if let Some(waker) = o_waker.take() {
                shared.reg_recv(&waker);
                o_waker.replace(waker);
            } else {
                let waker = RecvWaker::new_async(ctx, ());
                shared.reg_recv(&waker);
                o_waker.replace(waker);
            }
            // NOTE: The other side put something whie reg_send and did not see the waker,
            // should check the channel again, otherwise might incur a dead lock.
            if !shared.is_empty() {
                try_recv!(WakerState::Init as u8);
            }
            if !stream {
                let _waker = o_waker.as_ref().unwrap();
                let state = _waker.commit_waiting();
                trace_log!("rx{:?}: commit_waiting {:?} {}", tokio_task_id!(), _waker, state);
                if state == WakerState::Waked as u8 {
                    continue;
                }
            }
            break;
        }
        if shared.is_disconnected() {
            try_recv!(WakerState::Closed as u8);
            trace_log!("rx{:?}: disconnected {:?}", tokio_task_id!(), o_waker);
            return Err(TryRecvError::Disconnected);
        } else {
            return Err(TryRecvError::Empty);
        }
    }

    #[inline]
    pub fn into_stream(self) -> AsyncStream<T>
    where
        T: Send + Unpin + 'static,
    {
        AsyncStream::new(self)
    }

    #[inline]
    pub fn into_blocking(self) -> Rx<T> {
        self.into()
    }
}

/// A fixed-sized future object constructed by [AsyncRx::recv()]
pub struct RecvFuture<'a, T> {
    rx: &'a AsyncRx<T>,
    waker: Option<RecvWaker>,
}

unsafe impl<T: Send> Send for RecvFuture<'_, T> {}

impl<T> Drop for RecvFuture<'_, T> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            // cancelled
            self.rx.shared.abandon_recv_waker(waker);
        }
    }
}

impl<T> Future for RecvFuture<'_, T> {
    type Output = Result<T, RecvError>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        match _self.rx.poll_item(ctx, &mut _self.waker, false) {
            Err(e) => {
                if !e.is_empty() {
                    let _ = _self.waker.take();
                    return Poll::Ready(Err(RecvError {}));
                } else {
                    return Poll::Pending;
                }
            }
            Ok(item) => {
                debug_assert!(_self.waker.is_none());
                return Poll::Ready(Ok(item));
            }
        }
    }
}

/// A fixed-sized future object constructed by [AsyncRx::recv_timeout()]
pub struct RecvTimeoutFuture<'a, T, R> {
    rx: &'a AsyncRx<T>,
    waker: Option<RecvWaker>,
    sleep: Pin<Box<dyn Future<Output = R>>>,
}

unsafe impl<T: Unpin + Send, R> Send for RecvTimeoutFuture<'_, T, R> {}

impl<T, R> Drop for RecvTimeoutFuture<'_, T, R> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            // cancelled
            self.rx.shared.abandon_recv_waker(waker);
        }
    }
}

impl<T, R> Future for RecvTimeoutFuture<'_, T, R> {
    type Output = Result<T, RecvTimeoutError>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        match _self.rx.poll_item(ctx, &mut _self.waker, false) {
            Err(TryRecvError::Empty) => {
                if let Poll::Ready(_) = _self.sleep.as_mut().poll(ctx) {
                    return Poll::Ready(Err(RecvTimeoutError::Timeout));
                }
                return Poll::Pending;
            }
            Err(TryRecvError::Disconnected) => {
                return Poll::Ready(Err(RecvTimeoutError::Disconnected));
            }
            Ok(item) => {
                return Poll::Ready(Ok(item));
            }
        }
    }
}

/// For writing generic code with MAsyncRx & AsyncRx
pub trait AsyncRxTrait<T: Unpin + Send + 'static>:
    Send + 'static + fmt::Debug + fmt::Display + AsRef<ChannelShared<T>> + Sized + Into<AsyncStream<T>>
{
    /// Receive message, will await when channel is empty.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// returns Err([RecvError]) when all Tx dropped.
    fn recv<'a>(&'a self) -> RecvFuture<'a, T>;

    /// Waits for a message to be received from the channel, but only for a limited time.
    /// Will await when channel is empty.
    ///
    /// The behavior is atomic, either successfully polls a message,
    /// or operation cancelled due to timeout.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// returns Err([RecvTimeoutError::Disconnected]) when all Tx dropped and channel is empty.
    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    fn recv_timeout<'a>(&'a self, timeout: std::time::Duration) -> RecvTimeoutFuture<'a, T, ()>;

    /// Receives a message from the channel with a custom timer function (from other async runtime).
    ///
    /// The behavior is atomic: the message is either received successfully or the operation is canceled due to a timeout.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// Returns Err([RecvTimeoutError::Disconnected]) if the sender has been dropped and the channel is empty.
    ///
    /// # Argument:
    ///
    /// * `fut`: The sleep function. It's possible to wrap this function with cancelable handle,
    /// you can control when to stop polling. the return value of `fut` is ignore.
    /// We add generic `R` just in order to support smol::Timer.
    fn recv_with_timer<'a, F, R>(&'a self, fut: F) -> RecvTimeoutFuture<'a, T, R>
    where
        F: Future<Output = R> + 'static;

    /// Try to receive message, non-blocking.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) when all Tx dropped and channel is empty.
    fn try_recv(&self) -> Result<T, TryRecvError>;

    /// The number of messages in the channel at the moment
    #[inline(always)]
    fn len(&self) -> usize {
        self.as_ref().len()
    }

    /// The capacity of the channel, return None for unbounded channel.
    #[inline(always)]
    fn capacity(&self) -> Option<usize> {
        self.as_ref().capacity()
    }

    /// Whether channel is empty at the moment
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    /// Whether the channel is full at the moment
    #[inline(always)]
    fn is_full(&self) -> bool {
        self.as_ref().is_full()
    }

    /// Return true if the other side has closed
    #[inline(always)]
    fn is_disconnected(&self) -> bool {
        self.as_ref().is_disconnected()
    }

    fn clone_to_vec(self, count: usize) -> Vec<Self>;
}

impl<T: Unpin + Send + 'static> AsyncRxTrait<T> for AsyncRx<T> {
    #[inline(always)]
    fn clone_to_vec(self, _count: usize) -> Vec<Self> {
        assert_eq!(_count, 1);
        vec![self]
    }

    #[inline(always)]
    fn recv<'a>(&'a self) -> RecvFuture<'a, T> {
        AsyncRx::recv(self)
    }

    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    #[inline(always)]
    fn recv_timeout<'a>(&'a self, duration: std::time::Duration) -> RecvTimeoutFuture<'a, T, ()> {
        AsyncRx::recv_timeout(self, duration)
    }

    #[inline(always)]
    fn recv_with_timer<'a, F, R>(&'a self, fut: F) -> RecvTimeoutFuture<'a, T, R>
    where
        F: Future<Output = R> + 'static,
    {
        AsyncRx::recv_with_timer(self, fut)
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        AsyncRx::<T>::try_recv(self)
    }
}

/// A multi-consumer (receiver) that works in an async context.
///
/// Inherits from [`AsyncRx<T>`] and implements `Clone`.
/// Additional methods in [ChannelShared] can be accessed through `Deref`.
///
/// You can use `into()` to convert it to `AsyncRx<T>`.
///
/// `MAsyncRx` can be converted into `MRx` via the `From` trait,
/// which means you can have two types of receivers, both within async and
/// blocking contexts, for the same channel.

pub struct MAsyncRx<T>(pub(crate) AsyncRx<T>);

impl<T> fmt::Debug for MAsyncRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MAsyncRx")
    }
}

impl<T> fmt::Display for MAsyncRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MAsyncRx")
    }
}

unsafe impl<T: Send> Sync for MAsyncRx<T> {}

impl<T> Clone for MAsyncRx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_rx();
        Self(AsyncRx::new(inner.shared.clone()))
    }
}

impl<T> From<MAsyncRx<T>> for AsyncRx<T> {
    fn from(rx: MAsyncRx<T>) -> Self {
        rx.0
    }
}

impl<T> MAsyncRx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self(AsyncRx::new(shared))
    }

    #[inline]
    pub fn into_stream(self) -> AsyncStream<T>
    where
        T: Send + Unpin + 'static,
    {
        AsyncStream::new(self.0)
    }

    #[inline]
    pub fn into_blocking(self) -> MRx<T> {
        self.into()
    }
}

impl<T> Deref for MAsyncRx<T> {
    type Target = AsyncRx<T>;

    /// inherit all the functions of [AsyncRx]
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> From<MRx<T>> for MAsyncRx<T> {
    fn from(value: MRx<T>) -> Self {
        value.add_rx();
        Self::new(value.shared.clone())
    }
}

impl<T: Unpin + Send + 'static> AsyncRxTrait<T> for MAsyncRx<T> {
    #[inline(always)]
    fn clone_to_vec(self, count: usize) -> Vec<Self> {
        let mut v = Vec::with_capacity(count);
        for _ in 0..count - 1 {
            v.push(self.clone());
        }
        v.push(self);
        v
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }

    #[inline(always)]
    fn recv<'a>(&'a self) -> RecvFuture<'a, T> {
        self.0.recv()
    }

    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    #[inline(always)]
    fn recv_timeout<'a>(&'a self, duration: std::time::Duration) -> RecvTimeoutFuture<'a, T, ()> {
        self.0.recv_timeout(duration)
    }

    #[inline(always)]
    fn recv_with_timer<'a, F, R>(&'a self, fut: F) -> RecvTimeoutFuture<'a, T, R>
    where
        F: Future<Output = R> + 'static,
    {
        self.0.recv_with_timer(fut)
    }
}

impl<T> Deref for AsyncRx<T> {
    type Target = ChannelShared<T>;
    #[inline(always)]
    fn deref(&self) -> &ChannelShared<T> {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared<T>> for AsyncRx<T> {
    #[inline(always)]
    fn as_ref(&self) -> &ChannelShared<T> {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared<T>> for MAsyncRx<T> {
    #[inline(always)]
    fn as_ref(&self) -> &ChannelShared<T> {
        &self.0.shared
    }
}
