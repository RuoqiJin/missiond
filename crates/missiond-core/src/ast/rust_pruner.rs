//! Rust AST pruner — extracts function/struct/enum/impl/trait skeletons using tree-sitter.

use super::{AstPruner, CodeNode};
use tree_sitter::{Node, Parser};

pub struct RustPruner;

impl AstPruner for RustPruner {
    fn extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn extract_nodes(&self, source: &str) -> Vec<CodeNode> {
        let mut parser = Parser::new();
        let lang = tree_sitter_rust::LANGUAGE;
        if parser.set_language(&lang.into()).is_err() {
            return Vec::new();
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let src = source.as_bytes();
        let root = tree.root_node();
        let mut nodes = Vec::new();
        collect_nodes(root, src, &mut nodes);
        nodes
    }
}

fn collect_nodes(node: Node, src: &[u8], out: &mut Vec<CodeNode>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(n) = extract_function(child, src) {
                    out.push(n);
                }
            }
            "struct_item" => {
                if let Some(n) = extract_struct(child, src) {
                    out.push(n);
                }
            }
            "enum_item" => {
                if let Some(n) = extract_enum(child, src) {
                    out.push(n);
                }
            }
            "impl_item" => {
                extract_impl(child, src, out);
            }
            "trait_item" => {
                if let Some(n) = extract_trait(child, src) {
                    out.push(n);
                }
            }
            "mod_item" => {
                // Recurse into inline modules
                if let Some(body) = child.child_by_field_name("body") {
                    collect_nodes(body, src, out);
                }
            }
            "const_item" | "static_item" | "type_item" => {
                if let Some(n) = extract_simple_item(child, src) {
                    out.push(n);
                }
            }
            _ => {}
        }
    }
}

fn extract_function(node: Node, src: &[u8]) -> Option<CodeNode> {
    let name = node.child_by_field_name("name")?;
    let name_str = text(name, src);
    let is_exported = has_visibility(node);
    let docstring = collect_doc_comments(node, src);
    let calls = collect_calls(node, src);

    // Build signature: everything from start to body (exclusive)
    let sig = build_signature(node, src);

    // Build stub
    let mut stub = String::new();
    if let Some(ref doc) = docstring {
        for line in doc.lines() {
            stub.push_str(line);
            stub.push('\n');
        }
    }
    stub.push_str(&sig);
    stub.push_str(" {\n");
    if !calls.is_empty() {
        stub.push_str("    // Calls: ");
        stub.push_str(&calls.join(", "));
        stub.push('\n');
    }
    stub.push_str("    /* ...implementation elided... */\n}");

    let beacons = collect_beacon_tags(node, src);
    Some(CodeNode {
        name: name_str,
        node_type: "function".to_string(),
        signature: sig,
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        is_exported,
        docstring,
        stub_content: stub,
        calls,
        beacons,
    })
}

fn extract_struct(node: Node, src: &[u8]) -> Option<CodeNode> {
    let name = node.child_by_field_name("name")?;
    let name_str = text(name, src);
    let is_exported = has_visibility(node);
    let docstring = collect_doc_comments(node, src);

    // Build stub with field names and types
    let full_text = text(node, src);
    let mut stub = String::new();
    if let Some(ref doc) = docstring {
        for line in doc.lines() {
            stub.push_str(line);
            stub.push('\n');
        }
    }
    stub.push_str(&full_text);

    let beacons = collect_beacon_tags(node, src);
    Some(CodeNode {
        name: name_str,
        node_type: "struct".to_string(),
        signature: build_type_signature(node, src, "struct"),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        is_exported,
        docstring,
        stub_content: stub,
        calls: Vec::new(),
        beacons,
    })
}

fn extract_enum(node: Node, src: &[u8]) -> Option<CodeNode> {
    let name = node.child_by_field_name("name")?;
    let name_str = text(name, src);
    let is_exported = has_visibility(node);
    let docstring = collect_doc_comments(node, src);
    let full_text = text(node, src);

    let mut stub = String::new();
    if let Some(ref doc) = docstring {
        for line in doc.lines() {
            stub.push_str(line);
            stub.push('\n');
        }
    }
    stub.push_str(&full_text);

    let beacons = collect_beacon_tags(node, src);
    Some(CodeNode {
        name: name_str,
        node_type: "enum".to_string(),
        signature: build_type_signature(node, src, "enum"),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        is_exported,
        docstring,
        stub_content: stub,
        calls: Vec::new(),
        beacons,
    })
}

