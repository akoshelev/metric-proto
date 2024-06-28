use std::sync::atomic::{AtomicU64, Ordering};

pub static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Simple atomic increments
pub async fn do_work(iter: u64) {
    for _ in 0..iter {
        COUNTER.fetch_add(1, Ordering::Relaxed);
        tokio::task::yield_now().await;
    }
}