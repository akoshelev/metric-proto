use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant};


pub struct AtomicContext {
    inner: RefCell<Option<Arc<AtomicU64>>>
}

impl AtomicContext {
    pub const fn new() -> Self {
        Self {
            inner: RefCell::new(None)
        }
    }

    pub fn connect(&self, v: Arc<AtomicU64>) {
        *self.inner.borrow_mut() = Some(v);
    }

    pub fn increment(&self) {
        self.inner.borrow().as_ref().unwrap().fetch_add(1, Ordering::Relaxed);
    }
}

thread_local! {
    // TODO: const context makes it faster but hashmap does not support it
    // given that I need connect, it should be possible to use const
    pub static ATOMIC_CTX: AtomicContext = const { AtomicContext::new() }
}

/// Simple atomic increments
pub async fn do_work_async() {
    loop {
        // let start = Instant::now();
        let mut iter = 0;
        ATOMIC_CTX.with(|m| {
            m.increment();
        });
        iter += 1;
        if iter % 100 == 0 {
            tokio::task::yield_now().await
        }
        // super::sleep_or_yield(start.elapsed()).await;
    }
}