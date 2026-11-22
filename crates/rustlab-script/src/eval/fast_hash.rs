//! A small, fast hasher for the evaluator's variable map.
//!
//! Every variable read and write hashes its name. The standard library's
//! SipHash resists collision flooding from untrusted keys, which variable
//! names written in the script itself do not need, and it was the largest
//! single cost of a scalar loop (`acc = acc + k`). This is the multiply-
//! rotate hash rustc uses for its own maps (Fx), eight bytes per step.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// `HashMap` keyed with [`FxHasher`].
pub type FxHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().expect("8 bytes")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            // Zero-padded, with the tail length in the free top byte, so
            // "a" and "a\0" differ.
            let mut last = [0u8; 8];
            last[..rest.len()].copy_from_slice(rest);
            last[7] = rest.len() as u8;
            self.add(u64::from_le_bytes(last));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_behaves_like_a_map() {
        let mut m: FxHashMap<String, usize> = FxHashMap::default();
        let names: Vec<String> = (0..2000).map(|k| format!("v{k}_{}", "x".repeat(k % 13))).collect();
        for (k, n) in names.iter().enumerate() {
            m.insert(n.clone(), k);
        }
        assert_eq!(m.len(), names.len());
        for (k, n) in names.iter().enumerate() {
            assert_eq!(m.get(n.as_str()), Some(&k));
        }
        assert_eq!(m.get("v1_"), None);
    }

    #[test]
    fn short_names_differing_in_the_tail_hash_apart() {
        let h = |s: &str| {
            let mut x = FxHasher::default();
            std::hash::Hash::hash(s, &mut x);
            x.finish()
        };
        assert_ne!(h("a"), h("b"));
        assert_ne!(h("abcdefgh1"), h("abcdefgh2"));
        assert_ne!(h(""), h("\0"));
        assert_ne!(h("a"), h("a\0"));
    }
}
