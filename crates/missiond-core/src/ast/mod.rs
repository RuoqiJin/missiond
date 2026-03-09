//! Code context engine — tree-sitter AST → language-native stubs.
//!
//! Feature-gated behind `code-context`. Extracts structural skeletons from source code,
//! preserving signatures, doc comments, and call relationships while eliding implementation.

#[cfg(feature = "code-context")]
mod pruner;
#[cfg(feature = "code-context")]
mod rust_pruner;

#[cfg(feature = "code-context")]
pub use pruner::*;
#[cfg(feature = "code-context")]
pub use rust_pruner::RustPruner;

use serde::{Deserialize, Serialize};

/// A single extracted code node (function, struct, impl, trait, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeNode {
    /// Identifier only (e.g. "autopilot_tick")
    pub name: String,
    /// Node kind: function, struct, enum, impl, trait, mod, const, type
    pub node_type: String,
    /// Full signature with params/return type
    pub signature: String,
    /// Start line (1-indexed)
    pub start_line: usize,
    /// End line (1-indexed)
    pub end_line: usize,
    /// Whether the item is exported (pub)
    pub is_exported: bool,
    /// Extracted doc comment (/// or /** */)
    pub docstring: Option<String>,
    /// Language-native stub content
    pub stub_content: String,
    /// Called function/method names
    pub calls: Vec<String>,
    /// Beacon tags extracted from `// @beacon: tag1, tag2` comments above the node
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beacons: Vec<String>,
}

/// Extract beacon tags from a comment block. Looks for `// @beacon: tag1, tag2` patterns.
pub fn extract_beacon_tags(comments: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for line in comments.lines() {
        let trimmed = line.trim();
        // Match: // @beacon: ... or /// @beacon: ... or # @beacon: ...
        let after = if let Some(rest) = trimmed.strip_prefix("//") {
            rest.trim_start_matches('/').trim()
        } else if let Some(rest) = trimmed.strip_prefix('#') {
            rest.trim()
        } else {
            continue;
        };
        if let Some(beacon_part) = after.strip_prefix("@beacon:") {
            for tag in beacon_part.split(',') {
                let tag = tag.trim();
                if !tag.is_empty() {
                    tags.push(tag.to_string());
                }
            }
        }
    }
    tags.sort();
    tags.dedup();
    tags
}

impl CodeNode {
    /// Build the embedding text representation for vector search.
    pub fn embedding_text(&self, file_path: &str) -> String {
        let mut text = format!(
            "Name: {} | Path: {} > {} {}",
            self.name, file_path, self.node_type, self.signature
        );
        if let Some(ref doc) = self.docstring {
            text.push('\n');
            text.push_str(doc);
        }
        if !self.calls.is_empty() {
            text.push_str("\nCalls: ");
            text.push_str(&self.calls.join(", "));
        }
        text
    }

    /// Build the ID for deduplication: hash(repo + file_path + node_type + name + start_line)
    pub fn build_id(&self, repo: &str, file_path: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        repo.hash(&mut hasher);
        file_path.hash(&mut hasher);
        self.node_type.hash(&mut hasher);
        self.name.hash(&mut hasher);
        self.start_line.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_beacon_tags() {
        // Single tag
        assert_eq!(
            extract_beacon_tags("// @beacon: memory-extraction"),
            vec!["memory-extraction"]
        );

        // Multiple tags on one line
        assert_eq!(
            extract_beacon_tags("// @beacon: memory-extraction, autopilot-scheduling"),
            vec!["autopilot-scheduling", "memory-extraction"]
        );

        // Multiple lines
        assert_eq!(
            extract_beacon_tags("// @beacon: feat-a\n// @beacon: feat-b"),
            vec!["feat-a", "feat-b"]
        );

        // Doc comment with @beacon (///)
        assert_eq!(
            extract_beacon_tags("/// @beacon: documented-feature"),
            vec!["documented-feature"]
        );

        // No beacon
        assert!(extract_beacon_tags("// regular comment").is_empty());
        assert!(extract_beacon_tags("/// doc comment").is_empty());

        // Deduplication
        assert_eq!(
            extract_beacon_tags("// @beacon: dup\n// @beacon: dup"),
            vec!["dup"]
        );
    }
}
