//! Alloc-free integer formatting for text updates (§14.3 lists itoa-style
//! stack buffers as the mechanism; the runtime ships the helper so generated
//! code never allocates on the update path). Float formatting lands with M1
//! codegen behind the same buffer contract.

/// Format `v` into `buf`, returning the written subslice.
///
/// # Panics
/// If `buf` is shorter than 20 bytes (the `i64::MIN` width).
#[must_use]
pub fn format_i64(buf: &mut [u8], v: i64) -> &str {
    assert!(buf.len() >= 20, "format_i64 needs a 20-byte buffer");
    let negative = v < 0;
    let mut n = v.unsigned_abs();
    let mut end = buf.len();
    loop {
        end -= 1;
        buf[end] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if negative {
        end -= 1;
        buf[end] = b'-';
    }
    // Only ASCII digits were written; this cannot fail.
    core::str::from_utf8(&buf[end..]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::format_i64;

    #[test]
    fn integers_roundtrip() {
        let mut buf = [0u8; 20];
        assert_eq!(format_i64(&mut buf, 0), "0");
        assert_eq!(format_i64(&mut buf, 42), "42");
        assert_eq!(format_i64(&mut buf, -7), "-7");
        assert_eq!(format_i64(&mut buf, i64::MAX), "9223372036854775807");
        assert_eq!(format_i64(&mut buf, i64::MIN), "-9223372036854775808");
    }

    #[test]
    fn buffer_may_be_exactly_20() {
        let mut buf = [0u8; 20];
        assert_eq!(format_i64(&mut buf, 1), "1");
    }
}
