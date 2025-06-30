use std::fmt;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::task::*;

/// Waker object used by [crate::AsyncTx::poll_send()] and [crate::AsyncRx::poll_item()]
pub struct LockedWaker(Arc<LockedWakerInner>);

impl fmt::Debug for LockedWaker {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let _self = self.0.as_ref();
        write!(f, "LockedWaker(seq={}, waked={})", _self.seq, _self.waked.load(Ordering::Acquire))
    }
}

struct LockedWakerInner {
    waker: std::task::Waker,
    waked: AtomicBool,
    seq: u64,
}

impl Clone for LockedWaker {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct LockedWakerRef {
    w: Weak<LockedWakerInner>,
}

impl fmt::Debug for LockedWakerRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LockedWakerRef")
    }
}

impl LockedWaker {
    #[inline(always)]
    pub(crate) fn new(ctx: &Context, seq: u64) -> Self {
        let s = Arc::new(LockedWakerInner {
            seq,
            waker: ctx.waker().clone(),
            waked: AtomicBool::new(false),
        });
        Self(s)
    }

    #[inline(always)]
    pub(crate) fn get_seq(&self) -> u64 {
        self.0.seq
    }

    // return is_already waked
    #[inline(always)]
    pub(crate) fn abandon(&self) -> bool {
        let _self = self.0.as_ref();
        _self.waked.swap(true, Ordering::SeqCst)
    }

    #[inline(always)]
    pub(crate) fn weak(&self) -> LockedWakerRef {
        LockedWakerRef { w: Arc::downgrade(&self.0) }
    }

    #[inline(always)]
    pub(crate) fn is_waked(&self) -> bool {
        self.0.waked.load(Ordering::Acquire)
    }

    /// return true on suc wake up, false when already woken up.
    #[inline(always)]
    pub(crate) fn wake(&self) -> bool {
        let _self = self.0.as_ref();
        let waked = _self.waked.swap(true, Ordering::SeqCst);
        if !waked {
            _self.waker.wake_by_ref();
        }
        !waked
    }
}

impl LockedWakerRef {
    #[inline(always)]
    pub(crate) fn wake(&self) -> bool {
        if let Some(_self) = self.w.upgrade() {
            LockedWaker(_self).wake()
        } else {
            false
        }
    }

    /// return true to stop; return false to continue the search.
    pub(crate) fn try_to_clear(&self, seq: u64) -> bool {
        if let Some(w) = self.w.upgrade() {
            let waker = LockedWaker(w);
            let _seq = waker.get_seq();
            if _seq == seq {
                // It's my waker, stopped
                return true;
            }
            if !waker.is_waked() {
                waker.wake();
                // other future is before me, but not canceled, i should stop.
                // we do not known push back may have concurrent problem
                return true;
            }
            return _seq > seq;
        }
        false
    }
}

#[test]
fn test_waker() {
    println!("waker size {}", std::mem::size_of::<LockedWakerRef>());
}
