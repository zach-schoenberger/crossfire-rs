use std::{fmt, time::Duration};

use crossbeam::channel::{RecvError, RecvTimeoutError, TryRecvError};

use crate::{AsyncRxTrait, BlockingRxTrait, ChannelShared, MAsyncRx, ReceiveFuture};

/// Universal-consumer (receiver) that works in both async and sync contexts.
///
/// Inherits [`BlockingRxTrait<T>`], [`AsyncRxTrait<T>`], and implements [Clone].
/// Additional methods can be accessed through Deref<Target=[ChannelShared]>.
pub struct UniversalRx<T>(pub(crate) MAsyncRx<T>);

impl<T> fmt::Debug for UniversalRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UAsyncRx")
    }
}

impl<T> fmt::Display for UniversalRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UAsyncRx")
    }
}

impl<T> Clone for UniversalRx<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Send + 'static> BlockingRxTrait<T> for UniversalRx<T> {
    #[inline(always)]
    fn recv<'a>(&'a self) -> Result<T, RecvError> {
        match self.0.recv.recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.0.shared.on_recv();
                return Ok(i);
            }
        }
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.0.recv.try_recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.0.shared.on_recv();
                return Ok(i);
            }
        }
    }

    #[inline(always)]
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        match self.0.recv.recv_timeout(timeout) {
            Err(e) => return Err(e),
            Ok(i) => {
                self.0.shared.on_recv();
                return Ok(i);
            }
        }
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

impl<T: Unpin + Send + 'static> AsyncRxTrait<T> for UniversalRx<T> {
    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }

    #[inline(always)]
    fn recv<'a>(&'a self) -> ReceiveFuture<'a, T> {
        self.0.recv()
    }

    #[cfg(any(feature = "tokio", feature = "async_std"))]
    #[cfg_attr(docsrs, doc(cfg(any(feature = "tokio", feature = "async_std"))))]
    #[inline(always)]
    fn recv_timeout<'a>(&'a self, duration: std::time::Duration) -> ReceiveTimeoutFuture<'a, T> {
        self.0.recv_timeout(duration)
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

impl<T> AsRef<ChannelShared> for UniversalRx<T> {
    fn as_ref(&self) -> &ChannelShared {
        &self.0.shared
    }
}
