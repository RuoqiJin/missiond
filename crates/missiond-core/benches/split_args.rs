//! Benchmark for split_args — quote-aware CSV splitting.
//!
//! Run: cargo bench --bench split_args -p missiond-core

use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Copy of the production split_args for benchmarking.
/// The AI's task is to optimize the ORIGINAL in custom.rs —
/// this copy serves as the baseline reference.
fn split_args(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escape_next = false;

    for c in input.chars() {
        if escape_next {
            current.push(c);
            escape_next = false;
            continue;
        }
        match c {
            '\\' => {
                current.push(c);
                escape_next = true;
            }
            '"' => {
                current.push(c);
                in_quotes = !in_quotes;
            }
            ',' if !in_quotes => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    parts.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }
    parts
}

fn bench_split_args(c: &mut Criterion) {
    // Simple case
    c.bench_function("split_args_simple", |b| {
        b.iter(|| split_args(black_box("arg1, arg2, arg3")))
    });

    // Quoted args
    c.bench_function("split_args_quoted", |b| {
        b.iter(|| split_args(black_box(r#"key: "hello, world", other: "foo""#)))
    });

    // Many args
    let many_args = (0..50)
        .map(|i| format!("arg{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    c.bench_function("split_args_50", |b| {
        b.iter(|| split_args(black_box(&many_args)))
    });

    // Complex with escapes
    c.bench_function("split_args_complex", |b| {
        b.iter(|| {
            split_args(black_box(
                r#"path: "/tmp/foo bar/baz", cmd: "echo \"hello\"", flag: true, count: 42"#,
            ))
        })
    });
}

criterion_group!(benches, bench_split_args);
criterion_main!(benches);
