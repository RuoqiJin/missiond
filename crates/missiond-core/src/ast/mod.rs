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
