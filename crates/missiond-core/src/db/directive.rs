//! DirectiveLayerStore shared helpers (backend-agnostic).
//!
//! Column lists and tiny helpers reused by the PostgreSQL backend so SELECT
//! statements stay in sync with `crates/missiond-core/migrations/20260420000000_directive_plan_workflow.sql`.
//! Heavy lifting lives in `db/pg/directive.rs` — this file is intentionally thin.

/// Directive table column list.
#[allow(dead_code)]
pub(crate) const DIRECTIVE_COLS: &str =
    "id, utterance_text, sexp_text, version, status, compiler_model, references_json, created_at, approved_at";

/// Plan table column list.
#[allow(dead_code)]
pub(crate) const PLAN_COLS: &str =
    "id, board_task_id, source_directive_id, version, sexp_text, sexp_hash, status, compiler_model, compiled_from, contract_json, created_at, approved_at, finished_at";

/// Workflow table column list.
///
/// `avg_cost_usd` is fetched via an explicit `::float8` cast at the query site
/// (NUMERIC → f64) so we do not need a `rust_decimal` / `bigdecimal` sqlx feature.
#[allow(dead_code)]
pub(crate) const WORKFLOW_COLS_WITH_CAST: &str =
    "id, name, sexp_text, match_rules, learned_from, executions, success_count, avg_cost_usd::float8 AS avg_cost_usd, last_used_at, created_at";
