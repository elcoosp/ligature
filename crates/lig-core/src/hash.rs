//! Stable, dependency-free hashing for fingerprints (§5.6).
//!
//! FNV-1a 64: deterministic across releases and platforms — a requirement for
//! byte-reproducible fixtures and digest-gated emission (D-8/D-9). We
//! deliberately avoid `std`-hash (seeded, unstable) and `rustc-hash`
//! (non-contractual).

/// FNV-1a 64-bit offset basis.
const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64-bit hash of `bytes`.
#[must_use]
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = OFFSET_BASIS;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(PRIME);
        i += 1;
    }
    hash
}

/// Hash the canonical JSON encoding of a serializable value.
///
/// # Panics
/// Only if `serde_json` cannot serialize `T` — impossible for the IR types,
/// which are plain data.
#[must_use]
pub fn hash_canonical<T: serde::Serialize>(value: &T) -> u64 {
    let bytes = serde_json::to_vec(value).expect("canonical serialization of IR data");
    fnv1a64(&bytes)
}

#[cfg(test)]
mod tests {
    use super::{fnv1a64, hash_canonical};
    use serde::Serialize;

    #[test]
    fn fnv1a64_reference_vectors() {
        // Published FNV-1a 64 test vectors.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn hash_canonical_is_field_order_stable() {
        #[derive(Serialize)]
        struct A {
            x: u32,
            y: u32,
        }
        let a = A { x: 1, y: 2 };
        assert_eq!(hash_canonical(&a), hash_canonical(&a));
        assert_ne!(hash_canonical(&a), hash_canonical(&A { x: 2, y: 1 }));
    }
}
