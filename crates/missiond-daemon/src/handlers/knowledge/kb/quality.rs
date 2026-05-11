use serde_json::Value;

/// Content guard: reject verbose debug logs, stack traces, and narrative-style entries.
/// Returns Some(rejection_message) if content should be rejected, None if OK.
pub(super) fn check_content_quality(
    summary: &str,
    detail: &Option<Value>,
    category: Option<&str>,
) -> Option<String> {
    // Rule 1: summary too long — architecture:summary gets 800 chars, others 400
    let max_chars = match category {
        Some(c) if c == "architecture:summary" => 800,
        _ => 400,
    };
    if summary.chars().count() > max_chars {
        return Some(format!(
            "REJECTED: summary 过长（{}字）。summary 必须 ≤ {} 字，是结论性摘要。高密度技术细节（配置/命令/代码）请存入 detail 字段（JSON）。",
            summary.chars().count(),
            max_chars
        ));
    }

    // Rule 1b: empty or near-empty summary
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Some("REJECTED: summary 为空。".to_string());
    }
    if trimmed.chars().count() < 5 {
        return Some(format!(
            "REJECTED: summary 过短（{}字）。至少需要 5 个字符才能构成有意义的知识。",
            trimmed.chars().count()
        ));
    }

    // Rule 1c: test/probe entries
    let lower = summary.to_lowercase();
    let garbage_patterns = ["test write", "test kb write", "probe", "test entry"];
    for pattern in &garbage_patterns {
        if lower == *pattern || lower.starts_with(&format!("{} ", pattern)) {
            return Some(format!(
                "REJECTED: summary 疑似测试条目（'{}'）。测试数据不应写入知识库。",
                summary
            ));
        }
    }

    // Rule 1d: batch log entries (e.g., "realtime-extract 批次 batch-20260315-...")
    if lower.contains("batch-") && (lower.contains("处理完成") || lower.contains("批次")) {
        return Some(
            "REJECTED: summary 是批次处理日志，不是知识。操作日志不应存入 KB。".to_string(),
        );
    }

    // Rule 2: summary contains stack trace / log indicators
    let stack_patterns = [
        "at node_modules/",
        "Caused by:",
        "stack trace",
        "panic at",
        "RUST_BACKTRACE",
        "Error:",
        "    at ",
        "线程",
        "thread '",
    ];
    for pattern in &stack_patterns {
        if summary.contains(pattern) {
            return Some(format!(
                "REJECTED: summary 包含堆栈/日志片段（'{}'）。summary 应是泛化结论，不要包含原始报错。请提炼后重试。",
                pattern
            ));
        }
    }

    // Rule 3: narrative indicators — "先...然后...最后..." pattern in summary
    let narrative_words = [
        "先查看",
        "先检查",
        "然后尝试",
        "然后发现",
        "最后发现",
        "接着",
        "第一步",
        "第二步",
        "第三步",
        "首先我",
        "我尝试",
    ];
    let narrative_count = narrative_words
        .iter()
        .filter(|w| summary.contains(*w))
        .count();
    if narrative_count >= 2 {
        return Some(
            "REJECTED: summary 是叙事体（含「先...然后...」等流水账结构）。请改写为结论性陈述：\
             【现象关键字】→【根因】→【解决方案】。"
                .to_string(),
        );
    }

    // Rule 4: detail too large (> 2000 chars of serialized JSON = likely pasting raw logs)
    if let Some(d) = detail {
        let detail_str = serde_json::to_string(d).unwrap_or_default();
        if detail_str.len() > 2000 {
            return Some(format!(
                "REJECTED: detail 过长（{}字节）。detail 应是结构化三段式 {{trigger, conclusion, action}}，不要粘贴原始日志。请精简后重试。",
                detail_str.len()
            ));
        }
    }

    None
}
