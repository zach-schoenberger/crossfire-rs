use crate::locked_waker::*;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Context;

#[enum_dispatch(RegistryTrait)]
pub enum Registry {
    Single(RegistrySingle),
    Multi(RegistryMulti),
    Dummy(RegistryDummy),
}

#[enum_dispatch]
pub trait RegistryTrait {
    fn is_empty(&self) -> bool;

    /// For async context
    fn reg_async(&self, _ctx: &mut Context, _o_waker: &mut Option<LockedWaker>) -> bool;

    /// For thread context
    fn reg_blocking(&self, _waker: &LockedWaker);

    fn clear_wakers(&self, _seq: u64);

    fn cancel_waker(&self, _waker: &LockedWaker);

    fn fire(&self);

    fn close(&self);

    /// return waker queue size
    fn len(&self) -> usize;
}

/// RegistryDummy is for unbounded channel tx, which is never blocked
pub struct RegistryDummy();

impl RegistryDummy {
    #[inline(always)]
    pub fn new() -> Registry {
        Registry::Dummy(RegistryDummy())
    }
}

impl RegistryTrait for RegistryDummy {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        true
    }

    #[inline(always)]
    fn reg_async(&self, _ctx: &mut Context, _o_waker: &mut Option<LockedWaker>) -> bool {
        unreachable!();
    }

    #[inline(always)]
    fn reg_blocking(&self, _waker: &LockedWaker) {
        unreachable!();
    }

    #[inline(always)]
    fn clear_wakers(&self, _seq: u64) {}

    #[inline(always)]
    fn cancel_waker(&self, _waker: &LockedWaker) {}

    #[inline(always)]
    fn fire(&self) {}

    #[inline(always)]
    fn close(&self) {}

    /// return waker queue size
    #[inline(always)]
    fn len(&self) -> usize {
        0
    }
}

pub struct RegistrySingle {
    cell: WakerCell,
}

impl RegistrySingle {
    #[inline(always)]
    pub fn new() -> Registry {
        Registry::Single(Self { cell: WakerCell::new() })
    }
}

impl RegistryTrait for RegistrySingle {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        !self.cell.exists()
    }

    /// return is_skip
    #[inline(always)]
    fn reg_async(&self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>) -> bool {
        if let Some(_waker) = o_waker.as_ref() {
            // ref: https://github.com/frostyplanet/crossfire-rs/issues/14
            // https://docs.rs/tokio/latest/tokio/runtime/index.html#:~:text=Normally%2C%20tasks%20are%20scheduled%20only,is%20called%20a%20spurious%20wakeup
            // There might be situation like spurious wakeup, poll() again under no fire() ever
            // happened, waker still exists but cannot be used to wake the current future.
            // Since there's no lock inside fire(), to avoid race, can not update the content but to put a new one.
            if _waker.will_wake(ctx) {
                // Normally only selection or multiplex future will get here.
                // No need to reg again, since waker is not consumed nor changed
                return true;
            }
        }
        let waker = LockedWaker::new_async(ctx);
        let weak = waker.weak();
        o_waker.replace(waker);
        self.cell.put(weak);
        false
    }

    #[inline(always)]
    fn reg_blocking(&self, waker: &LockedWaker) {
        self.cell.put(waker.weak());
    }

    #[inline(always)]
    fn cancel_waker(&self, _waker: &LockedWaker) {
        // Got to be it, because only one single thread.
        self.cell.clear();
    }

    #[inline(always)]
    fn clear_wakers(&self, _seq: u64) {
        // Got to be it, because only one single thread.
        self.cell.clear();
    }

    #[inline(always)]
    fn fire(&self) {
        self.cell.wake();
    }

    #[inline(always)]
    fn close(&self) {
        self.fire();
    }

    /// return waker queue size
    #[inline(always)]
    fn len(&self) -> usize {
        if self.cell.exists() {
            1
        } else {
            0
        }
    }
}

struct RegistryMultiInner {
    queue: VecDeque<LockedWakerRef>,
    seq: u64,
}

pub struct RegistryMulti {
    checking: AtomicBool,
    is_empty: AtomicBool,
    inner: Mutex<RegistryMultiInner>,
}

impl RegistryMulti {
    #[inline(always)]
    pub fn new() -> Registry {
        Registry::Multi(Self {
            inner: Mutex::new(RegistryMultiInner { queue: VecDeque::with_capacity(32), seq: 0 }),
            checking: AtomicBool::new(false),
            is_empty: AtomicBool::new(true),
        })
    }

    #[inline(always)]
    fn push(&self, waker: &LockedWaker) {
        let weak = waker.weak();
        let mut guard = self.inner.lock();
        let seq = guard.seq.wrapping_add(1);
        guard.seq = seq;
        waker.set_seq(seq);
        if guard.queue.is_empty() {
            self.is_empty.store(false, Ordering::Release);
            guard.queue.push_back(weak);
        } else {
            guard.queue.push_back(weak);
        }
    }
}

