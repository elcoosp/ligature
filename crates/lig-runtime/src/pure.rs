//! Pure builtin helpers for whitelisted view-expression calls (§6.5 Allow
//! tier). `no_std + alloc`, no host access, no state — the compiler emits
//! direct calls to these from generated code.
//!
//! Key hashing is NOT here: the canonical FNV-1a key hash lives in
//! [`crate::reconcile::key_hash`] (pinned by its own FNV vectors) and that
//! is what the codegen emits — this module once carried a duplicate
//! (`key_str`/`key_int`) that nothing called, removed before the two
//! implementations could drift.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// `items.join(sep)` on a list of strings.
#[must_use]
pub fn join(items: &[String], sep: &str) -> String {
    items.join(sep)
}

/// `[...a, b]` — list concatenation for the `arr.concat` builtin (§7.5).
#[must_use]
pub fn concat<T: Clone>(a: &[T], b: &[T]) -> Vec<T> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    out
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
    use super::{f64_to_fixed, i64_to_string, join};
    use alloc::{string::String, vec, vec::Vec};

    #[test]
    fn joins_with_separator() {
        let items: Vec<String> = vec![String::from("a"), String::from("b")];
        assert_eq!(join(&items, ", "), "a, b");
        assert_eq!(join(&[], ", "), "");
    }

    #[test]
    fn fixed_and_to_string_render() {
        assert_eq!(f64_to_fixed(12.345, 2), "12.35");
        assert_eq!(i64_to_string(-42), "-42");
    }
}
