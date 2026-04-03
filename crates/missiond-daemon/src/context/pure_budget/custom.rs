//! Pure function implementations for token budget calculations.
//!
//! All functions are stateless, no &self, no side effects.
//! Zero external crates — pure Rust std only.

use super::generated::TokenBudget;

pub struct TokenBudgetImpl;

impl TokenBudget for TokenBudgetImpl {
    /// Heuristic: ASCII chars ÷ 4 ≈ tokens, CJK chars ÷ 2 ≈ tokens.
    /// This is a rough estimate for prompt budget planning, not billing.
    fn estimate_tokens(text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }

        let mut ascii_chars = 0usize;
        let mut cjk_chars = 0usize;

        for c in text.chars() {
            if is_cjk(c) {
                cjk_chars += 1;
            } else {
                ascii_chars += 1;
            }
        }

        // ASCII: ~4 chars per token (ceil), CJK: ~2 chars per token (ceil)
        let ascii_tokens = (ascii_chars + 3) / 4;
        let cjk_tokens = (cjk_chars + 1) / 2;

        ascii_tokens + cjk_tokens
    }

    /// Diminishing returns allocation: first source gets 60%, rest splits remainder.
    /// This reflects that the first/primary context source is usually most relevant.
    fn allocate_budget(total: usize, source_count: usize) -> Vec<usize> {
        if source_count == 0 {
            return vec![];
        }
        if source_count == 1 {
            return vec![total];
        }

        // First source gets 60%, remainder split evenly among the rest
        let primary = (total * 60) / 100;
        let remainder = total - primary;
        let per_secondary = remainder / (source_count - 1);

        let mut result = Vec::with_capacity(source_count);
        result.push(primary);

        let mut allocated = primary;
        for i in 1..source_count {
            if i == source_count - 1 {
                // Last source gets the remainder to avoid rounding loss
                result.push(total - allocated);
            } else {
                result.push(per_secondary);
                allocated += per_secondary;
            }
        }
        result
    }
}

/// CJK Unified Ideographs + common CJK ranges.
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   | // CJK Unified Ideographs
        '\u{3400}'..='\u{4DBF}'   | // CJK Extension A
        '\u{F900}'..='\u{FAFF}'   | // CJK Compatibility Ideographs
        '\u{3000}'..='\u{303F}'   | // CJK Symbols and Punctuation
        '\u{3040}'..='\u{309F}'   | // Hiragana
        '\u{30A0}'..='\u{30FF}'   | // Katakana
        '\u{AC00}'..='\u{D7AF}'     // Hangul Syllables
    )
}

// ═══════════════════════════════════════════════════════════
// TDD Test Suite — 4 cases from Forge declaration
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // --- estimate_tokens ---
    #[test]
    fn test_estimate_tokens_english() {
        // "hello world" = 11 ASCII chars, ceil(11/4) = 3
        let result = TokenBudgetImpl::estimate_tokens("hello world");
        assert_eq!(result, 3);
    }

    #[test]
    fn test_estimate_tokens_empty() {
        let result = TokenBudgetImpl::estimate_tokens("");
        assert_eq!(result, 0);
    }

    // --- allocate_budget ---
    #[test]
    fn test_allocate_budget_single() {
        let result = TokenBudgetImpl::allocate_budget(1000, 1);
        assert_eq!(result, vec![1000]);
    }

    #[test]
    fn test_allocate_budget_two() {
        let result = TokenBudgetImpl::allocate_budget(1000, 2);
        assert_eq!(result, vec![600, 400]);
    }
}
