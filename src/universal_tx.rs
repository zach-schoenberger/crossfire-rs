use crate::{AsyncTxTrait, BlockingTxTrait, ChannelShared, MAsyncTx, SendFuture};
use crossbeam::channel::{SendError, SendTimeoutError, TrySendError};
use std::{fmt, time::Duration};

/// Universal-producer (sender) that works in both async and sync contexts.
///
/// Inherits [`BlockingTxTrait<T>`], [`AsyncTxTrait<T>`], and implements [Clone].
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
pub struct UniversalTx<T>(MAsyncTx<T>);

impl<T: Unpin> Clone for UniversalTx<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> fmt::Debug for UniversalTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UAsyncTx")
    }
}

impl<T> fmt::Display for UniversalTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UAsyncTx")
    }
}

impl<T: Send + 'static> BlockingTxTrait<T> for UniversalTx<T> {
    #[inline(always)]
    fn send(&self, item: T) -> Result<(), SendError<T>> {
        match self.0.sender.send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.0.shared.on_send();
                return Ok(());
            }
        }
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        match self.0.sender.try_send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.0.shared.on_send();
                return Ok(());
            }
        }
    }

    #[inline(always)]
    fn send_timeout(&self, item: T, timeout: Duration) -> Result<(), SendTimeoutError<T>> {
        match self.0.sender.send_timeout(item, timeout) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.0.shared.on_send();
                return Ok(());
            }
        }
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.0.sender.len()
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.sender.is_empty()
    }

    #[inline(always)]
    fn is_full(&self) -> bool {
        self.0.sender.is_full()
    }
}

impl<T: Unpin + Send + 'static> AsyncTxTrait<T> for UniversalTx<T> {
    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(item)
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

impl<T> AsRef<ChannelShared> for UniversalTx<T> {
    fn as_ref(&self) -> &ChannelShared {
        &self.0.shared
    }
}
