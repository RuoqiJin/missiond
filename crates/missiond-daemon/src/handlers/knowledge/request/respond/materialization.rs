use super::*;
use crate::handlers::knowledge::file_artifacts::{
    ArtifactCommitEnvelope, ArtifactCommitEnvelopeInput,
};
use crate::handlers::knowledge::plan::plan_contract_json_from_sexp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) struct BoardTaskMaterialization {
    pub(in crate::handlers::knowledge::request) board_task_id: String,
    pub(in crate::handlers::knowledge::request) board_task_created: bool,
}

pub(in crate::handlers::knowledge::request) fn board_task_materialization_to_json(
    m: &BoardTaskMaterialization,
) -> Value {
    json!({
        "board_task_id": m.board_task_id,
        "board_task_created": m.board_task_created,
        "source": "request-local review adapter",
    })
}

pub(in crate::handlers::knowledge::request) async fn ensure_request_board_task(
    state: &AppState,
    args: &Value,
    request_id: &str,
    paths: &RequestPaths,
) -> Result<BoardTaskMaterialization> {
    if let Some(id) = nonblank(args.get("board_task_id")) {
        let task = state
            .store
            .get_board_task(&id)
            .await
            .map_err(|e| anyhow::anyhow!("DB error: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("board_task `{}` not found", id))?;
        return Ok(BoardTaskMaterialization {
            board_task_id: task.id.to_string(),
            board_task_created: false,
        });
    }

    let project = nonblank(args.get("project")).or_else(|| nonblank(args.get("target_project")));
    let input = CreateBoardTaskInput {
        title: format!("Mission request {} plan", request_id),
        description: Some(format!(
            "Hidden anchor for request-local plan materialized from {}.",
            path_json(&paths.plan)
        )),
        priority: Some("medium".into()),
        category: Some("dev".into()),
        project,
        hidden: Some(true),
        context_intent: Some("code".into()),
        ..Default::default()
    };
    let task = state
        .store
        .create_board_task(&input)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    Ok(BoardTaskMaterialization {
        board_task_id: task.id.to_string(),
        board_task_created: true,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) struct PlanMaterialization {
    pub(in crate::handlers::knowledge::request) plan_ref: PlanRef,
    pub(in crate::handlers::knowledge::request) board_task_id: String,
    pub(in crate::handlers::knowledge::request) version: i32,
    pub(in crate::handlers::knowledge::request) sexp_hash: String,
    pub(in crate::handlers::knowledge::request) board_task_created: bool,
    pub(in crate::handlers::knowledge::request) artifact_projection: Option<PlanArtifactProjection>,
    pub(in crate::handlers::knowledge::request) artifact_projection_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::request) struct PlanArtifactProjection {
    pub(in crate::handlers::knowledge::request) path: PathBuf,
    pub(in crate::handlers::knowledge::request) sha256: String,
    pub(in crate::handlers::knowledge::request) bytes: u64,
    pub(in crate::handlers::knowledge::request) overwritten: bool,
}

pub(in crate::handlers::knowledge::request) fn plan_materialization_to_json(
    m: &PlanMaterialization,
) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("plan_id".into(), json!(m.plan_ref.id));
    obj.insert("board_task_id".into(), json!(m.board_task_id));
    obj.insert("version".into(), json!(m.version));
    obj.insert("sexp_hash".into(), json!(m.sexp_hash));
    obj.insert("board_task_created".into(), json!(m.board_task_created));
    obj.insert("source".into(), json!("request-local plan.lisp"));
    if let Some(p) = m.artifact_projection.as_ref() {
        obj.insert(
            "artifact_projection".into(),
            json!({
                "path": path_json(&p.path),
                "sha256": p.sha256,
                "bytes": p.bytes,
                "overwritten": p.overwritten,
            }),
        );
    }
    if let Some(e) = m.artifact_projection_error.as_ref() {
        obj.insert("artifact_projection_error".into(), json!(e));
    }
    Value::Object(obj)
}

pub(in crate::handlers::knowledge::request) fn enrich_materialized_plan_lisp(
    body: &str,
    plan_ref: &PlanRef,
    version: i32,
    board_task_id: &str,
) -> String {
    if body.contains(":plan_id") && body.contains(":version") && body.contains(":board_task_id") {
        return body.to_string();
    }

    let trimmed_len = body.trim_end().len();
    let trailing = &body[trimmed_len..];
    let mut core = body[..trimmed_len].to_string();
    if !core.ends_with(')') {
        return body.to_string();
    }

    core.pop();
    if !core.contains(":plan_id") {
        let _ = write!(core, "\n  :plan_id {}", lisp_string(&plan_ref.id));
    }
    if !core.contains(":version") {
        let _ = write!(core, "\n  :version {}", version);
    }
    if !core.contains(":board_task_id") {
        let _ = write!(core, "\n  :board_task_id {}", lisp_string(board_task_id));
    }
    core.push(')');
    core.push_str(trailing);
    core
}

pub(in crate::handlers::knowledge::request) fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

pub(in crate::handlers::knowledge::request) async fn materialize_request_plan(
    state: &AppState,
    args: &Value,
    request_id: &str,
    paths: &RequestPaths,
    plan_text: &str,
) -> Result<PlanMaterialization> {
    let mut anchor_args = args.clone();
    if nonblank(args.get("board_task_id")).is_none() {
        if let Some(board_task_id) = extract_lisp_keyword_string(plan_text, "board_task_id") {
            if let Some(obj) = anchor_args.as_object_mut() {
                obj.insert("board_task_id".into(), json!(board_task_id));
            }
        }
    }
    let anchor = ensure_request_board_task(state, &anchor_args, request_id, paths).await?;

    let existing = state
        .store
        .plan_list_by_task(&anchor.board_task_id)
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    let version = existing.iter().map(|p| p.version).max().unwrap_or(0) + 1;
    let source_directive_id = extract_lisp_keyword_string(plan_text, "directive_id")
        .and_then(|id| uuid::Uuid::parse_str(&id).ok());
    let sexp_hash = sha256_hex(plan_text);
    let plan_id = state
        .store
        .plan_insert(
            &anchor.board_task_id,
            source_directive_id,
            version,
            plan_text,
            &sexp_hash,
            PlanStatus::Draft,
            None,
            Some("mission_request request-local plan.lisp"),
        )
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;
    state
        .store
        .plan_update_contract_json(plan_id, &plan_contract_json_from_sexp(plan_text))
        .await
        .map_err(|e| anyhow::anyhow!("DB error: {}", e))?;

    let plan_ref = PlanRef {
        id: plan_id.to_string(),
    };
    let enriched_plan_text =
        enrich_materialized_plan_lisp(plan_text, &plan_ref, version, &anchor.board_task_id);
    let artifact_projection = if enriched_plan_text != plan_text {
        match ArtifactCommitEnvelope::commit_text(
            state,
            ArtifactCommitEnvelopeInput {
                operation_key: format!("mission_request:{request_id}:plan:{plan_id}:v{version}"),
                surface: "mission_request.materialization".to_string(),
                request_id: Some(request_id.to_string()),
                project_id: nonblank(args.get("project")),
                artifact_kind: "plan".to_string(),
                artifact_path: paths.plan.clone(),
                content: enriched_plan_text.clone(),
                overwrite: true,
                db_table: Some("plans".to_string()),
                db_row_id: Some(plan_id.to_string()),
                event_id: None,
                event_seq: None,
                payload: json!({
                    "commit_surface": "mission_request.materialization",
                    "board_task_id": anchor.board_task_id,
                    "plan_version": version,
                    "sexp_hash": sexp_hash,
                }),
            },
        )
        .await
        {
            Ok(write) => Some(PlanArtifactProjection {
                path: write.path,
                sha256: write.sha256,
                bytes: write.bytes,
                overwritten: write.overwritten,
            }),
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "request-local plan materialized into DB but failed to commit plan.lisp through ArtifactCommitEnvelope: {:#}",
                    e
                ));
            }
        }
    } else {
        None
    };

    Ok(PlanMaterialization {
        plan_ref,
        board_task_id: anchor.board_task_id,
        version,
        sexp_hash,
        board_task_created: anchor.board_task_created,
        artifact_projection,
        artifact_projection_error: None,
    })
}