fn extract_impl(node: Node, src: &[u8], out: &mut Vec<CodeNode>) {
    // Extract each method inside the impl block
    let impl_type = node.child_by_field_name("type")
        .map(|n| text(n, src))
        .unwrap_or_default();
    let trait_name = node.child_by_field_name("trait")
        .map(|n| text(n, src));

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_item" {
                if let Some(mut method) = extract_function(child, src) {
                    // Prefix method name with impl type for disambiguation
                    method.node_type = "method".to_string();
                    if let Some(ref t) = trait_name {
                        method.signature = format!(
                            "impl {} for {} > {}",
                            t, impl_type, method.signature
                        );
                    } else {
                        method.signature = format!(
                            "impl {} > {}",
                            impl_type, method.signature
                        );
                    }
                    out.push(method);
                }
            }
        }
    }
}

fn extract_trait(node: Node, src: &[u8]) -> Option<CodeNode> {
    let name = node.child_by_field_name("name")?;
    let name_str = text(name, src);
    let is_exported = has_visibility(node);
    let docstring = collect_doc_comments(node, src);

    // Collect method signatures from trait body
    let mut methods = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "function_item" || child.kind() == "function_signature_item" {
                let sig = build_signature(child, src);
                methods.push(format!("    {};", sig));
            }
        }
    }

    let mut stub = String::new();
    if let Some(ref doc) = docstring {
        for line in doc.lines() {
            stub.push_str(line);
            stub.push('\n');
        }
    }
    let sig = build_type_signature(node, src, "trait");
    stub.push_str(&sig);
    stub.push_str(" {\n");
    for m in &methods {
        stub.push_str(m);
        stub.push('\n');
    }
    stub.push('}');

    let beacons = collect_beacon_tags(node, src);
    Some(CodeNode {
        name: name_str,
        node_type: "trait".to_string(),
        signature: sig,
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        is_exported,
        docstring,
        stub_content: stub,
        calls: Vec::new(),
        beacons,
    })
}

fn extract_simple_item(node: Node, src: &[u8]) -> Option<CodeNode> {
    let name = node.child_by_field_name("name")?;
    let name_str = text(name, src);
    let node_type = match node.kind() {
        "const_item" => "const",
        "static_item" => "static",
        "type_item" => "type",
        _ => return None,
    };
    let is_exported = has_visibility(node);
    let docstring = collect_doc_comments(node, src);
    let full_text = text(node, src);

    let beacons = collect_beacon_tags(node, src);
    Some(CodeNode {
        name: name_str,
        node_type: node_type.to_string(),
        signature: full_text.clone(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        is_exported,
        docstring,
        stub_content: full_text,
        calls: Vec::new(),
        beacons,
    })
}

// ── Helpers ──

fn text(node: Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

fn has_visibility(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return true;
        }
    }
    false
}

/// Collect doc comments (///) immediately preceding a node.
fn collect_doc_comments(node: Node, src: &[u8]) -> Option<String> {
    let mut docs = Vec::new();
    let mut sibling = node.prev_sibling();
    while let Some(s) = sibling {
        if s.kind() == "line_comment" {
            let t = text(s, src);
            if t.starts_with("///") || t.starts_with("//!") {
                docs.push(t);
                sibling = s.prev_sibling();
                continue;
            }
        }
        if s.kind() == "attribute_item" {
            // Skip #[derive(...)] etc.
            sibling = s.prev_sibling();
            continue;
        }
        break;
    }
    if docs.is_empty() {
        None
    } else {
        docs.reverse();
        Some(docs.join("\n"))
    }
}

/// Collect `// @beacon: tag1, tag2` from regular comments above a node.
fn collect_beacon_tags(node: Node, src: &[u8]) -> Vec<String> {
    let mut comments = Vec::new();
    let mut sibling = node.prev_sibling();
    while let Some(s) = sibling {
        if s.kind() == "line_comment" {
            comments.push(text(s, src));
            sibling = s.prev_sibling();
            continue;
        }
        if s.kind() == "attribute_item" {
            sibling = s.prev_sibling();
            continue;
        }
        break;
    }
    if comments.is_empty() {
        return Vec::new();
    }
    comments.reverse();
    let joined = comments.join("\n");
    super::extract_beacon_tags(&joined)
}

