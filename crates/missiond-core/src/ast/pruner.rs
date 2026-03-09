//! AstPruner trait — language-agnostic interface for code structure extraction.

use super::CodeNode;

/// Language-specific AST pruner. Extracts structural skeletons from source code.
pub trait AstPruner: Send + Sync {
    /// Parse source code and extract all top-level code nodes.
    /// Returns empty vec on parse failure (graceful degradation).
    fn extract_nodes(&self, source: &str) -> Vec<CodeNode>;

    /// File extensions this pruner handles (e.g. ["rs"] for Rust).
    fn extensions(&self) -> &[&str];
}

/// Detect language from file extension and return the appropriate pruner.
pub fn pruner_for_extension(ext: &str) -> Option<Box<dyn AstPruner>> {
    match ext {
        "rs" => Some(Box::new(super::rust_pruner::RustPruner)),
        // "ts" | "tsx" => Some(Box::new(super::ts_pruner::TsPruner)),
        // "py" => Some(Box::new(super::python_pruner::PythonPruner)),
        _ => None,
    }
}

/// Extract nodes from a file, auto-detecting language from path extension.
pub fn extract_from_file(file_path: &str, source: &str) -> Vec<CodeNode> {
    let ext = file_path.rsplit('.').next().unwrap_or("");
    match pruner_for_extension(ext) {
        Some(pruner) => pruner.extract_nodes(source),
        None => Vec::new(),
    }
}
