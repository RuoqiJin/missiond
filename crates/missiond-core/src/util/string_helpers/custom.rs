//! Safe string truncation — never breaks char boundaries.
//! Zero external deps, pure Rust std.

use super::generated::StringHelpers;

pub struct StringHelpersImpl;

impl StringHelpers for StringHelpersImpl {
    /// Walk backwards from max_bytes to find a char boundary.
    /// `str::is_char_boundary(i)` is O(1) — this is always fast.
    fn safe_byte_truncate(s: &str, max_bytes: usize) -> &str {
        if s.len() <= max_bytes {
            return s;
        }
        // Find the largest index <= max_bytes that is a char boundary
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }

    /// Use char_indices to find the byte offset of the Nth char.
    fn safe_char_truncate(s: &str, max_chars: usize) -> &str {
        match s.char_indices().nth(max_chars) {
            Some((byte_idx, _)) => &s[..byte_idx],
            None => s, // string has <= max_chars characters
        }
    }
}

/// Convenience free functions (most callers want these, not the trait).
#[inline]
pub fn safe_byte_truncate(s: &str, max_bytes: usize) -> &str {
    StringHelpersImpl::safe_byte_truncate(s, max_bytes)
}

#[inline]
pub fn safe_char_truncate(s: &str, max_chars: usize) -> &str {
    StringHelpersImpl::safe_char_truncate(s, max_chars)
}

// ═══════════════════════════════════════════════════════════
// TDD Test Suite — 11 cases from Forge declaration
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // --- safe_byte_truncate ---
    #[test]
    fn test_ascii_within() {
        assert_eq!(safe_byte_truncate("hello", 10), "hello");
    }

    #[test]
    fn test_ascii_exact() {
        assert_eq!(safe_byte_truncate("hello", 5), "hello");
    }

    #[test]
    fn test_ascii_cut() {
        assert_eq!(safe_byte_truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_cjk_boundary_4() {
        // "abc" = 3 bytes, "你" = 3 bytes (starts at byte 3, ends at byte 6)
        // max_bytes=4 → can't fit "你" (needs bytes 3..6), backs up to byte 3
        assert_eq!(safe_byte_truncate("abc你好", 4), "abc");
    }

    #[test]
    fn test_cjk_boundary_6() {
        // "abc" = 3 bytes, "你" = 3 bytes (bytes 3..6)
        // max_bytes=6 → fits "abc你"
        assert_eq!(safe_byte_truncate("abc你好", 6), "abc你");
    }

    #[test]
    fn test_box_drawing() {
        // "ab" = 2 bytes, "═" = 3 bytes (U+2550, bytes 2..5)
        // max_bytes=4 → can't fit "═" (needs bytes 2..5), backs up to byte 2
        assert_eq!(safe_byte_truncate("ab═cd", 4), "ab");
    }

    #[test]
    fn test_empty() {
        assert_eq!(safe_byte_truncate("", 100), "");
    }

    #[test]
    fn test_zero_max() {
        assert_eq!(safe_byte_truncate("hello", 0), "");
    }

    // --- safe_char_truncate ---
    #[test]
    fn test_char_within() {
        assert_eq!(safe_char_truncate("hello", 10), "hello");
    }

    #[test]
    fn test_char_cut() {
        assert_eq!(safe_char_truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_char_cjk() {
        assert_eq!(safe_char_truncate("你好世界", 2), "你好");
    }
}
