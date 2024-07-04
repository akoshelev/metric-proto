use std::array;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{BuildHasher, Hash, Hasher};
use std::iter::zip;
use hashbrown::hash_map::RawEntryMut;
use rustc_hash::FxBuildHasher;

pub trait LabelValue : Display + Send {
    fn as_u64(&self) -> u64;

    fn boxed(&self) -> Box<dyn LabelValue>;
}

pub struct MetricName<'tag, const LABELS: usize = 5> {
    key: &'static str,
    labels: [Option<(&'static str, &'tag dyn LabelValue)>; LABELS],
}

impl <const LABELS: usize> MetricName<'static, LABELS> {
    pub fn with_no_labels(name: &'static str) -> Self {
        Self {
            key: name,
            labels: array::from_fn(|_| None),
        }
    }
}

impl <'a, const LABELS: usize> MetricName<'a, LABELS> {

    pub fn with_one_label<R: LabelValue + 'a>(name: &'static str, label_name: &'static str, label_value: &'a R) -> Self {

        let labels: [_; LABELS] = array::from_fn(move |i| {
            if i == 0 {
                Some((label_name, label_value as &dyn LabelValue))
            } else {
                None
            }
        });


        Self {
            key: name,
            labels,
            // labels: array::from_fn(|i| if i != 0 { None } else { Some((label_name, label_value)) }),
        }
    }

    /// this should be the majority of the cost for dimensionalities. This operation needs to happen
    /// once per metric + all combination of dimensionalities.
    fn clone_into_owned(&self) -> OwnedMetricName<LABELS> {
        // todo: we computed hashes for labels already, so we could re-use them if it is expensive
        // to recompute
        OwnedMetricName {
            key: self.key,
            labels: self.labels.map(|v| v.map(|v| (v.0, v.1.as_u64(), v.1.boxed())))
        }
    }
}

fn compute_label_hash<H: Hasher>(state: &mut H, label: &Option<(&'static str, &dyn LabelValue)>) {
    if let Some((label_key, label_val)) = label {
        state.write(label_key.as_bytes());
        state.write_u64(label_val.as_u64());
    }
}

impl Hash for MetricName<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.key.as_bytes());
        let [l0, l1, l2, l3, l4] = &self.labels;
        compute_label_hash(state, l0);
        compute_label_hash(state, l1);
        compute_label_hash(state, l2);
        compute_label_hash(state, l3);
        compute_label_hash(state, l4);
        // for x in self.labels {
        //     if let Some((label_key, label_val)) = x {
        //         state.write(label_key.as_bytes());
        //         state.write_u64(label_val.as_u64());
        //     }
        // }
    }
}

struct OwnedMetricName<const LABELS: usize = 5> {
    key: &'static str,
    labels: [Option<(&'static str, u64, Box<dyn LabelValue>)>; LABELS]
}

impl <const LABELS: usize> Clone for OwnedMetricName<LABELS> {
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            labels: self.labels.each_ref().map(|v| v.as_ref().map(|(label, hash, val)| (*label, *hash, val.boxed())))
        }
    }
}

impl <const LABELS: usize> OwnedMetricName<LABELS> {
    pub fn same(&self, other: &Self) -> bool {
        self.key.eq(other.key) && zip(&self.labels, &other.labels).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => {
                a.0 == b.0 && a.1 == b.1
            }
            (None, None) => true,
            _ => false,
        })
    }
}

impl <const LABELS: usize> Debug for OwnedMetricName<LABELS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // TODO: labels
        f.debug_struct("OwnedMetricName")
            .field("key", &self.key)
            .finish()
    }
}

/// This must be consistent with [`MetricName`] hash implementation
impl <const LABELS: usize> Hash for OwnedMetricName<LABELS> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.key.as_bytes());
        for x in &self.labels {
            if let Some((label_key, hash, _)) = x {
                state.write(label_key.as_bytes());
                state.write_u64(*hash);
            }
        }
    }
}


impl PartialEq<MetricName<'_>> for &OwnedMetricName {
    fn eq(&self, other: &MetricName) -> bool {
        if !self.key.eq(other.key) {
            return false
        }

        std::iter::zip(&self.labels, &other.labels).all(|(a, b)| {
            match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    // compare the label name and hash of label value only, to avoid walking through
                    // the owned string. Again, this is not fool-proof and assumes cooperation from
                    // label implementors
                    a.1.eq(&b.1.as_u64()) && a.0.eq(b.0)
                },
                _ => false
            }
        })
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "ahash"), derive(Default))]
pub struct MetricStore {
    #[cfg(not(feature = "ahash"))]
    buf: hashbrown::HashMap<OwnedMetricName, u64, FxBuildHasher>,
    #[cfg(feature = "ahash")]
    buf: hashbrown::HashMap<OwnedMetricName, u64, ahash::RandomState>,
}

