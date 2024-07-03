use std::fmt::Display;
use std::hash::{BuildHasher, Hash, Hasher};
use hashbrown::Equivalent;
use hashbrown::hash_map::RawEntryMut;
use rustc_hash::FxBuildHasher;


trait LabelValue : Display {
    fn as_u64(&self) -> u64;
}

struct MetricName<'tag, const LABELS: usize = 5> {
    key: &'static str,
    labels: [Option<(&'static str, &'tag dyn LabelValue)>; LABELS],
}

impl <const LABELS: usize> MetricName<'_, LABELS> {
    /// this should be the majority of the cost for dimensionalities. This operation needs to happen
    /// once per metric + all combination of dimensionalities.
    pub fn clone_into_owned(&self) -> OwnedMetricName<LABELS> {
        // todo: we computed hashes for labels already, so we could re-use them if it is expensive
        // to recompute
        OwnedMetricName {
            key: self.key,
            labels: self.labels.map(|v| v.map(|v| (v.0, v.1.as_u64(), v.1.to_string())))
        }
    }
}


impl <const LABELS: usize> Hash for MetricName<'_, LABELS> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.key.as_bytes());
        for x in self.labels {
            if let Some((label_key, label_val)) = x {
                state.write(label_key.as_bytes());
                state.write_u64(label_val.as_u64());
            }
        }
    }
}

struct OwnedMetricName<const LABELS: usize = 5> {
    key: &'static str,
    labels: [Option<(&'static str, u64, String)>; LABELS]
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
                    a.0.eq(b.0) && a.1.eq(&b.1.as_u64())
                },
                _ => false
            }
        })
    }
}

#[derive(Default)]
struct MetricStore {
    buf: hashbrown::HashMap<OwnedMetricName, u64, FxBuildHasher>
}


impl MetricStore {
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

#[cfg_attr(test, global_allocator)]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[cfg(test)]
mod tests {
    use std::fmt::{Display, Formatter};
    use crate::dimensions::{LabelValue, MetricName, MetricStore};

    enum HelperIdentity {
        H1,
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
                HelperIdentity::H1 => 1,
                HelperIdentity::H2 => 2,
                HelperIdentity::H3 => 3,
            }
        }
    }

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
