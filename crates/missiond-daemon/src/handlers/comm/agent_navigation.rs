//! MissionD SSOT agent navigation closure.
//!
//! This surface is deliberately narrow: compiled navigation artifacts are
//! read-only, and the only write is an append-only JSON sidecar for guide
//! usage feedback.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use chrono::Utc;
use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::helpers::missiond_project_root;
use crate::state::AppState;

const AGENT_SLICES_REL: &str = ".missiond/v3/runtime/compiled/compiled-agent-slices.json";
const PROJECT_NAVIGATION_REL: &str =
    ".missiond/v3/runtime/compiled/compiled-project-agent-navigation.json";
const REVIEW_SIDECAR_REL: &str = ".missiond/v3/runtime/agent-navigation-review.json";

pub(crate) async fn handle(_state: &AppState, _name: &str, args: Value) -> Result<ToolResult> {
    let action = string_arg(&args, "action").unwrap_or("catalog");
    match action {
        "catalog" => Ok(ToolResult::json_pretty(&catalog_action(&args))),
        "review" => Ok(ToolResult::json_pretty(&review_action())),
        "feedback" => match feedback_action(&args) {
            Ok(value) => Ok(ToolResult::json_pretty(&value)),
            Err(err) => Ok(ToolResult::structured_error(ToolError::new(
                error_codes::INVALID_PARAM,
                err.to_string(),
            ))),
        },
        "suggest_entries" => Ok(ToolResult::json_pretty(&suggest_entries_action(&args))),
        other => Ok(ToolResult::structured_error(
            ToolError::new(
                error_codes::UNKNOWN_ACTION,
                format!("unknown mission_agent_navigation action `{other}`"),
            )
            .with_suggestion("valid: catalog|review|feedback|suggest_entries"),
        )),
    }
}

fn catalog_action(args: &Value) -> Value {
    let project = project_arg(args);
    let agent = read_json_rel(AGENT_SLICES_REL);
    let project_nav = read_json_rel(PROJECT_NAVIGATION_REL);
    let review = read_review_sidecar().unwrap_or_else(|_| empty_review_sidecar());
    catalog_from_values(args, &project, agent, project_nav, review)
}

fn catalog_from_values(
    args: &Value,
    project: &str,
    agent: Result<Value>,
    project_nav: Result<Value>,
    review: Value,
) -> Value {
    let intent = string_arg(args, "intent")
        .or_else(|| string_arg(args, "query"))
        .unwrap_or_default();
    let surface = string_arg(args, "surface");
    let entry_id = string_arg(args, "entry_id").or_else(|| string_arg(args, "entryId"));
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 250) as usize;

    if project != "missiond" {
        return match project_nav {
            Ok(compiled) => {
                let projects = project_cards(&compiled, Some(project), limit);
                json!({
                    "schema": "missiond.agent-navigation.catalog.v1",
                    "ok": true,
                    "project": project,
                    "mode": "project-navigation",
                    "projects": projects,
                    "review": review_summary(&review),
                    "source": {
                        "projectAgentNavigation": PROJECT_NAVIGATION_REL,
                        "reviewSidecar": REVIEW_SIDECAR_REL
                    }
                })
            }
            Err(err) => unavailable_catalog(project, intent, err),
        };
    }

    match agent {
        Ok(compiled) => {
            let entries = compiled
                .pointer("/payload/entries")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let selected = select_entry(&entries, entry_id, surface, intent);
            let entries = entries.into_iter().take(limit).collect::<Vec<_>>();
            json!({
                "schema": "missiond.agent-navigation.catalog.v1",
                "ok": true,
                "project": project,
                "mode": "missiond-agent-entries",
                "selectedEntry": selected,
                "entries": entries,
                "review": review_summary(&review),
                "source": {
                    "agentSlices": AGENT_SLICES_REL,
                    "reviewSidecar": REVIEW_SIDECAR_REL,
                    "sourceHash": compiled.get("source_hash").cloned().unwrap_or(Value::Null)
                }
            })
        }
        Err(err) => unavailable_catalog(project, intent, err),
    }
}

fn review_action() -> Value {
    let review = read_review_sidecar().unwrap_or_else(|_| empty_review_sidecar());
    json!({
        "schema": "missiond.agent-navigation.review.v1",
        "ok": true,
        "sidecar": REVIEW_SIDECAR_REL,
        "summary": review_summary(&review),
        "events": review.pointer("/events").cloned().unwrap_or_else(|| json!([]))
    })
}

