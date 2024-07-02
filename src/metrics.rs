use std::cell::{RefCell};
use std::fmt::{Debug, Formatter};
use std::ops::{Add, AddAssign};
use std::time::{Duration, Instant};
use crossbeam::channel::Sender;
use rustc_hash::FxHashMap;

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
            self.tx.borrow().as_ref().unwrap().send(copy).unwrap();
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
    store: FxHashMap<MetricKey, MetricValue>,
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
    fn into_metric(self) -> (MetricKey, MetricValue);
}

#[allow(dead_code)]
pub struct Counter(pub &'static str, pub u64);

impl Metric for Counter {
    fn into_metric(self) -> (MetricKey, MetricValue) {
        (MetricKey, MetricValue(self.1))
    }
}


impl Snapshot {
    pub fn new() -> Self {
        Self {
            store: FxHashMap::default(),
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
        *self.store.entry(key).or_insert_with(|| MetricValue::default()) += value;
        self.cnt += 1;

        self.cnt >= 50_000
    }

    pub fn merge(&mut self, other: &Self) {
        for (key, value) in other.store.iter() {
            *self.store.entry(key.clone()).or_insert_with(|| MetricValue::default()) += *value;
        }
    }

    pub fn get(&self, key: &MetricKey) -> Option<MetricValue> {
        self.store.get(key).copied()
    }
}

thread_local! {
    // TODO: const context makes it faster but hashmap does not support it
    // given that I need connect, it should be possible to use const
    pub static METRICS_CTX: MetricsContext = const { MetricsContext::new() }
}

pub const KEY: &str = "metric.1";

pub async fn do_work_async() {
    loop {
        METRICS_CTX.with(|m| {
            m.increment(Counter(KEY, 1));
        });
        tokio::task::yield_now().await;
    }
}

