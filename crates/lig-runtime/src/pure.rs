//! Pure builtin helpers for whitelisted view-expression calls (§6.5 Allow
//! tier). `no_std + alloc`, no host access, no state — the compiler emits
//! direct calls to these from generated code.

use alloc::format;
use alloc::string::String;

/// `items.join(sep)` on a list of strings.
#[must_use]
pub fn join(items: &[String], sep: &str) -> String {
    items.join(sep)
}

/// Stable `u64` key for a string list item (keyed reconcile, §7.5).
#[must_use]
pub fn key_str(s: &str) -> u64 {
    hash_fnv1a64(s.as_bytes())
}

/// Stable `u64` key for an integer list item.
#[must_use]
pub const fn key_int(v: i64) -> u64 {
    v as u64
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const fn hash_fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

fn pow10(n: u32) -> f64 {
    let mut out = 1.0_f64;
    for _ in 0..n {
        out *= 10.0;
    }
    out
}

/// `n.toFixed(digits)` — fixed-point decimal rendering (v1: 0–18 digits).
///
/// # Panics
/// Never — out-of-range digit counts clamp to the representable window.
#[must_use]
pub fn f64_to_fixed(v: f64, digits: u32) -> String {
    let digits = digits.min(18);
    let mult = pow10(digits);
    // Round half away from zero without libm (no_std): scale, offset, cast.
    let scaled = v * mult;
    let rounded = if scaled >= 0.0 {
        (scaled + 0.5) as i64
    } else {
        (scaled - 0.5) as i64
    };
    let out = rounded as f64 / mult;
    format!("{out}")
}

/// `n.toString()` for integers.
#[must_use]
pub fn i64_to_string(v: i64) -> String {
    let mut buf = [0u8; 20];
    crate::format_i64(&mut buf, v).into()
}

#[cfg(test)]
mod tests {
    use super::{f64_to_fixed, i64_to_string, join, key_int, key_str};
    use alloc::{string::String, vec, vec::Vec};

    #[test]
    fn joins_with_separator() {
        let items: Vec<String> = vec![String::from("a"), String::from("b")];
        assert_eq!(join(&items, ", "), "a, b");
        assert_eq!(join(&[], ", "), "");
    }

    #[test]
    fn keys_are_stable() {
        assert_eq!(key_str("alpha"), key_str("alpha"));
        assert_ne!(key_str("alpha"), key_str("beta"));
        assert_eq!(key_int(7), key_int(7));
        assert_ne!(key_int(7), key_int(8));
    }

    #[test]
    fn fixed_and_to_string_render() {
        assert_eq!(f64_to_fixed(12.345, 2), "12.35");
        assert_eq!(i64_to_string(-42), "-42");
    }
}
