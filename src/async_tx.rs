use crate::backoff::Backoff;
use crate::sink::AsyncSink;
use crate::{channel::*, MTx, Tx};
use std::cell::Cell;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::mem::{needs_drop, ManuallyDrop, MaybeUninit};
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Single producer (sender) that works in async context.
///
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
///
/// **NOTE: AsyncTx is not Clone, nor Sync.**
/// If you need concurrent access, use [MAsyncTx](crate::MAsyncTx) instead.
///
/// AsyncTx has Send marker, can be moved to other coroutine.
/// The following code is OK :
///
/// ``` rust
/// use crossfire::*;
/// async fn foo() {
///     let (tx, rx) = spsc::bounded_async::<usize>(100);
///     tokio::spawn(async move {
///          let _ = tx.send(2).await;
///     });
///     drop(rx);
/// }
/// ```
///
/// Because AsyncTx does not have Sync marker, using `Arc<AsyncTx>` will lose Send marker.
///
/// For your safety, the following code **should not compile**:
///
/// ``` compile_fail
/// use crossfire::*;
/// use std::sync::Arc;
/// async fn foo() {
///     let (tx, rx) = spsc::bounded_async::<usize>(100);
///     let tx = Arc::new(tx);
///     tokio::spawn(async move {
///          let _ = tx.send(2).await;
///     });
///     drop(rx);
/// }
/// ```
pub struct AsyncTx<T> {
    pub(crate) shared: Arc<ChannelShared<T>>,
    // Remove the Sync marker to prevent being put in Arc
    _phan: PhantomData<Cell<()>>,
}

impl<T> fmt::Debug for AsyncTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncTx")
    }
}

impl<T> fmt::Display for AsyncTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncTx")
    }
}

unsafe impl<T: Send> Send for AsyncTx<T> {}

impl<T> Drop for AsyncTx<T> {
    fn drop(&mut self) {
        self.shared.close_tx();
    }
}

impl<T> TryFrom<Tx<T>> for AsyncTx<T> {
    type Error = Tx<T>;
    fn try_from(value: Tx<T>) -> Result<Self, Self::Error> {
        match &value.shared.senders {
            Registry::Multi(_) | Registry::Single(_) => {
                let value = ManuallyDrop::new(value);
                unsafe { Ok(AsyncTx::new(std::ptr::read(&value.shared))) }
            }
            Registry::Dummy(_) => Err(value),
        }
    }
}

impl<T> AsyncTx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self { shared, _phan: Default::default() }
    }

    #[inline]
    pub fn into_sink(self) -> AsyncSink<T> {
        AsyncSink::new(self)
    }
}

impl<T: Unpin + Send + 'static> AsyncTx<T> {
    /// Send message. Will await when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    #[inline(always)]
    pub fn send<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        return SendFuture { shared: &self.shared, item: MaybeUninit::new(item), waker: None };
    }

    /// Try to send message, non-blocking
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([TrySendError::Full]) on channel full for bounded channel.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        if self.shared.is_disconnected() {
            return Err(TrySendError::Disconnected(item));
        }
        let _item = MaybeUninit::new(item);
        match self.shared.try_send(&_item) {
            Err(()) => {
                return unsafe { Err(TrySendError::Full(_item.assume_init())) };
            }
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

    /// Waits for a message to be sent into the channel, but only for a limited time.
    /// Will await when channel is full.
    ///
    /// The behavior is atomic, either message sent successfully or returned on error.
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([SendTimeoutError::Timeout]) when the operation timed out.
    ///
    /// Returns Err([SendTimeoutError::Disconnected]) when all Rx dropped.
    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    #[inline]
    pub fn send_timeout<'a>(
        &'a self, item: T, duration: std::time::Duration,
    ) -> SendTimeoutFuture<'a, T> {
        let sleep = {
            #[cfg(feature = "tokio")]
            {
                Box::pin(tokio::time::sleep(duration))
            }
            #[cfg(not(feature = "tokio"))]
            {
                Box::pin(async_std::task::sleep(duration))
            }
        };
        return SendTimeoutFuture {
            shared: &self.shared,
            item: MaybeUninit::new(item),
            waker: None,
            sleep,
        };
    }

    /// Internal function might change in the future. For public version, use AsyncSink::poll_send() instead
    ///
    /// Returns `Poll::Ready(Ok(()))` on message sent.
    ///
    /// Returns `Poll::Pending` for Poll::Pending case.
    ///
    /// Returns `Poll::Ready(Err(())` when all Rx dropped.
    #[inline(always)]
    pub(crate) fn poll_send<'a>(
        shared: &'a ChannelShared<T>, ctx: &'a mut Context, item: &MaybeUninit<T>,
        o_waker: &'a mut Option<LockedWaker>,
    ) -> Poll<Result<(), ()>> {
        if shared.is_disconnected() {
            return Poll::Ready(Err(()));
        }
        // When the result is not TrySendError::Full,
        // make sure always take the o_waker out and abandon,
        // to skip the timeout cleaning logic in Drop.
        let try_times = if shared.bound_size <= Some(2) { 5 } else { 1 };
        let mut backoff = Backoff::new(try_times);
        loop {
            if shared.try_send(item).is_ok() {
                shared.on_send();
                if let Some(old_waker) = o_waker.take() {
                    shared.cancel_send_waker(old_waker);
                }
                return Poll::Ready(Ok(()));
            }
            if backoff.is_completed() {
                if shared.reg_send_async(ctx, o_waker) {
                    // waker is not consumed
                    break;
                }
                // NOTE: The other side put something whie reg_send and did not see the waker,
                // should check the channel again, otherwise might incur a dead lock.
                if !shared.is_full() {
                    continue;
                }
                break;
            }
            backoff.snooze();
        }
        if shared.is_disconnected() {
            return Poll::Ready(Err(()));
        }
        return Poll::Pending;
    }
}

