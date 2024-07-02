use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant};

pub static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Simple atomic increments
pub async fn do_work_async() {
    loop {
        // let start = Instant::now();
        COUNTER.fetch_add(1, Ordering::Relaxed);
        tokio::task::yield_now().await
        // super::sleep_or_yield(start.elapsed()).await;
    }
}