#[cfg(feature = "ahash")]
impl Default for MetricStore {
    fn default() -> Self {
        let state = ahash::RandomState::generate_with(0, 1, 2, 3);
        Self {
            buf: HashMap::with_hasher(state)
        }
    }
}


impl MetricStore {
    pub fn merge(&mut self, other: Self) {
        for (k, v) in other.buf {
            let hash = compute_hash(self.buf.hasher(), &k);
            let raw_entry = self.buf.raw_entry_mut();
            *raw_entry.from_hash(hash, |q| q.same(&k)).or_insert_with(|| (k, 0)).1 += v;
        }
    }

    pub fn update(&mut self, key: &MetricName, val: u64) {
        let hash = compute_hash(self.buf.hasher(), &key);
        let raw_entry = self.buf.raw_entry_mut();
        match raw_entry.from_hash(hash, |q| q.eq(key)) {
            RawEntryMut::Occupied(mut view) => {
                *view.get_mut() += val;
            }
            RawEntryMut::Vacant(view) => {
                view.insert(key.clone_into_owned(), val);
            }
        }
    }

    /// The cost of this operation can be higher than update and it is ok
    pub fn get_counter(&self, key: &MetricName) -> Option<u64> {
        let hash = compute_hash(self.buf.hasher(), &key);
        let raw_entry = self.buf.raw_entry();
        raw_entry.from_hash(hash, |q| q.eq(key)).map(|v| *v.1)
    }

    pub fn get_counter_all_dim(&self, key: &'static str) -> Option<u64> {
        let mut res = None;
        for (k, v) in &self.buf {
            if k.key == key {
                *res.get_or_insert(0) += v;
            }
        }

        res
        // let hash = compute_hash(self.buf.hasher(), &key);
        // let raw_entry = self.buf.raw_entry();
        // raw_entry.from_hash(hash, |q| q.eq(key)).map(|v| *v.1)
    }
}

fn compute_hash<B: BuildHasher, K: Hash + ?Sized>(hash_builder: &B, key: &K) -> u64 {
    let mut hasher = hash_builder.build_hasher();
    key.hash(&mut hasher);
    hasher.finish()
}


impl <'a, R: LabelValue> From<(&'static str, (&'static str, &'a R))> for MetricName<'a> {
    fn from(value: (&'static str, (&'static str, &'a R))) -> Self {
        Self {
            key: value.0,
            labels: [Some((value.1.0, value.1.1)), None, None, None, None],
        }
    }
}

#[cfg(test)]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[derive(Clone)]
#[repr(u8)]
pub enum HelperIdentity {
    H1 = 0,
    H2,
    H3
}

impl Display for HelperIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HelperIdentity::H1 => write!(f, "H1"),
            HelperIdentity::H2 => write!(f, "H2"),
            HelperIdentity::H3 => write!(f, "H3"),
        }
    }
}

impl LabelValue for HelperIdentity {
    fn as_u64(&self) -> u64 {
        match self {
            HelperIdentity::H1 => 0,
            HelperIdentity::H2 => 1,
            HelperIdentity::H3 => 2,
        }
    }

    fn boxed(&self) -> Box<dyn LabelValue> {
        Box::new(self.clone()) as Box<dyn LabelValue>
    }
}

#[cfg(test)]
mod tests {
    
    use crate::dimensions::{HelperIdentity, MetricName, MetricStore};


    #[test]
    fn one_dimension() {
        let mut store = MetricStore::default();


        let h1_metric: MetricName = ("foo", ("helper", &HelperIdentity::H1)).into();
        let h2_metric = ("foo", ("helper", &HelperIdentity::H2)).into();
        let h3_metric = ("foo", ("helper", &HelperIdentity::H3)).into();
        store.update(&h1_metric, 0);
        store.update(&h2_metric, 0);

        let _profiler = dhat::Profiler::builder().testing().build();
        for i in 0..10 {
            let h1_metric: MetricName = ("foo", ("helper", &HelperIdentity::H1)).into();
            // this should not cause allocations
            store.update(&h1_metric, i);
        }

        store.update(&h2_metric, 3);

        let stats = dhat::HeapStats::get();
        assert_eq!(stats.total_bytes, 0, "Some allocations occurred: {:?}", stats);

        assert_eq!(store.get_counter(&h1_metric), Some(45));
        assert_eq!(store.get_counter(&h2_metric), Some(3));
        assert_eq!(store.get_counter(&h3_metric), None);
    }
}
