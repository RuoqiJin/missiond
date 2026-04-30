use serde_json::{json, Value};

use crate::handlers::knowledge::file_artifacts::{
    attempt_artifact_write, ArtifactKind, WriterContext,
};
use crate::state::AppState;

use super::methodology::{extract_steps, GeneratedMeta};
use super::COMPILER_VERSION;

// ───────────────────────────────────────────────────────────────────────
// wave-14 :: workflow file-first writer args
//
// distill (dry_run + sonnet) and compile_methodology share one writer
// surface so the on-disk path layout stays consistent across both actions.
// Topic precedence is per-action (distill uses `name`; compile_methodology
// uses explicit `topic` > `name` > source stem). The DB row / YAML write
// runs first; the file write is best-effort and reports partial-status on
// failure (file-vs-db contract).
// ───────────────────────────────────────────────────────────────────────

pub(super) struct WorkflowFileArgs<'a> {
    pub(super) write_file: bool,
    pub(super) overwrite_file: bool,
    pub(super) topic: Option<&'a str>,
    pub(super) project: Option<&'a str>,
    pub(super) cwd: Option<&'a str>,
    pub(super) target_project: Option<&'a str>,
}

pub(super) fn extract_workflow_file_args(args: &Value) -> WorkflowFileArgs<'_> {
    WorkflowFileArgs {
        write_file: args
            .get("write_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        overwrite_file: args
            .get("overwrite_file")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        topic: args.get("topic").and_then(|v| v.as_str()),
        project: args.get("project").and_then(|v| v.as_str()),
        cwd: args.get("cwd").and_then(|v| v.as_str()),
        target_project: args.get("target_project").and_then(|v| v.as_str()),
    }
}

pub(super) async fn maybe_write_workflow_artifact(
    state: &AppState,
    args: &WorkflowFileArgs<'_>,
    payload: &mut Value,
    content: &str,
    fallback_topic: &str,
) {
    if !args.write_file {
        return;
    }
    let topic = args
        .topic
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_topic.trim());
    if topic.is_empty() {
        if let Some(map) = payload.as_object_mut() {
            map.insert("file_written".to_string(), json!(false));
            map.insert(
                "file_write_error".to_string(),
                json!("write_file=true requires a non-empty `topic` argument (or a workflow `name` fallback)"),
            );
            let already_partial = map
                .get("status")
                .and_then(|v| v.as_str())
                .map(|s| s == "partial")
                .unwrap_or(false);
            if !already_partial {
                map.insert("status".to_string(), json!("partial"));
            }
        }
        return;
    }
    let outcome = attempt_artifact_write(
        &state.project_registry,
        WriterContext {
            kind: ArtifactKind::Workflow,
            topic,
            project: args.project,
            cwd: args.cwd,
            target_project: args.target_project,
            overwrite: args.overwrite_file,
        },
        content,
    )
    .await;
    outcome.splice_into(payload);
}

/// wave38-01 :: methodology-compile match_rules builder.
///
/// compile_methodology has no Workflow DB row, so the enriched V3 workflow
/// artifact written under `.missiond/workflows/<topic>.lisp` derives every
/// stable ref from the deterministic `GeneratedMeta` (flow_id, source_hash,
/// compiler version, source path, generated_at) plus a fixed `source_kind`
/// = "methodology" / `compiler` = "deterministic-v0" pair so downstream
/// readers can distinguish a methodology projection from a distill row even
/// without consulting the DB. The shape mirrors what distill stores in
/// `Workflow.match_rules`, so verify-task-runner-batch / planners read both
/// projections through the same `:match_rules (…)` Lisp form.
pub(super) fn build_methodology_match_rules(meta: &GeneratedMeta) -> Value {
    json!({
        "source_kind": "methodology",
        "compiler": "deterministic-v0",
        "compiler_version": COMPILER_VERSION,
        "compiler_status": meta.compiler_status,
        "flow_id": meta.flow_id,
        "source_hash": meta.source_hash,
        "source_path": meta.source_path,
        "generated_at": meta.generated_at,
    })
}

pub(super) fn render_workflow_artifact_sexp(
    workflow_id: &str,
    source_plans: &[String],
    match_rules: &Value,
    status: &str,
    body: &str,
) -> String {
    let source_plans = render_lisp_vector(
        &source_plans
            .iter()
            .map(|plan| lisp_string(plan))
            .collect::<Vec<_>>(),
    );
    let steps = render_workflow_steps(body);
    format!(
        "(workflow\n  :workflow_id {workflow_id}\n  :source_plans {source_plans}\n  :match_rules {match_rules}\n  :steps {steps}\n  :status :{status}\n  :body {body}\n)\n",
        workflow_id = lisp_string(workflow_id),
        source_plans = source_plans,
        match_rules = json_to_lisp(match_rules),
        steps = steps,
        status = sanitize_lisp_symbol(status),
        body = body.trim(),
    )
}

fn render_workflow_steps(body: &str) -> String {
    let steps = extract_steps(body);
    if steps.is_empty() {
        return "[]".to_string();
    }
    render_lisp_vector(
        &steps
            .iter()
            .map(|step| {
                format!(
                    "(:id {} :body {})",
                    lisp_string(&step.id),
                    lisp_string(&step.body)
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn json_to_lisp(value: &Value) -> String {
    match value {
        Value::Null => "nil".to_string(),
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => lisp_string(s),
        Value::Array(items) => {
            render_lisp_vector(&items.iter().map(json_to_lisp).collect::<Vec<_>>())
        }
        Value::Object(map) => {
            let fields = map
                .iter()
                .map(|(key, value)| {
                    format!(":{} {}", sanitize_lisp_symbol(key), json_to_lisp(value))
                })
                .collect::<Vec<_>>();
            format!("({})", fields.join(" "))
        }
    }
}

fn render_lisp_vector(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", items.join(" "))
    }
}

fn lisp_string(s: &str) -> String {
    format!("{:?}", s)
}

fn sanitize_lisp_symbol(s: &str) -> String {
    let mut out = String::with_capacity(s.len().max(1));
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "value".to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}