fn feedback_action(args: &Value) -> Result<Value> {
    let outcome = string_arg(args, "outcome").unwrap_or("used");
    if !matches!(
        outcome,
        "used" | "missed" | "wrong_entry" | "insufficient_context" | "suggested"
    ) {
        return Err(anyhow!(
            "unsupported outcome `{outcome}`; expected used|missed|wrong_entry|insufficient_context|suggested"
        ));
    }
    let project = project_arg(args).to_string();
    let mut sidecar = read_review_sidecar().unwrap_or_else(|_| empty_review_sidecar());
    let event = json!({
        "id": Uuid::new_v4().to_string(),
        "at": Utc::now().to_rfc3339(),
        "projectId": project,
        "entryId": string_arg(args, "entry_id").or_else(|| string_arg(args, "entryId")),
        "surface": string_arg(args, "surface"),
        "intent": string_arg(args, "intent").or_else(|| string_arg(args, "query")),
        "outcome": outcome,
        "rationale": string_arg(args, "rationale"),
        "agentId": string_arg(args, "agent_id").or_else(|| string_arg(args, "agentId")),
        "source": "mission_agent_navigation.feedback"
    });
    if !sidecar.pointer("/events").is_some_and(Value::is_array) {
        sidecar["events"] = json!([]);
    }
    sidecar
        .get_mut("events")
        .and_then(Value::as_array_mut)
        .expect("events is array")
        .push(event.clone());
    sidecar["updatedAt"] = json!(Utc::now().to_rfc3339());
    atomic_write_json(&review_sidecar_path(), &sidecar)?;
    Ok(json!({
        "schema": "missiond.agent-navigation.feedback.v1",
        "ok": true,
        "sidecar": REVIEW_SIDECAR_REL,
        "event": event,
        "summary": review_summary(&sidecar)
    }))
}

fn suggest_entries_action(args: &Value) -> Value {
    let project = project_arg(args);
    match read_json_rel(PROJECT_NAVIGATION_REL) {
        Ok(compiled) => json!({
            "schema": "missiond.agent-navigation.suggestions.v1",
            "ok": true,
            "project": project,
            "suggestions": project_cards(&compiled, (project != "missiond").then_some(project), 100)
                .into_iter()
                .map(|mut card| {
                    card["suggestionOnly"] = json!(true);
                    card
                })
                .collect::<Vec<_>>(),
            "rule": "MissionD reports suggestions only; it must not edit sibling repository SSOT files."
        }),
        Err(err) => json!({
            "schema": "missiond.agent-navigation.suggestions.v1",
            "ok": false,
            "diagnostic": {
                "code": "PROJECT_AGENT_NAVIGATION_UNAVAILABLE",
                "message": format!("cannot read {PROJECT_NAVIGATION_REL}: {err}"),
                "recovery": "Run node scripts/compile-v3-runtime.mjs --write."
            },
            "suggestions": []
        }),
    }
}

fn unavailable_catalog(project: &str, intent: &str, err: anyhow::Error) -> Value {
    json!({
        "schema": "missiond.agent-navigation.catalog.v1",
        "ok": false,
        "project": project,
        "intent": intent,
        "diagnostic": {
            "code": "AGENT_NAVIGATION_UNAVAILABLE",
            "message": format!("{err}"),
            "recovery": "Run node scripts/compile-v3-runtime.mjs --write."
        },
    })
}

fn project_cards(compiled: &Value, project: Option<&str>, limit: usize) -> Vec<Value> {
    compiled
        .pointer("/payload/projects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|card| {
            project
                .map(|id| card.get("projectId").and_then(Value::as_str) == Some(id))
                .unwrap_or(true)
        })
        .take(limit)
        .collect()
}

fn select_entry(
    entries: &[Value],
    entry_id: Option<&str>,
    surface: Option<&str>,
    intent: &str,
) -> Option<Value> {
    if let Some(entry_id) = entry_id {
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(entry_id))
        {
            return Some(entry.clone());
        }
    }
    if let Some(surface) = surface {
        if let Some(entry) = entries.iter().find(|entry| {
            entry
                .get("surfaces")
                .and_then(Value::as_array)
                .is_some_and(|surfaces| surfaces.iter().any(|item| item.as_str() == Some(surface)))
        }) {
            return Some(entry.clone());
        }
    }
    let normalized = intent.to_lowercase();
    entries
        .iter()
        .filter_map(|entry| {
            let score = score_entry(entry, &normalized);
            (score > 0).then_some((score, entry))
        })
        .max_by(|(left_score, left), (right_score, right)| {
            left_score.cmp(right_score).then_with(|| {
                right
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .cmp(left.get("id").and_then(Value::as_str).unwrap_or(""))
            })
        })
        .map(|(_, entry)| entry.clone())
}

