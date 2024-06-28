use std::cell::{Cell, RefCell};
use std::fmt::{Debug, Formatter};
use std::ops::{Add, AddAssign};
use crossbeam::channel::Sender;
use rustc_hash::FxHashMap;

pub struct MetricsContext {
    increments: Cell<usize>,
    snapshot: Snapshot,
    tx: RefCell<Option<Sender<Snapshot>>>,
}

impl MetricsContext {
    pub fn new() -> Self {
        Self {
            increments: Cell::new(0),
            snapshot: Snapshot::new(),
            tx: RefCell::new(None),
        }
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn increment<M: Metric>(&self, metric: M) {
        self.increments.set(self.increments.get() + 1);
        self.snapshot.increment(metric);

        if self.increments.get() % 50000 == 0 {
            self.tx.borrow().as_ref().unwrap().send(self.snapshot.clone()).unwrap();
            self.snapshot.clear();
        }
    }

    pub fn connect(&self, tx: Sender<Snapshot>) {
        *self.tx.borrow_mut() = Some(tx);
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
pub struct Snapshot(RefCell<FxHashMap<MetricKey, MetricValue>>);

impl Debug for Snapshot {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0.borrow())
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
        Self(RefCell::new(FxHashMap::default()))
    }

    pub fn clear(&self) {
        self.0.borrow_mut().values_mut().for_each(|v| *v = MetricValue::default());
    }

    pub fn increment<M: Metric>(&self, metric: M) {
        let (key, value) = metric.into_metric();
        *self.0.borrow_mut().entry(key).or_insert_with(|| MetricValue::default()) += value;
    }

    pub fn merge(&self, other: &Self) {
        let mut this = self.0.borrow_mut();
        let other = other.0.borrow();

        for (key, value) in other.iter() {
            *this.entry(key.clone()).or_insert_with(|| MetricValue::default()) += *value;
        }
    }

    pub fn get(&self, key: MetricKey) -> MetricValue {
        self.0.borrow()[&key]
    }
}

thread_local! {
    // TODO: const context makes it faster but hashmap does not support it
    // pub static METRICS: Snapshot = Snapshot::new();
    pub static METRICS_CTX: MetricsContext = MetricsContext::new();
}

pub const KEY: &str = "metric.1";

pub async fn do_work(tx: Sender<Snapshot>, iter: u64) {
    METRICS_CTX.with(|m| {
        m.connect(tx)
    });

    for _ in 0..iter {
        METRICS_CTX.with(|m| {
            m.increment(Counter(KEY, 1));
        });
        tokio::task::yield_now().await;
    }
}

