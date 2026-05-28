use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;

pub(crate) struct CapsuleIsolation<'a> {
    pub user_id: Option<&'a str>,
    pub tenant_id: Option<&'a str>,
    pub application_id: Option<&'a str>,
    pub channel: &'a str,
}

pub(crate) fn generate_lisp_capsule(
    isolation: &CapsuleIsolation<'_>,
    context_gather_payload: &Value,
    topic_id: Option<&str>,
    topic_label: Option<&str>,
    task_id: Option<&str>,
    intent_ref: Option<&str>,
    plan_ref: Option<&str>,
) -> (String, String) {
    let mut buf = String::with_capacity(4096);

    writeln!(buf, "(context-capsule").ok();
    writeln!(buf, "  :schema \"missiond.context-capsule.v1\"").ok();

    writeln!(buf, "  (L0-isolation").ok();
    write_opt_field(&mut buf, "user-id", isolation.user_id);
    write_opt_field(&mut buf, "tenant-id", isolation.tenant_id);
    write_opt_field(&mut buf, "application-id", isolation.application_id);
    writeln!(buf, "    :channel \"{}\"", escape_lisp(isolation.channel)).ok();
    writeln!(buf, "  )").ok();

    if let Some(ssot) = context_gather_payload
        .get("sources")
        .and_then(|s| s.get("ssot"))
    {
        writeln!(buf, "  (L1-ssot-snapshot").ok();
        write_json_summary(&mut buf, "ssot", ssot);
        writeln!(buf, "  )").ok();
    }

    if let Some(kb) = context_gather_payload
        .get("sources")
        .and_then(|s| s.get("kb"))
    {
        writeln!(buf, "  (L2-active-kb").ok();
        write_json_summary(&mut buf, "kb", kb);
        writeln!(buf, "  )").ok();
    }

    if let Some(registry) = context_gather_payload
        .get("sources")
        .and_then(|s| s.get("project_registry"))
    {
        writeln!(buf, "  (L3-project-registry").ok();
        write_json_summary(&mut buf, "projects", registry);
        writeln!(buf, "  )").ok();
    }

    if let Some(skill) = context_gather_payload
        .get("sources")
        .and_then(|s| s.get("skill_context"))
    {
        writeln!(buf, "  (L4-skill-evidence").ok();
        write_json_summary(&mut buf, "skills", skill);
        writeln!(buf, "  )").ok();
    }

    if let Some(infra) = context_gather_payload
        .get("sources")
        .and_then(|s| s.get("infra"))
    {
        writeln!(buf, "  (L5-infra-facts").ok();
        write_json_summary(&mut buf, "infra", infra);
        writeln!(buf, "  )").ok();
    }

    if let Some(convs) = context_gather_payload
        .get("sources")
        .and_then(|s| s.get("conversation_logs"))
    {
        writeln!(buf, "  (L6-related-history").ok();
        write_json_summary(&mut buf, "conversations", convs);
        writeln!(buf, "  )").ok();
    }

    writeln!(buf, "  (L7-topic-context").ok();
    write_opt_field(&mut buf, "topic-id", topic_id);
    write_opt_field(&mut buf, "topic-label", topic_label);
    write_opt_field(&mut buf, "task-id", task_id);
    write_opt_field(&mut buf, "intent-ref", intent_ref);
    write_opt_field(&mut buf, "plan-ref", plan_ref);
    write_payload_string_field(&mut buf, "query", context_gather_payload.get("query"));
    write_json_summary(
        &mut buf,
        "unknowns",
        context_gather_payload
            .get("unknowns")
            .unwrap_or(&Value::Null),
    );
    write_json_summary(
        &mut buf,
        "evidence-refs",
        context_gather_payload
            .get("evidence_refs")
            .unwrap_or(&Value::Null),
    );
    write_json_summary(
        &mut buf,
        "unresolved",
        context_gather_payload
            .get("unresolved")
            .unwrap_or(&Value::Null),
    );
    writeln!(buf, "  )").ok();

    writeln!(buf, ")").ok();

    let hash = format!("{:x}", Sha256::digest(buf.as_bytes()));
    (buf, hash)
}

fn write_opt_field(buf: &mut String, key: &str, val: Option<&str>) {
    match val {
        Some(v) => writeln!(buf, "    :{key} \"{}\"", escape_lisp(v)).ok(),
        None => writeln!(buf, "    :{key} nil").ok(),
    };
}

fn write_json_summary(buf: &mut String, label: &str, val: &Value) {
    match val {
        Value::Array(arr) => {
            writeln!(buf, "    :{label}-count {}", arr.len()).ok();
        }
        Value::Object(map) => {
            if let Some(count) = map.get("count").or(map.get("returned")) {
                writeln!(buf, "    :{label}-count {count}").ok();
            } else {
                writeln!(buf, "    :{label}-keys {}", map.len()).ok();
            }
        }
        _ => {
            writeln!(buf, "    :{label} :present").ok();
        }
    }
    if !val.is_null() {
        let compact = compact_json(val, 1600);
        if !compact.is_empty() {
            writeln!(buf, "    :{label}-json \"{}\"", escape_lisp(&compact)).ok();
        }
    }
}

fn write_payload_string_field(buf: &mut String, key: &str, val: Option<&Value>) {
    if let Some(text) = val.and_then(Value::as_str) {
        writeln!(buf, "    :{key} \"{}\"", escape_lisp(text)).ok();
    }
}

fn compact_json(val: &Value, max_chars: usize) -> String {
    let raw = serde_json::to_string(val).unwrap_or_default();
    if raw.chars().count() <= max_chars {
        return raw;
    }
    let mut out = raw
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

fn escape_lisp(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