/// Build function signature (everything before the body block).
fn build_signature(node: Node, src: &[u8]) -> String {
    let start = node.start_byte();
    let body = node.child_by_field_name("body");
    let end = body.map(|b| b.start_byte()).unwrap_or(node.end_byte());
    let sig_bytes = &src[start..end];
    let sig = String::from_utf8_lossy(sig_bytes).trim().to_string();
    // Remove trailing doc comments that may leak in
    sig.trim_end().to_string()
}

/// Build type signature for struct/enum/trait (keyword + name + generics).
fn build_type_signature(node: Node, src: &[u8], keyword: &str) -> String {
    let vis = if has_visibility(node) { "pub " } else { "" };
    let name = node.child_by_field_name("name")
        .map(|n| text(n, src))
        .unwrap_or_default();
    let type_params = node.child_by_field_name("type_parameters")
        .map(|n| text(n, src))
        .unwrap_or_default();
    format!("{}{} {}{}", vis, keyword, name, type_params)
}

/// Collect all function/method call names from a node's body.
fn collect_calls(node: Node, src: &[u8]) -> Vec<String> {
    let mut calls = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call_expression" {
            if let Some(func) = n.child_by_field_name("function") {
                let call_name = match func.kind() {
                    "identifier" => text(func, src),
                    "field_expression" => {
                        // method call: obj.method()
                        func.child_by_field_name("field")
                            .map(|f| text(f, src))
                            .unwrap_or_default()
                    }
                    "scoped_identifier" => {
                        // Path::call() — take the last segment
                        let t = text(func, src);
                        t.rsplit("::").next().unwrap_or(&t).to_string()
                    }
                    _ => String::new(),
                };
                if !call_name.is_empty() && !calls.contains(&call_name) {
                    calls.push(call_name);
                }
            }
        }
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) {
            stack.push(child);
        }
    }
    calls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::AstPruner;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
/// Adds two numbers
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let pruner = RustPruner;
        let nodes = pruner.extract_nodes(source);
        assert_eq!(nodes.len(), 1);
        let n = &nodes[0];
        assert_eq!(n.name, "add");
        assert_eq!(n.node_type, "function");
        assert!(n.is_exported);
        assert!(n.docstring.as_ref().unwrap().contains("Adds two numbers"));
        assert!(n.stub_content.contains("pub fn add"));
        assert!(n.stub_content.contains("implementation elided"));
    }

    #[test]
    fn test_extract_struct_and_impl() {
        let source = r#"
/// A point in 2D space
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// Calculate distance to origin
    pub fn distance(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}
"#;
        let pruner = RustPruner;
        let nodes = pruner.extract_nodes(source);
        assert!(nodes.len() >= 2, "Expected struct + method, got {}", nodes.len());

        let struct_node = nodes.iter().find(|n| n.node_type == "struct").unwrap();
        assert_eq!(struct_node.name, "Point");
        assert!(struct_node.is_exported);

        let method = nodes.iter().find(|n| n.node_type == "method").unwrap();
        assert_eq!(method.name, "distance");
        assert!(method.signature.contains("impl Point"));
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
fn process() {
    let data = fetch_data();
    let result = transform(data);
    result.save();
}
"#;
        let pruner = RustPruner;
        let nodes = pruner.extract_nodes(source);
        assert_eq!(nodes.len(), 1);
        let n = &nodes[0];
        assert!(n.calls.contains(&"fetch_data".to_string()));
        assert!(n.calls.contains(&"transform".to_string()));
        assert!(n.calls.contains(&"save".to_string()));
    }

    #[test]
    fn test_embedding_text() {
        let node = CodeNode {
            name: "foo".to_string(),
            node_type: "function".to_string(),
            signature: "pub fn foo(x: i32) -> bool".to_string(),
            start_line: 10,
            end_line: 20,
            is_exported: true,
            docstring: Some("/// Check if x is valid".to_string()),
            stub_content: String::new(),
            calls: vec!["validate".to_string(), "check".to_string()],
            beacons: Vec::new(),
        };
        let text = node.embedding_text("src/lib.rs");
        assert!(text.contains("Name: foo"));
        assert!(text.contains("Path: src/lib.rs"));
        assert!(text.contains("Calls: validate, check"));
    }
}
