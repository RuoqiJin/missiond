use super::*;

// ───────────────────────────────────────────────────────────────────────
// compile_methodology — dry-run preview vs deterministic compiler v0
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompileMode {
    DryRun,
    Deterministic,
}

pub(super) fn parse_compile_mode(raw: Option<&str>) -> Result<CompileMode, String> {
    match raw {
        None | Some("") | Some("dry_run") => Ok(CompileMode::DryRun),
        Some("deterministic") => Ok(CompileMode::Deterministic),
        Some(other) => Err(format!(
            "compile_mode must be one of [\"dry_run\", \"deterministic\"]; got `{}`",
            other
        )),
    }
}

pub(super) async fn action_compile_methodology(
    state: &AppState,
    args: &Value,
) -> Result<ToolResult> {
    let mode = match parse_compile_mode(args.get("compile_mode").and_then(|v| v.as_str())) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                msg,
            )));
        }
    };

    let project_root = match super::project_root::resolve_project_root_from_args(state, args).await
    {
        Ok(p) => p,
        Err(reason) => {
            return Ok(ToolResult::structured_error(
                ToolError::new(error_codes::INVALID_PARAM, reason).with_suggestion(
                    "supply `project` (registered id) or absolute `cwd`; \
                     compile_methodology refuses process-cwd fallback so the generated YAML \
                     always lands inside the registered project root.",
                ),
            ));
        }
    };
    let workflows_dir = project_root.join(WORKFLOWS_DIR);

    let path = match resolve_methodology_path(
        &project_root,
        args.get("name").and_then(|v| v.as_str()),
        args.get("workflow_path").and_then(|v| v.as_str()),
    ) {
        Ok(p) => p,
        Err(msg) => {
            return Ok(ToolResult::structured_error(ToolError::new(
                error_codes::MISSING_PARAM,
                msg,
            )));
        }
    };

    if !path.exists() {
        return Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::NOT_FOUND,
                format!("methodology lisp not found: {}", path.display()),
            )
            .with_suggestion(format!(
                "place it under {} and retry",
                workflows_dir.display()
            )),
        ));
    }

    let content =
        std::fs::read_to_string(&path).map_err(|e| anyhow!("read {}: {}", path.display(), e))?;

    match mode {
        CompileMode::DryRun => action_compile_dry_run(&path, &content),
        CompileMode::Deterministic => {
            action_compile_deterministic(state, &project_root, &path, &content, args).await
        }
    }
}

pub(super) fn action_compile_dry_run(path: &Path, content: &str) -> Result<ToolResult> {
    let line_count = content.lines().count();
    // Surface both the cheap line-counter (back-compat with earlier
    // dry-run consumers that scraped `phase_form_count` / `step_form_count`)
    // and the v0 semantic lifter's richer breakdown so callers can preview
    // exactly what `compile_mode="deterministic"` will emit.
    let phases = count_top_form(content, "phase");
    let steps = count_top_form(content, "step");
    let lifted = extract_methodology_lifted(content);
    Ok(ToolResult::json_pretty(&json!({
        "status": "dry_run",
        "compile_mode": "dry_run",
        "actor_pending": "intent-layer :: workflow compiler (Lisp → executable YAML)",
        "flow_ref": "F-methodology-to-executable-compile",
        "source_path": path.display().to_string(),
        "lines": line_count,
        "phase_form_count": phases,
        "step_form_count": steps,
        "lifted_form_count": lifted.total_count(),
        "lifted_form_breakdown": json!({
            "phases": lifted.phases.len(),
            "principles": lifted.principles.len(),
            "anti_patterns": lifted.anti_patterns.len(),
            "gates": lifted.gates.len(),
            "artifacts": lifted.artifacts.len(),
            "authorities": lifted.authorities.len(),
        }),
        "next_step": "pass compile_mode=\"deterministic\" to emit an executable YAML preview; persist=true writes it to .missiond/generated/flows/<flow_id>.yaml",
    })))
}