fn score_entry(entry: &Value, normalized_intent: &str) -> i64 {
    let mut score = 0;
    if let Some(id) = entry.get("id").and_then(Value::as_str) {
        if normalized_intent.contains(&id.to_lowercase()) {
            score += 100;
        }
    }
    if let Some(label) = entry.get("label").and_then(Value::as_str) {
        for token in label.to_lowercase().split(|ch: char| !ch.is_alphanumeric()) {
            if token.len() >= 4 && normalized_intent.contains(token) {
                score += 10;
            }
        }
    }
    for key in ["intentKeywords", "surfaces"] {
        if let Some(values) = entry.get(key).and_then(Value::as_array) {
            for value in values.iter().filter_map(Value::as_str) {
                let normalized = value.to_lowercase();
                if !normalized.is_empty() && normalized_intent.contains(&normalized) {
                    score += 25 + normalized.len() as i64;
                }
            }
        }
    }
    score
}

fn review_summary(review: &Value) -> Value {
    let events = review
        .pointer("/events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut outcomes = serde_json::Map::new();
    for event in &events {
        if let Some(outcome) = event.get("outcome").and_then(Value::as_str) {
            let current = outcomes.get(outcome).and_then(Value::as_i64).unwrap_or(0);
            outcomes.insert(outcome.to_string(), json!(current + 1));
        }
    }
    json!({
        "eventCount": events.len(),
        "outcomes": outcomes,
        "updatedAt": review.get("updatedAt").cloned().unwrap_or(Value::Null)
    })
}

fn empty_review_sidecar() -> Value {
    json!({
        "schema": "missiond.agent-navigation-review.v1",
        "updatedAt": null,
        "events": []
    })
}

fn read_json_rel(rel: &str) -> Result<Value> {
    let text = fs::read_to_string(missiond_project_root().join(rel))?;
    Ok(serde_json::from_str(&text)?)
}

fn read_review_sidecar() -> Result<Value> {
    let path = review_sidecar_path();
    if !path.exists() {
        return Ok(empty_review_sidecar());
    }
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn review_sidecar_path() -> PathBuf {
    missiond_project_root().join(REVIEW_SIDECAR_REL)
}

fn atomic_write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn project_arg(args: &Value) -> &str {
    string_arg(args, "project")
        .or_else(|| string_arg(args, "project_id"))
        .or_else(|| string_arg(args, "projectId"))
        .unwrap_or("missiond")
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_navigation_catalog_selects_entry_by_intent() {
        let args = json!({"action": "catalog", "intent": "change plan execution"});
        let catalog = catalog_from_values(
            &args,
            "missiond",
            Ok(test_agent_slices()),
            Ok(test_project_navigation()),
            empty_review_sidecar(),
        );
        assert_eq!(
            catalog.pointer("/selectedEntry/id").and_then(Value::as_str),
            Some("modify-plan-execution")
        );
    }

    #[test]
    fn agent_navigation_catalog_prefers_autopilot_completion() {
        let args =
            json!({"action": "catalog", "intent": "我要修改 autopilot 的 BoardTask 完成判定"});
        let catalog = catalog_from_values(
            &args,
            "missiond",
            Ok(test_agent_slices()),
            Ok(test_project_navigation()),
            empty_review_sidecar(),
        );
        assert_eq!(
            catalog.pointer("/selectedEntry/id").and_then(Value::as_str),
            Some("modify-workstation-autopilot")
        );
    }

    #[test]
    fn agent_navigation_suggests_registered_project_without_write_scope() {
        let cards = project_cards(&test_project_navigation(), Some("jarvis"), 10);
        assert_eq!(
            cards
                .first()
                .and_then(|card| card.get("projectId"))
                .and_then(Value::as_str),
            Some("jarvis")
        );
        assert!(cards
            .first()
            .and_then(|card| card.get("writeScope"))
            .and_then(Value::as_array)
            .is_some_and(|scope| scope.is_empty()));
    }

    fn test_agent_slices() -> Value {
        json!({
            "payload": {
                "entries": [
                    {
                        "id": "modify-board-backend",
                        "label": "Modify Board backend",
                        "intentKeywords": ["board", "boardtask"],
                        "surfaces": ["mission_board"]
                    },
                    {
                        "id": "modify-plan-execution",
                        "label": "Modify plan execution",
                        "intentKeywords": ["plan execution"],
                        "surfaces": ["mission_plan"]
                    },
                    {
                        "id": "modify-workstation-autopilot",
                        "label": "Modify workstation autopilot",
                        "intentKeywords": ["autopilot", "boardtask 完成", "完成判定"],
                        "surfaces": ["autopilot-runtime"]
                    }
                ]
            }
        })
    }

    fn test_project_navigation() -> Value {
        json!({
            "payload": {
                "projects": [
                    {
                        "id": "project:jarvis",
                        "projectId": "jarvis",
                        "coverageState": "native-ssot-present",
                        "readFirst": ["/Users/jinchen/Projects/jarvis/.missiond/intent.lisp"],
                        "writeScope": []
                    }
                ]
            }
        })
    }
}
