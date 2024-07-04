use std::cell::{RefCell};
use std::fmt::{Debug, Formatter};
use std::ops::{Add, AddAssign};
use crossbeam::channel::Sender;
use crate::dimensions::{HelperIdentity, MetricName, MetricStore};

pub struct MetricsContext {
    snapshot: RefCell<Option<Snapshot>>,
    tx: RefCell<Option<Sender<Snapshot>>>,
}

impl MetricsContext {
    pub const fn new() -> Self {
        Self {
            snapshot: RefCell::new(None),
            tx: RefCell::new(None),
        }
    }

    pub fn take_snapshot(&self) -> Snapshot {
        self.snapshot.borrow_mut().as_mut().unwrap().take()
    }

    // #[inline]
    pub fn increment<M: Metric>(&self, metric: M) {
        let mut snapshot = self.snapshot.borrow_mut();
        let snapshot_mut = snapshot.as_mut().unwrap();
        if snapshot_mut.increment(metric) && self.tx.borrow().is_some() {
            let copy = snapshot_mut.take();
            let _ = self.tx.borrow().as_ref().unwrap().send(copy);
        }
    }

    pub fn connect(&self, tx: Sender<Snapshot>) {
        *self.tx.borrow_mut() = Some(tx);
        *self.snapshot.borrow_mut() = Some(Snapshot::new());
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct MetricKey;

#[derive(Copy, Clone, Debug, Default, Ord, PartialOrd, Eq, PartialEq)]
pub struct MetricValue(pub u64);

impl Add for MetricValue {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for MetricValue {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

#[derive(Clone)]
pub struct Snapshot {
    store: MetricStore,
    cnt: usize
}

impl Debug for Snapshot {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("cnt", &self.cnt)
            .field("store", &self.store)
            .finish()
    }
}

pub trait Metric: Sized {
    fn into_metric(&self) -> (MetricName, MetricValue);
}

#[allow(dead_code)]
pub struct Counter(pub &'static str, pub u64);

impl Metric for Counter {
    fn into_metric(&self) -> (MetricName, MetricValue) {
        (MetricName::with_no_labels(self.0), MetricValue(self.1))
    }
}

pub struct OneDimensionCounter(pub &'static str, pub HelperIdentity, pub u64);

impl Metric for OneDimensionCounter {
    fn into_metric(&self) -> (MetricName, MetricValue) {
        (MetricName::with_one_label(self.0, "dest", &self.1), MetricValue(self.2))
    }
}


impl Snapshot {
    pub fn new() -> Self {
        Self {
            store: Default::default(),
            cnt: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cnt == 0
    }

    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::new())
    }

    // #[inline]
    pub fn increment<M: Metric>(&mut self, metric: M) -> bool {
        let (key, value) = metric.into_metric();
        self.store.update(&key, value.0);
        self.cnt += 1;

        self.cnt >= 50_000
    }

    pub fn merge(&mut self, other: Self) {
        self.store.merge(other.store);
    }

    pub fn get(&self, key: &MetricName) -> Option<u64> {
        self.store.get_counter(key)
    }

    pub fn get_all_dims(&self, key: &'static str) -> Option<u64> {
        self.store.get_counter_all_dim(key)
    }
}

thread_local! {
    pub static METRICS_CTX: MetricsContext = const { MetricsContext::new() }
}

pub const KEY: &str = "metric";

pub async fn do_work_async() {
    loop {
        let mut iter = 0;
        METRICS_CTX.with(|m| {
            m.increment(Counter(KEY, 1));
        });
        iter += 1;
        if iter % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
}

pub async fn do_work_async_one_dim() {
    loop {
        let mut iter = 0;
        METRICS_CTX.with(|m| {
            if iter % 3 == 0 {
                m.increment(OneDimensionCounter(KEY, HelperIdentity::H3, 1));
            } else if iter & (iter - 1) == 0 {
                m.increment(OneDimensionCounter(KEY, HelperIdentity::H2, 1));
            } else {
                m.increment(OneDimensionCounter(KEY, HelperIdentity::H1, 1));
            }
        });
        iter += 1;
        if iter % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
}

