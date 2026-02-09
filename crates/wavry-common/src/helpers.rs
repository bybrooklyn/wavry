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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq_identical() {
        assert!(constant_time_eq("token123", "token123"));
        assert!(constant_time_eq("", ""));
        assert!(constant_time_eq("a", "a"));
    }

    #[test]
    fn test_constant_time_eq_different() {
        assert!(!constant_time_eq("token123", "token124"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("", "a"));
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert!(!constant_time_eq("short", "much_longer_string"));
        assert!(!constant_time_eq("a", ""));
        assert!(!constant_time_eq("abc", "ab"));
    }

    #[test]
    fn test_constant_time_eq_unicode() {
        assert!(constant_time_eq("émoji🎉", "émoji🎉"));
        assert!(!constant_time_eq("hello", "hėllo"));
    }
}