/// A fixed-sized future object constructed by [AsyncTx::make_send_future()]
pub struct SendFuture<'a, T: Unpin> {
    shared: &'a ChannelShared<T>,
    item: MaybeUninit<T>,
    waker: Option<LockedWaker>,
}

unsafe impl<T: Unpin + Send> Send for SendFuture<'_, T> {}

impl<T: Unpin> Drop for SendFuture<'_, T> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            // Cancelling the future, poll is not ready
            if waker.abandon() {
                // We are waked, but give up sending, should notify another sender for safety
                self.shared.on_recv();
            } else {
                self.shared.clear_send_wakers(waker.get_seq());
            }
            if needs_drop::<T>() {
                if size_of::<T>() > size_of::<*mut T>() {
                    unsafe { self.item.assume_init_drop() };
                }
            }
        }
    }
}

impl<T: Unpin + Send + 'static> Future for SendFuture<'_, T> {
    type Output = Result<(), SendError<T>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        match AsyncTx::poll_send(&_self.shared, ctx, &_self.item, &mut _self.waker) {
            Poll::Ready(Ok(())) => {
                debug_assert!(_self.waker.is_none());
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(Err(())) => {
                let _ = _self.waker.take();
                return Poll::Ready(Err(SendError(unsafe { _self.item.assume_init_read() })));
            }
            Poll::Pending => return Poll::Pending,
        }
    }
}

/// A fixed-sized future object constructed by [AsyncTx::send_timeout()]
#[cfg(any(feature = "tokio", feature = "async_std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
pub struct SendTimeoutFuture<'a, T: Unpin> {
    shared: &'a ChannelShared<T>,
    item: MaybeUninit<T>,
    waker: Option<LockedWaker>,
    #[cfg(feature = "tokio")]
    sleep: Pin<Box<tokio::time::Sleep>>,
    #[cfg(not(feature = "tokio"))]
    sleep: Pin<Box<dyn Future<Output = ()>>>,
}

#[cfg(any(feature = "tokio", feature = "async_std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
unsafe impl<T: Unpin + Send> Send for SendTimeoutFuture<'_, T> {}

#[cfg(any(feature = "tokio", feature = "async_std"))]
impl<T: Unpin> Drop for SendTimeoutFuture<'_, T> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            // Cancelling the future, poll is not ready
            if waker.abandon() {
                // We are waked, but give up sending, should notify another sender for safety
                self.shared.on_recv();
            } else {
                self.shared.clear_send_wakers(waker.get_seq());
            }
        }
    }
}

