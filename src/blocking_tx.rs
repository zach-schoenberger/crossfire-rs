use crate::channel::*;
use crossbeam::channel::Sender;
use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Duration;

/// Single producer (sender) that works in blocking context.
///
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
///
/// **NOTE: Tx is not Clone, nor Sync.**
/// If you need concurrent access, use [MTx](crate::MTx) instead.
///
/// Tx has Send marker, can be moved to other thread.
/// The following code is OK:
///
/// ``` rust
/// use crossfire::*;
/// let (tx, rx) = spsc::bounded_blocking::<usize>(100);
/// std::thread::spawn(move || {
///     let _ = tx.send(1);
/// });
/// drop(rx);
/// ```
///
/// Because Tx does not have Sync marker, using `Arc<Tx>` will lose Send marker.
///
/// For your safety, the following code **should not compile**:
///
/// ``` compile_fail
/// use crossfire::*;
/// use std::sync::Arc;
/// let (tx, rx) = spsc::bounded_blocking::<usize>(100);
/// let tx = Arc::new(tx);
/// std::thread::spawn(move || {
///     let _ = tx.send(1);
/// });
/// drop(rx);
/// ```
pub struct Tx<T> {
    pub(crate) sender: Sender<T>,
    pub(crate) shared: Arc<ChannelShared>,
    // Remove the Sync marker to prevent being put in Arc
    _phan: PhantomData<Cell<()>>,
}

unsafe impl<T: Send> Send for Tx<T> {}

impl<T> fmt::Debug for Tx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Tx")
    }
}

impl<T> fmt::Display for Tx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Tx")
    }
}

impl<T> Drop for Tx<T> {
    fn drop(&mut self) {
        self.shared.close_tx();
    }
}

impl<T> Tx<T> {
    #[inline]
    pub(crate) fn new(sender: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self { sender, shared, _phan: Default::default() }
    }

    /// Send message. Will block when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    ///
    #[inline]
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        match self.sender.send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
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
        match self.sender.try_send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

    /// Waits for a message to be sent into the channel, but only for a limited time.
    /// Will block when channel is full.
    ///
    /// The behavior is atomic, either message sent successfully or returned on error.
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([SendTimeoutError::Timeout]) when the the operation timed out.
    ///
    /// Returns Err([SendTimeoutError::Disconnected]) when all Rx dropped.
    #[inline]
    pub fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>> {
        match self.sender.send_timeout(item, timeout) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

    /// The number of messages in the channel at the moment
    #[inline]
    pub fn len(&self) -> usize {
        self.sender.len()
    }

    /// Whether channel is empty at the moment
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sender.is_empty()
    }

    /// Whether the channel is full at the moment
    #[inline]
    pub fn is_full(&self) -> bool {
        self.sender.is_full()
    }
}

/// Multi-producer (sender) that works in blocking context.
///
/// Inherits [`Tx<T>`] and implements [Clone].
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
///
/// You can use `into()` to convert it to `Tx<T>`.
pub struct MTx<T>(pub(crate) Tx<T>);

impl<T> fmt::Debug for MTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MTx")
    }
}

impl<T> fmt::Display for MTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MTx")
    }
}

unsafe impl<T: Send> Sync for MTx<T> {}

impl<T> MTx<T> {
    #[inline]
    pub(crate) fn new(send: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self(Tx::new(send, shared))
    }
}

impl<T: Unpin> Clone for MTx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_tx();
        Self(Tx::new(inner.sender.clone(), inner.shared.clone()))
    }
}

impl<T> Deref for MTx<T> {
    type Target = Tx<T>;

    /// inherit all the functions of [Tx]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for MTx<T> {
    /// inherit all the functions of [Tx]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// For writing generic code with MTx & Tx
pub trait BlockingTxTrait<T: Send + 'static>:
    Send + 'static + fmt::Debug + fmt::Display + AsRef<ChannelShared>
{
    /// Send message. Will block when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    fn send(&self, _item: T) -> Result<(), SendError<T>>;

    /// Try to send message, non-blocking
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([TrySendError::Full]) on channel full for bounded channel.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    fn try_send(&self, _item: T) -> Result<(), TrySendError<T>>;

    /// Waits for a message to be sent into the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([SendTimeoutError::Timeout]) when the message could not be sent because the channel is full and the operation timed out.
    ///
    /// Returns Err([SendTimeoutError::Disconnected]) when all Rx dropped.
    fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>>;

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

impl<T: Send + 'static> BlockingTxTrait<T> for Tx<T> {
    #[inline(always)]
    fn send(&self, item: T) -> Result<(), SendError<T>> {
        Tx::send(self, item)
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        Tx::try_send(self, item)
    }

    #[inline(always)]
    fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>> {
        Tx::send_timeout(&self, item, timeout)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Tx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        Tx::is_empty(self)
    }

    #[inline(always)]
    fn is_full(&self) -> bool {
        Tx::is_full(self)
    }
}

impl<T: Send + 'static> BlockingTxTrait<T> for MTx<T> {
    #[inline(always)]
    fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.0.send(item)
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(item)
    }

    #[inline(always)]
    fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>> {
        self.0.send_timeout(item, timeout)
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

impl<T> Deref for Tx<T> {
    type Target = ChannelShared;
    fn deref(&self) -> &ChannelShared {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared> for Tx<T> {
    fn as_ref(&self) -> &ChannelShared {
        &self.shared
    }
}

impl<T> AsRef<ChannelShared> for MTx<T> {
    fn as_ref(&self) -> &ChannelShared {
        &self.0.shared
    }
}
