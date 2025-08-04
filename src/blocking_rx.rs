use crate::{channel::*, AsyncRx, MAsyncRx};
use crossbeam::channel::Receiver;
use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

/// Single consumer (receiver) that works in blocking context.
///
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
///
/// **NOTE: Rx is not Clone, nor Sync.**
/// If you need concurrent access, use [MRx](crate::MRx) instead.
///
/// Rx has Send marker, can be moved to other thread.
/// The following code is OK:
///
/// ``` rust
/// use crossfire::*;
/// let (tx, rx) = mpsc::bounded_blocking::<usize>(100);
/// std::thread::spawn(move || {
///     let _ = rx.recv();
/// });
/// drop(tx);
/// ```
///
/// Because Rx does not have Sync marker, using `Arc<Rx>` will lose Send marker.
///
/// For your safety, the following code **should not compile**:
///
/// ``` compile_fail
/// use crossfire::*;
/// use std::sync::Arc;
/// let (tx, rx) = mpsc::bounded_blocking::<usize>(100);
/// let rx = Arc::new(rx);
/// std::thread::spawn(move || {
///     let _ = rx.recv();
/// });
/// drop(tx);
/// ```
pub struct Rx<T> {
    pub(crate) recv: Receiver<T>,
    pub(crate) shared: Arc<ChannelShared>,
    // Remove the Sync marker to prevent being put in Arc
    _phan: PhantomData<Cell<()>>,
}

unsafe impl<T: Send> Send for Rx<T> {}

impl<T> fmt::Debug for Rx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Rx")
    }
}

impl<T> fmt::Display for Rx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Rx")
    }
}

impl<T> Drop for Rx<T> {
    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl<T> From<AsyncRx<T>> for Rx<T> {
    fn from(value: AsyncRx<T>) -> Self {
        value.add_rx();
        Self::new(value.recv.clone(), value.shared.clone())
    }
}

impl<T> Rx<T> {
    #[inline]
    pub(crate) fn new(recv: Receiver<T>, shared: Arc<ChannelShared>) -> Self {
        Self { recv, shared, _phan: Default::default() }
    }

    /// Receive message, will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvError]) when all Tx dropped.
    #[inline]
    pub fn recv<'a>(&'a self) -> Result<T, RecvError> {
        match self.recv.recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Try to receive message, non-blocking.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// returns Err([TryRecvError::Disconnected]) when all Tx dropped and channel is empty.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Waits for a message to be received from the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// The behavior is atomic, either successfully polls a message,
    /// or operation cancelled due to timeout.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// returns Err([RecvTimeoutError::Disconnected]) when all Tx dropped and channel is empty.
    #[inline]
    pub fn recv_timeout(&self, duration: Duration) -> Result<T, RecvTimeoutError> {
        match self.recv.recv_timeout(duration) {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// The number of messages in the channel at the moment
    #[inline]
    pub fn len(&self) -> usize {
        self.recv.len()
    }

    /// Whether channel is empty at the moment
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }

    /// Whether the channel is full at the moment
    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.recv.is_full()
    }
}

/// Multi-consumer (receiver) that works in blocking context.
///
/// Inherits [`Rx<T>`] and implements [Clone].
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
///
/// You can use `into()` to convert it to `Rx<T>`.
pub struct MRx<T>(pub(crate) Rx<T>);

impl<T> fmt::Debug for MRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MRx")
    }
}

impl<T> fmt::Display for MRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MRx")
    }
}

unsafe impl<T: Send> Sync for MRx<T> {}

impl<T> MRx<T> {
    #[inline]
    pub(crate) fn new(recv: Receiver<T>, shared: Arc<ChannelShared>) -> Self {
        Self(Rx::new(recv, shared))
    }
}

impl<T> Clone for MRx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_rx();
        Self(Rx::new(inner.recv.clone(), inner.shared.clone()))
    }
}

impl<T> Deref for MRx<T> {
    type Target = Rx<T>;

    /// inherit all the functions of [Rx]
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> From<MRx<T>> for Rx<T> {
    fn from(rx: MRx<T>) -> Self {
        rx.0
    }
}

impl<T> From<MAsyncRx<T>> for MRx<T> {
    fn from(value: MAsyncRx<T>) -> Self {
        value.add_rx();
        Self::new(value.recv.clone(), value.shared.clone())
    }
}

/// For writing generic code with MRx & Rx
pub trait BlockingRxTrait<T: Send + 'static>:
    Send + 'static + fmt::Debug + fmt::Display + AsRef<ChannelShared>
{
    /// Receive message, will block when channel is empty.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([RecvError]) when all Tx dropped.
    fn recv<'a>(&'a self) -> Result<T, RecvError>;

    /// Try to receive message, non-blocking.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) when all Tx dropped and channel is empty.
    fn try_recv(&self) -> Result<T, TryRecvError>;

    /// Waits for a message to be received from the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// returns Err([RecvTimeoutError::Disconnected]) when all Tx dropped and channel is empty.
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError>;

    /// The number of messages in the channel at the moment
    fn len(&self) -> usize;

    /// Whether channel is empty at the moment
    fn is_empty(&self) -> bool;

    /// Whether the channel is full at the moment
    fn is_full(&self) -> bool;

    /// Return true if the other side has closed
    #[inline(always)]
    fn is_disconnected(&self) -> bool {
        self.as_ref().is_disconnected()
    }
}

impl<T: Send + 'static> BlockingRxTrait<T> for Rx<T> {
    #[inline(always)]
    fn recv<'a>(&'a self) -> Result<T, RecvError> {
        Rx::recv(self)
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        Rx::try_recv(self)
    }

    #[inline(always)]
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        Rx::recv_timeout(self, timeout)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Rx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        Rx::is_empty(self)
    }

    #[inline(always)]
    fn is_full(&self) -> bool {
        Rx::is_full(self)
    }
}

impl<T: Send + 'static> BlockingRxTrait<T> for MRx<T> {
    #[inline(always)]
    fn recv<'a>(&'a self) -> Result<T, RecvError> {
        self.0.recv()
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }

    #[inline(always)]
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        self.0.recv_timeout(timeout)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline(always)]
    fn is_full(&self) -> bool {
        self.0.is_full()
    }
}

impl<T> Deref for Rx<T> {
    type Target = ChannelShared;

    #[inline(always)]
    fn deref(&self) -> &ChannelShared {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared> for Rx<T> {
    #[inline(always)]
    fn as_ref(&self) -> &ChannelShared {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared> for MRx<T> {
    #[inline(always)]
    fn as_ref(&self) -> &ChannelShared {
        &self.0.shared
    }
}
