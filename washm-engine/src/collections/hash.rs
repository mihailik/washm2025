use std::collections::HashMap;
use std::hash::Hasher;

pub type FastHashMap<K, V> = HashMap<K, V>;

/// Compute a 64-bit hash of a byte slice.
pub fn ahash_bytes(bytes: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    // no external imports required for these tests

    #[test]
    fn test_fast_hashmap_basic() {
        let mut map: FastHashMap<String, usize> = FastHashMap::default();
        map.insert("one".to_string(), 1);
        map.insert("two".to_string(), 2);
        assert_eq!(map.get("one"), Some(&1));
        assert_eq!(map.get("none"), None);
    }

    #[test]
    fn test_ahash_bytes() {
        let out = ahash_bytes(b"hello");
        // Just assert we produce a u64 value (not zero unless input empty)
        assert_ne!(out, 0);
        let out2 = ahash_bytes(b"hello");
        assert_eq!(out, out2);
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn ahash_is_deterministic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            let out1 = ahash_bytes(&bytes);
            let out2 = ahash_bytes(&bytes);
            prop_assert_eq!(out1, out2);
        }
    }

    proptest! {
        #[test]
        fn fastmap_ops(ops in proptest::collection::vec((".*", any::<u64>()), 0..128)) {
            let mut map: FastHashMap<String, u64> = FastHashMap::default();
            for (k, v) in ops {
                map.insert(k.clone(), v);
                if let Some(found_v) = map.get(&k) { assert_eq!(*found_v, v); }
            }
        }
    }
}
