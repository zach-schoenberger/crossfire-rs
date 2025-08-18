use std::sync::atomic::{AtomicUsize, Ordering};

static DROP_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub trait TestDropMsg: Unpin + Send + 'static {
    fn new(v: usize) -> Self;

    fn get_value(&self) -> usize;
}

pub struct SmallMsg(pub usize);

impl Drop for SmallMsg {
    fn drop(&mut self) {
        DROP_COUNTER.fetch_add(1, Ordering::SeqCst);
    }
}

impl TestDropMsg for SmallMsg {
    fn new(v: usize) -> Self {
        Self(v)
    }

    fn get_value(&self) -> usize {
        self.0
    }
}

pub struct LargeMsg([usize; 4]);

impl TestDropMsg for LargeMsg {
    fn new(v: usize) -> Self {
        Self([v, v, v, v])
    }

    fn get_value(&self) -> usize {
        self.0[0]
    }
}

impl Drop for LargeMsg {
    fn drop(&mut self) {
        DROP_COUNTER.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn get_drop_counter() -> usize {
    DROP_COUNTER.load(Ordering::SeqCst)
}

pub fn reset_drop_counter() {
    DROP_COUNTER.store(0, Ordering::SeqCst);
}

#[allow(dead_code)]
macro_rules! runtime_block_on {
    ($f: expr) => {{
        #[cfg(feature = "async_std")]
        {
            log::info!("run with async_std");
            async_std::task::block_on($f);
        }
        #[cfg(not(feature = "async_std"))]
        {
            let runtime_flag = std::env::var("SINGLE_THREAD_RUNTIME").unwrap_or("".to_string());
            let mut rt = if runtime_flag.len() > 0 {
                log::info!("run with tokio current thread");
                tokio::runtime::Builder::new_current_thread()
            } else {
                log::info!("run with tokio multi thread");
                tokio::runtime::Builder::new_multi_thread()
            };
            rt.enable_all().build().unwrap().block_on($f);
        }
    }};
}
pub(super) use runtime_block_on;

#[allow(dead_code)]
macro_rules! async_spawn {
    ($f: expr) => {{
        #[cfg(feature = "async_std")]
        {
            async_std::task::spawn($f)
        }
        #[cfg(not(feature = "async_std"))]
        {
            tokio::spawn($f)
        }
    }};
}
pub(super) use async_spawn;

#[allow(dead_code)]
macro_rules! async_join_result {
    ($th: expr) => {{
        #[cfg(feature = "async_std")]
        {
            $th.await
        }
        #[cfg(not(feature = "async_std"))]
        {
            $th.await.expect("join")
        }
    }};
}
pub(super) use async_join_result;

pub async fn sleep(duration: std::time::Duration) {
    #[cfg(feature = "async_std")]
    {
        async_std::task::sleep(duration).await;
    }
    #[cfg(not(feature = "async_std"))]
    {
        tokio::time::sleep(duration).await;
    }
}

pub async fn timeout<F, T>(duration: std::time::Duration, future: F) -> Result<T, String>
where
    F: std::future::Future<Output = T>,
{
    #[cfg(feature = "async_std")]
    {
        async_std::future::timeout(duration, future)
            .await
            .map_err(|_| format!("Test timed out after {:?}", duration))
    }
    #[cfg(not(feature = "async_std"))]
    {
        tokio::time::timeout(duration, future)
            .await
            .map_err(|_| format!("Test timed out after {:?}", duration))
    }
}
