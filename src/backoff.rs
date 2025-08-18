use core::num::NonZero;
use std::mem::transmute;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

pub const SPIN_LIMIT: u16 = 6;
pub const DEFAULT_LIMIT: u16 = 6;
pub const MAX_LIMIT: u16 = 10;

static DETECT_CONFIG: AtomicU32 =
    AtomicU32::new(BackoffConfig { spin_limit: SPIN_LIMIT, limit: DEFAULT_LIMIT }.to_u32());

static _INIT: AtomicBool = AtomicBool::new(false);

/// Detect cpu number and auto setting backoff config, which applys anytime after execution, and
/// save the result to global atomic.
///
/// One one core system, it will be more effective (as much as 2x faster) to use yield than spinning.
/// Cpu detection is somehow slow, you can call it manually before channel initialization.
pub fn detect_backoff_cfg() {
    if _INIT.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return;
    }
    if thread::available_parallelism().unwrap_or(NonZero::new(1).unwrap())
        == NonZero::new(1).unwrap()
    {
        let cfg = BackoffConfig { spin_limit: 0, limit: DEFAULT_LIMIT }.to_u32();
        DETECT_CONFIG.store(cfg, Ordering::Relaxed);
    };
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BackoffConfig {
    pub spin_limit: u16,
    pub limit: u16,
}

impl Default for BackoffConfig {
    #[inline(always)]
    fn default() -> Self {
        Self::from_u32(DETECT_CONFIG.load(Ordering::Relaxed))
    }
}

impl BackoffConfig {
    #[inline(always)]
    pub const fn to_u32(self) -> u32 {
        let i: u32 = unsafe { transmute(self) };
        return i;
    }

    #[inline(always)]
    pub const fn from_u32(config: u32) -> Self {
        unsafe { transmute(config) }
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub const fn async_limit(mut self, limit: u16) -> Self {
        if limit < self.limit {
            self.limit = limit;
        }
        self.spin_limit = limit;
        self
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub const fn limit(mut self, limit: u16) -> Self {
        self.limit = limit;
        self
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub const fn spin(mut self, spin_limit: u16) -> Self {
        if spin_limit < self.spin_limit {
            self.spin_limit = spin_limit;
        }
        self
    }
}

pub struct Backoff {
    step: u16,
    pub config: BackoffConfig,
}

impl Backoff {
    #[inline(always)]
    pub fn new(config: BackoffConfig) -> Self {
        Self { step: 0, config }
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn spin(&mut self) {
        for _ in 0..1 << self.step {
            std::hint::spin_loop();
        }
        if self.step < MAX_LIMIT {
            self.step += 1;
        }
    }

    #[inline(always)]
    pub fn snooze(&mut self) {
        if self.step < self.config.spin_limit {
            for _ in 0..1 << self.step {
                std::hint::spin_loop();
            }
        } else {
            std::thread::yield_now();
        }
        if self.step < MAX_LIMIT {
            self.step += 1;
        }
    }

    pub fn yield_now(&mut self) {
        std::thread::yield_now();
        if self.step < MAX_LIMIT {
            self.step += 1;
        }
    }

    #[inline(always)]
    pub fn is_completed(&self) -> bool {
        self.step >= self.config.limit
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn step(&self) -> usize {
        self.step as usize
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.step = 0;
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_backoff() {
        let backoff = Backoff::new(BackoffConfig { spin_limit: 1, limit: 0 });
        assert!(backoff.is_completed());
        println!("backoff size {}", size_of::<Backoff>());
        println!("BackoffConfig size {}", size_of::<BackoffConfig>());
        assert_eq!(size_of::<BackoffConfig>(), size_of::<u32>());
        let config = BackoffConfig { spin_limit: 6, limit: 7 };
        let config_i = config.to_u32();
        let _config = BackoffConfig::from_u32(config_i);
        assert_eq!(config.spin_limit, _config.spin_limit);
        assert_eq!(config.limit, _config.limit);

        let mut backoff = Backoff::new(BackoffConfig { spin_limit: 2, limit: 4 });
        assert_eq!(backoff.step, 0);
        backoff.spin();
        assert_eq!(backoff.step, 1);
        backoff.snooze();
        assert_eq!(backoff.step, 2);
        backoff.snooze();
        backoff.snooze();
        backoff.snooze();
        backoff.snooze();
        assert_eq!(backoff.step, 6);
        backoff.spin();
        assert_eq!(backoff.step, 7);
    }
}