#[cfg(any(feature = "tokio", feature = "async_std"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
impl<T: Unpin + Send + 'static> Future for SendTimeoutFuture<'_, T> {
    type Output = Result<(), SendTimeoutError<T>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        let r = AsyncTx::poll_send(&_self.shared, ctx, &_self.item, &mut _self.waker);
        match r {
            Poll::Ready(Ok(())) => {
                debug_assert!(_self.waker.is_none());
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(Err(())) => {
                let _ = _self.waker.take();
                return Poll::Ready(Err(SendTimeoutError::Disconnected(unsafe {
                    _self.item.assume_init_read()
                })));
            }
            Poll::Pending => {
                if let Poll::Ready(()) = _self.sleep.as_mut().poll(ctx) {
                    return Poll::Ready(Err(SendTimeoutError::Timeout(unsafe {
                        _self.item.assume_init_read()
                    })));
                }
                return Poll::Pending;
            }
        }
    }
}

/// For writing generic code with MAsyncTx & AsyncTx
pub trait AsyncTxTrait<T: Unpin + Send + 'static>:
    Send + 'static + fmt::Debug + fmt::Display + AsRef<ChannelShared<T>>
{
    /// Try to send message, non-blocking
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([TrySendError::Full]) on channel full for bounded channel.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>>;

    /// The number of messages in the channel at the moment
    #[inline(always)]
    fn len(&self) -> usize {
        self.as_ref().len()
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

    /// Send message. Will await when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    fn send<'a>(&'a self, item: T) -> SendFuture<'a, T>;

    /// Waits for a message to be sent into the channel, but only for a limited time.
    /// Will await when channel is full.
    ///
    /// The behavior is atomic, either message sent successfully or returned on error.
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([SendTimeoutError::Timeout]) when the operation timed out.
    ///
    /// Returns Err([SendTimeoutError::Disconnected]) when all Rx dropped.
    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    fn send_timeout<'a>(
        &'a self, item: T, duration: std::time::Duration,
    ) -> SendTimeoutFuture<'a, T>;
}

impl<T: Unpin + Send + 'static> AsyncTxTrait<T> for AsyncTx<T> {
    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        AsyncTx::try_send(self, item)
    }

    #[inline(always)]
    fn send<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        AsyncTx::send(self, item)
    }

    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    #[inline(always)]
    fn send_timeout<'a>(
        &'a self, item: T, duration: std::time::Duration,
    ) -> SendTimeoutFuture<'a, T> {
        AsyncTx::send_timeout(self, item, duration)
    }
}

/// Multi-producer (sender) that works in async context.
///
/// Inherits [`AsyncTx<T>`] and implements [Clone].
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
///
/// You can use `into()` to convert it to `AsyncTx<T>`.
pub struct MAsyncTx<T>(pub(crate) AsyncTx<T>);

impl<T> fmt::Debug for MAsyncTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MAsyncTx")
    }
}

impl<T> fmt::Display for MAsyncTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MAsyncTx")
    }
}

unsafe impl<T: Send> Sync for MAsyncTx<T> {}

impl<T: Unpin> Clone for MAsyncTx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_tx();
        Self(AsyncTx::new(inner.shared.clone()))
    }
}

impl<T> MAsyncTx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self(AsyncTx::new(shared))
    }
}

impl<T> Deref for MAsyncTx<T> {
    type Target = AsyncTx<T>;

    /// inherit all the functions of [AsyncTx]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Unpin + Send + 'static> AsyncTxTrait<T> for MAsyncTx<T> {
    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(item)
    }

    #[inline(always)]
    fn send<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        self.0.send(item)
    }

    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    #[inline(always)]
    fn send_timeout<'a>(
        &'a self, item: T, duration: std::time::Duration,
    ) -> SendTimeoutFuture<'a, T> {
        self.0.send_timeout(item, duration)
    }
}

impl<T> TryFrom<MTx<T>> for MAsyncTx<T> {
    type Error = MTx<T>;
    fn try_from(value: MTx<T>) -> Result<Self, Self::Error> {
        if matches!(value.shared.recvs, Registry::Multi(_)) {
            let value = ManuallyDrop::new(value);
            unsafe { Ok(MAsyncTx(AsyncTx::new(std::ptr::read(&value.shared)))) }
        } else {
            Err(value)
        }
    }
}

impl<T> Deref for AsyncTx<T> {
    type Target = ChannelShared<T>;
    fn deref(&self) -> &ChannelShared<T> {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared<T>> for AsyncTx<T> {
    fn as_ref(&self) -> &ChannelShared<T> {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared<T>> for MAsyncTx<T> {
    fn as_ref(&self) -> &ChannelShared<T> {
        &self.0.shared
    }
}