impl RegistryTrait for RegistryMulti {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.is_empty.load(Ordering::Acquire)
    }

    #[inline(always)]
    fn reg_async(&self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>) -> bool {
        if let Some(_waker) = o_waker.as_ref() {
            // ref: https://github.com/frostyplanet/crossfire-rs/issues/14
            // https://docs.rs/tokio/latest/tokio/runtime/index.html#:~:text=Normally%2C%20tasks%20are%20scheduled%20only,is%20called%20a%20spurious%20wakeup
            // There might be situation like spurious wakeup, poll() again under no fire() ever
            // happened, waker still exists but cannot be used to wake the current future.
            // Since there's no lock inside fire(), to avoid race, can not update the content but to put a new one.
            if _waker.will_wake(ctx) {
                // Normally only selection or multiplex future will get here.
                // No need to reg again, since waker is not consumed nor changed
                return true;
            }
        }
        let waker = LockedWaker::new_async(ctx);
        self.push(&waker);
        o_waker.replace(waker);
        false
    }

    #[inline(always)]
    fn reg_blocking(&self, waker: &LockedWaker) {
        self.push(waker);
    }

    #[inline(always)]
    fn cancel_waker(&self, waker: &LockedWaker) {
        if self.is_empty.load(Ordering::Acquire) {
            return;
        }
        let mut guard = self.inner.lock();
        // Just abandon and leave it to fire() to clean it
        let seq = waker.get_seq();
        if let Some(waker_ref) = guard.queue.pop_front() {
            waker_ref.try_to_clear(seq);
        }
    }

    /// Call when ReceiveFuture is cancelled.
    /// to clear the LockedWakerRef which has been sent to the other side.
    #[inline(always)]
    fn clear_wakers(&self, seq: u64) {
        if self.checking.swap(true, Ordering::SeqCst) {
            // Other thread is cleaning
            return;
        }
        let mut guard = self.inner.lock();
        while let Some(waker_ref) = guard.queue.pop_front() {
            if waker_ref.try_to_clear(seq) {
                // we do not known push back may have concurrent problem
                break;
            }
        }

        self.checking.store(false, Ordering::Release);
    }

    #[inline(always)]
    fn fire(&self) {
        if self.is_empty.load(Ordering::Acquire) {
            return;
        }
        let mut guard = self.inner.lock();
        let seq = guard.seq;
        while let Some(waker_ref) = guard.queue.pop_front() {
            if guard.queue.is_empty() {
                self.is_empty.store(true, Ordering::Release);
                if waker_ref.wake() {
                    return;
                }
            } else {
                if let Some(waker) = waker_ref.upgrade() {
                    if waker.wake() {
                        return;
                    }
                    // The latest seq in RegistryMulti is always last_waker.get_seq() +1
                    // Because some waker (issued by sink / stream) might be INIT all the time,
                    // prevent to dead loop situation when they are wake up and re-register again.
                    if waker.get_seq().wrapping_add(1) == seq {
                        return;
                    }
                }
            }
        }
    }

    #[inline(always)]
    fn close(&self) {
        let mut guard = self.inner.lock();
        while let Some(waker) = guard.queue.pop_front() {
            waker.wake();
        }
        self.is_empty.store(true, Ordering::Release);
    }

    /// return waker queue size
    #[inline(always)]
    fn len(&self) -> usize {
        let guard = self.inner.lock();
        guard.queue.len()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::locked_waker::LockedWaker;
    #[test]
    fn test_registry_multi() {
        let reg = RegistryMulti::new();

        // test push
        let waker1 = LockedWaker::new_blocking();
        assert_eq!(reg.is_empty(), true);
        reg.reg_blocking(&waker1);
        assert!(waker1.get_seq() > 0);
        assert_eq!(reg.is_empty(), false);
        assert_eq!(reg.len(), 1);
        assert_eq!(waker1.is_waked(), false);

        let waker2 = LockedWaker::new_blocking();
        reg.reg_blocking(&waker2);
        assert_eq!(reg.len(), 2);
        assert_eq!(waker2.get_seq(), waker1.get_seq() + 1);
        assert_eq!(waker2.is_waked(), false);

        // test fire
        reg.fire();
        assert_eq!(waker1.is_waked(), true);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.is_empty(), false);
        reg.fire();
        assert_eq!(waker2.is_waked(), true);
        assert_eq!(reg.len(), 0);
        assert_eq!(reg.is_empty(), true);

        // test seq

        let waker3 = LockedWaker::new_blocking();
        reg.reg_blocking(&waker3);
        let waker4 = LockedWaker::new_blocking();
        reg.reg_blocking(&waker4);
        for _ in 0..10 {
            let _waker = LockedWaker::new_blocking();
            reg.reg_blocking(&_waker);
        }
        assert_eq!(reg.len(), 12);
        assert_eq!(waker4.abandon(), false);
        reg.clear_wakers(waker4.get_seq());
        assert_eq!(reg.len(), 10);
        assert!(waker3.is_waked());
        assert!(waker4.is_waked());

        // test close
        assert_eq!(reg.is_empty(), false);
        reg.close();
        assert_eq!(reg.len(), 0);
    }
}