pub(super) async fn action_compile_deterministic(
    state: &AppState,
    project_root: &Path,
    path: &Path,
    content: &str,
    args: &Value,
) -> Result<ToolResult> {
    if let Err(msg) = validate_methodology_source(content) {
        return Ok(ToolResult::structured_error(
            ToolError::new(error_codes::INVALID_PARAM, msg)
                .with_suggestion("repair the methodology lisp and retry"),
        ));
    }

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("methodology")
        .to_string();
    let output_flow_id = args
        .get("output_flow_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let flow_id = derive_flow_id(&stem, output_flow_id);
    let display_name = format!("methodology compile v0 — {}", stem);

    let located_steps = extract_steps_with_lines(content);
    let lifted = extract_methodology_lifted(content);
    let review_required = located_steps.is_empty();
    let hash = source_hash(content);
    let generated_at = chrono::Utc::now().to_rfc3339();
    let source_display = source_path_for_yaml(project_root, path);

    let meta = GeneratedMeta {
        flow_id: flow_id.clone(),
        name: display_name,
        source_path: source_display.clone(),
        source_hash: hash.clone(),
        generated_at: generated_at.clone(),
        compiler_status: COMPILER_STATUS_PREVIEW.to_string(),
    };
    let yaml = build_generated_yaml(&meta, &located_steps, &lifted, review_required)
        .map_err(|e| anyhow!("serialize yaml: {}", e))?;

    let persist = args
        .get("persist")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let overwrite = args
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut payload = json!({
        "status": "compiled_preview",
        "compile_mode": "deterministic",
        "compiler_version": COMPILER_VERSION,
        "compiler_status": COMPILER_STATUS_PREVIEW,
        "flow_ref": "F-methodology-to-executable-compile :: s2/s3/s5",
        "flow_id": flow_id,
        "source_path": source_display,
        "source_hash": hash,
        "generated_at": generated_at,
        "step_count": located_steps.len(),
        "review_required": review_required,
        "lifted_form_count": lifted.total_count(),
        "lifted_form_breakdown": json!({
            "phases": lifted.phases.len(),
            "principles": lifted.principles.len(),
            "anti_patterns": lifted.anti_patterns.len(),
            "gates": lifted.gates.len(),
            "artifacts": lifted.artifacts.len(),
            "authorities": lifted.authorities.len(),
        }),
        "params_echo": args.get("params").cloned().unwrap_or(Value::Null),
        "future_compiler_actor": "intent-layer LLM/forge compiler — semantic execution of phase/anti-pattern/gate forms deferred; v0 lifts them into methodology_metadata only",
        "yaml_preview": yaml,
    });

    if !persist {
        payload["persisted"] = json!(false);
        payload["next_step"] = json!(
            "persist=true to write to .missiond/generated/flows/<flow_id>.yaml; \
             then run_methodology(flow_id=<flow_id>, dry_run=true) to inspect, dry_run=false to dispatch"
        );
        return Ok(ToolResult::json_pretty(&payload));
    }

    let yaml_path = generated_yaml_path(project_root, &meta.flow_id);
    if yaml_path.exists() && !overwrite {
        return Ok(ToolResult::structured_error(ToolError::new(
            error_codes::INVALID_PARAM,
            format!(
                "generated YAML already exists at {}; pass overwrite=true to replace",
                yaml_path.display()
            ),
        )));
    }
    atomic_write(&yaml_path, &yaml).map_err(|e| anyhow!("write {}: {}", yaml_path.display(), e))?;

    payload["persisted"] = json!(true);
    payload["flow_path"] = json!(yaml_path.display().to_string());
    payload["next_step"] = json!(
        "run_methodology(flow_id=<flow_id>, dry_run=true) to verify; dry_run=false to dispatch into mission_flow_run"
    );

    // wave-14/38 :: file-first SSOT mirror. compile_methodology already reads
    // the methodology lisp from `.missiond/workflows/<name>.lisp`, so the
    // file-first writer is only meaningful when the caller wants to
    // canonicalise / snapshot the source under a different topic, OR when
    // the caller passes overwrite_file=true to "re-emit" the same file.
    // Topic precedence: explicit `topic` arg > `name` arg > source stem.
    //
    // wave38-01 :: project the methodology compile as the same enriched V3
    // workflow artifact shape distill writes (render_workflow_artifact_sexp).
    // The methodology branch never produces a Workflow DB row, so :workflow_id
    // is stamped with the generated `flow_id` (deterministic, derived from
    // stem + output_flow_id) instead of a UUID; :source_plans stays empty;
    // :match_rules carries source_kind/compiler/compiler_version/source_hash/
    // flow_id/source_path/generated_at so reviewers can correlate the .lisp
    // artifact with the generated YAML; :steps re-runs the same step
    // extractor distill uses; :status is compiled (or compiled_review_required
    // when the methodology has no executable steps); :body is the methodology
    // Lisp body verbatim. Reviewers therefore see the V3 contract artifact,
    // not a raw source mirror. No DB migration is introduced.
    let file_args = extract_workflow_file_args(args);
    let fallback_topic = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| stem.clone());
    let topic_for_gate = file_args
        .topic
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_topic.clone());
    let methodology_status = if review_required {
        "compiled_review_required"
    } else {
        "compiled"
    };
    let methodology_match_rules = build_methodology_match_rules(&meta);
    let methodology_artifact_sexp = render_workflow_artifact_sexp(
        &meta.flow_id,
        &[],
        &methodology_match_rules,
        methodology_status,
        content,
    );
    maybe_write_workflow_artifact(
        state,
        &file_args,
        &mut payload,
        &methodology_artifact_sexp,
        &fallback_topic,
    )
    .await;

    // wave-14 :: review-gate auto-create. compile_methodology has no
    // workflow_id (the methodology source predates any distilled row), so
    // the artifact_id used in the deterministic question id is the
    // generated `flow_id`. The hook only fires when both
    // `review_gate_policy=emit_question` AND the file-first mirror was
    // requested AND landed (`file_written=true`); a YAML-only persist run
    // intentionally stays quiet because the workflow scope is not yet
    // canonicalised in `.missiond/workflows/<topic>.lisp`.
    let policy = parse_review_gate_policy(args);
    let policy_explicit = review_gate_policy_was_explicit(args);
    let legacy = parse_compile_review_gate(args);
    apply_compile_review_gates(
        &mut payload,
        &state.bus,
        policy,
        policy_explicit,
        &legacy,
        "workflow",
        &meta.flow_id,
        1,
        Some(&topic_for_gate),
    )
    .await;

    Ok(ToolResult::json_pretty(&payload))
}

pub(super) fn count_top_form(content: &str, name: &str) -> usize {
    let pat = format!("({}", name);
    content
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with(&pat)
                && t.as_bytes()
                    .get(pat.len())
                    .map(|b| b.is_ascii_whitespace() || *b == b')')
                    .unwrap_or(false)
        })
        .count()
}
