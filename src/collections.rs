use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct LockedQueue<T> {
    empty: AtomicBool,
    queue: Mutex<VecDeque<T>>,
}

impl<T> LockedQueue<T> {
    #[inline]
    pub fn new(cap: usize) -> Self {
        Self { empty: AtomicBool::new(true), queue: Mutex::new(VecDeque::with_capacity(cap)) }
    }

    #[inline(always)]
    pub fn push(&self, msg: T) {
        let mut guard = self.queue.lock();
        let _ = self.empty.compare_exchange_weak(true, false, Ordering::SeqCst, Ordering::Relaxed);
        guard.push_back(msg);
    }

    #[inline(always)]
    pub fn pop(&self) -> Option<T> {
        if self.empty.load(Ordering::Acquire) {
            return None;
        }
        let mut guard = self.queue.lock();
        if let Some(item) = guard.pop_front() {
            Some(item)
        } else {
            self.empty.store(true, Ordering::Release);
            None
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        let guard = self.queue.lock();
        guard.len()
    }
}
