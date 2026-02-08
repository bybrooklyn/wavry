//! Common helper functions for Wavry.

/// Performs a constant-time comparison of two strings.
/// This is used to prevent timing attacks when comparing security tokens.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    if a_bytes.len() != b_bytes.len() {
        return false;
    }

    let mut diff = 0u8;
    for (lhs, rhs) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= lhs ^ rhs;
    }
    diff == 0
}
