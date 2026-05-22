use super::*;
use crate::handlers::knowledge::workstation_dispatch;

// ───────────────────────────────────────────────────────────────────────
// plan-runner auto-selection v1
//
// When `mission_plan(action=execute)` is called without `target` (or other
// dispatch knobs), execution reads hints from plan.contract_json. New rows
// receive that projection at compile/materialization time; old empty rows are
// reprojected by missiond-lispc before dispatch. Explicit args still win.
//
// Lisp authority:
//   intent-flow.lisp        :: F-intent-alignment-plan-execution-loop ::
//                                s6 execution-runner
//   intent-flow.lisp        :: F-workstation-dispatch-policy
//   intent-intent-layer.lisp :: section unified-entry-pipeline ::
//                                role plan-runner
//   intent-worker.lisp      :: claudecode-workstation-orchestration
//   intent-tools.lisp       :: mission_plan :: :dispatch-strategy-consumer
// ───────────────────────────────────────────────────────────────────────

pub(crate) const AGENT_TEAM_OBJECTIVE_HINT: &str = "使用 agent-team提高效率";

#[derive(Debug, Default, Clone)]
pub(crate) struct ParsedPlanHints {
    pub(super) target: Option<String>,
    pub(super) flow_id: Option<String>,
    pub(super) dispatch_strategy: Option<String>,
    pub(super) parallelism: Option<String>,
    pub(super) target_project: Option<String>,
    pub(super) requested_cwd: Option<String>,
    pub(super) objective: Option<String>,
    pub(super) summary: Option<String>,
    /// wave-15 / task 05 — workstation-dispatch hint contract. Captured
    /// here so a single PLAN.lisp scan extracts every recognised field;
    /// the workstation_dispatch module reads them via `to_workstation_*`.
    pub(super) scope: Option<String>,
    pub(super) commit_policy: Option<String>,
    pub(super) owned_files_raw: Option<String>,
    pub(super) forbidden_files_raw: Option<String>,
    pub(super) acceptance_commands_raw: Option<String>,
    /// `:workstation-dispatch true` opts into workstation_dispatch v0.
    /// Stored as the parsed bareword so we keep the conservative
    /// "no Lisp interpretation" stance.
    pub(super) workstation_dispatch_flag: Option<String>,
}

impl ParsedPlanHints {
    pub(super) fn to_summary_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        let mut put = |k: &str, v: &Option<String>| {
            if let Some(s) = v {
                map.insert(k.to_string(), Value::String(s.clone()));
            }
        };
        put("target", &self.target);
        put("flow_id", &self.flow_id);
        put("dispatch_strategy", &self.dispatch_strategy);
        put("parallelism", &self.parallelism);
        put("target_project", &self.target_project);
        put("requested_cwd", &self.requested_cwd);
        put("objective", &self.objective);
        put("summary", &self.summary);
        put("scope", &self.scope);
        put("commit_policy", &self.commit_policy);
        put("owned_files", &self.owned_files_raw);
        put("forbidden_files", &self.forbidden_files_raw);
        put("acceptance_commands", &self.acceptance_commands_raw);
        put("workstation_dispatch", &self.workstation_dispatch_flag);
        Value::Object(map)
    }

    /// True iff the PLAN.lisp surfaced `:workstation-dispatch true` (or any
    /// bareword that lowercases to `true`/`yes`/`on`). False otherwise —
    /// `:workstation-dispatch false` and absence both produce False.
    pub(super) fn workstation_dispatch_opt_in(&self) -> bool {
        match self.workstation_dispatch_flag.as_deref() {
            Some(raw) => matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            ),
            None => false,
        }
    }

    /// Project the parsed PLAN.lisp scalars into the workstation-dispatch
    /// hint struct. Lists (`owned-files`, `forbidden-files`,
    /// `acceptance-commands`) round-trip through whitespace splitting on
    /// the captured raw value because the conservative scanner records
    /// the whole bracket span as one string.
    pub(super) fn to_workstation_hints(&self) -> workstation_dispatch::WorkstationDispatchHints {
        workstation_dispatch::WorkstationDispatchHints {
            objective: self.objective.clone().or_else(|| self.summary.clone()),
            scope: self.scope.clone(),
            owned_files: split_lisp_string_list(self.owned_files_raw.as_deref()),
            forbidden_files: split_lisp_string_list(self.forbidden_files_raw.as_deref()),
            acceptance_commands: split_lisp_string_list(self.acceptance_commands_raw.as_deref()),
            commit_policy: self.commit_policy.clone(),
            target_project: self.target_project.clone(),
            requested_cwd: self.requested_cwd.clone(),
            dispatch_strategy: self.dispatch_strategy.clone(),
        }
    }
}

/// Split a captured PLAN.lisp list value (`["a" "b"]` / `(a b)` / bareword
/// run) into a vector of strings. Quoted strings have their quotes
/// stripped; bare words pass through. Whitespace and commas separate
/// elements. Conservative on purpose: anything weird produces an empty
/// slice rather than a partial parse.
pub(crate) fn split_lisp_string_list(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else { return Vec::new() };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .or_else(|| trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')))
        .unwrap_or(trimmed);
    let mut out: Vec<String> = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        while i < n && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        if i >= n {
            break;
        }
        if chars[i] == '"' {
            i += 1;
            let start = i;
            let mut esc = false;
            while i < n {
                let c = chars[i];
                if esc {
                    esc = false;
                    i += 1;
                    continue;
                }
                if c == '\\' {
                    esc = true;
                    i += 1;
                    continue;
                }
                if c == '"' {
                    break;
                }
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            if !s.trim().is_empty() {
                out.push(s);
            }
            if i < n {
                i += 1;
            }
        } else {
            let start = i;
            while i < n
                && !chars[i].is_whitespace()
                && chars[i] != ','
                && chars[i] != '"'
                && chars[i] != '('
                && chars[i] != ')'
                && chars[i] != '['
                && chars[i] != ']'
            {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            if !s.trim().is_empty() {
                out.push(s);
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedExec {
    pub(super) target: &'static str,
    pub(super) target_source: &'static str,
    pub(super) dispatch_strategy: &'static str,
    pub(super) dispatch_strategy_source: &'static str,
    pub(super) plan_hint_summary: Value,
}

pub(crate) fn parse_plan_hints_for_plan(plan: &Plan) -> ParsedPlanHints {
    parse_plan_hints_from_contract_json(&plan.contract_json).unwrap_or_default()
}

pub(crate) fn plan_contract_json_from_sexp(sexp: &str) -> Value {
    json!({
        "schema_version": "missiond.plan-contract.v1",
        "projection_engine": "rust-compat",
        "payload": {
            "head": if sexp.trim_start().starts_with("(plan-draft") { "plan-draft" } else { "plan" },
            "hints": parse_plan_hints(sexp).to_summary_json(),
            "nodes": [],
        },
    })
}

pub(crate) fn parse_plan_hints_from_contract_json(contract: &Value) -> Option<ParsedPlanHints> {
    let payload = contract.get("payload").unwrap_or(contract);
    let hints = payload.get("hints")?.as_object()?;
    if hints.is_empty() {
        return None;
    }
    let mut out = ParsedPlanHints::default();
    let fill = |slot: &mut Option<String>, key: &str| {
        if let Some(value) = hints.get(key).and_then(plan_contract_value_to_hint_string) {
            if !value.trim().is_empty() {
                *slot = Some(value);
            }
        }
    };
    fill(&mut out.target, "target");
    fill(&mut out.flow_id, "flow_id");
    fill(&mut out.dispatch_strategy, "dispatch_strategy");
    fill(&mut out.parallelism, "parallelism");
    fill(&mut out.target_project, "target_project");
    fill(&mut out.requested_cwd, "requested_cwd");
    fill(&mut out.objective, "objective");
    fill(&mut out.summary, "summary");
    fill(&mut out.scope, "scope");
    fill(&mut out.commit_policy, "commit_policy");
    fill(&mut out.owned_files_raw, "owned_files");
    fill(&mut out.forbidden_files_raw, "forbidden_files");
    fill(&mut out.acceptance_commands_raw, "acceptance_commands");
    fill(&mut out.workstation_dispatch_flag, "workstation_dispatch");
    Some(out)
}

fn plan_contract_value_to_hint_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(v) => Some(v.to_string()),
        Value::Number(v) => Some(v.to_string()),
        Value::Array(values) => {
            let parts: Vec<String> = values
                .iter()
                .filter_map(plan_contract_value_to_hint_string)
                .collect();
            Some(format!("[{}]", parts.join(" ")))
        }
        _ => None,
    }
}

/// Parse a PLAN.lisp s-expression for known runner hints. This is NOT a full
/// Lisp interpreter; it scans `:keyword value` pairs at any depth and keeps
/// the first occurrence per keyword. Conservative on purpose: anything that
/// doesn't look like a simple keyword/value pair is silently skipped.
pub(crate) fn parse_plan_hints(sexp: &str) -> ParsedPlanHints {
    let mut h = ParsedPlanHints::default();

    fn store_first(slot: &mut Option<String>, value: &str) {
        if slot.is_none() {
            let v = value.trim();
            if !v.is_empty() {
                *slot = Some(v.to_string());
            }
        }
    }

    for (raw_key, value) in scan_keyword_pairs(sexp) {
        let key = raw_key.to_ascii_lowercase();
        match key.as_str() {
            "target" | "target-tool" | "tool" => store_first(&mut h.target, &value),
            "flow-id" | "flow_id" => store_first(&mut h.flow_id, &value),
            "dispatch-strategy" | "dispatch_strategy" => {
                store_first(&mut h.dispatch_strategy, &value)
            }
            "parallelism" => store_first(&mut h.parallelism, &value),
            "target-project" | "target_project" | "project" => {
                store_first(&mut h.target_project, &value)
            }
            "requested-cwd" | "requested_cwd" | "cwd" => store_first(&mut h.requested_cwd, &value),
            "objective" => store_first(&mut h.objective, &value),
            "summary" => store_first(&mut h.summary, &value),
            "scope" => store_first(&mut h.scope, &value),
            "commit-policy" | "commit_policy" => store_first(&mut h.commit_policy, &value),
            "owned-files" | "owned_files" => store_first(&mut h.owned_files_raw, &value),
            "forbidden-files" | "forbidden_files" => {
                store_first(&mut h.forbidden_files_raw, &value)
            }
            "acceptance-commands" | "acceptance_commands" => {
                store_first(&mut h.acceptance_commands_raw, &value)
            }
            "workstation-dispatch" | "workstation_dispatch" => {
                store_first(&mut h.workstation_dispatch_flag, &value)
            }
            _ => {}
        }
    }
    h
}

/// Scan a string for `:keyword value` pairs. Three value shapes are recognised:
///   * double-quoted string literal — handles `\\` and `\"` escapes
///   * bracket / paren list — `[a "b" c]` or `(a "b" c)` round-trip as one
///     captured string spanning the whole bracket pair (wave-15 / task 05
///     opt-in addition; readers split via `split_lisp_string_list`).
///   * bareword — terminates on whitespace / `(` / `)` / `[` / `]` / `"`
/// Bare `:k` with no value and `:k :next-key` patterns are still skipped so
/// the parser stays conservative for non-list authoring.
pub(crate) fn scan_keyword_pairs(sexp: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = sexp.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut esc = false;
    let mut in_comment = false;
    while i < n {
        let c = chars[i];
        if in_comment {
            if c == '\n' {
                in_comment = false;
            }
            i += 1;
            continue;
        }
        if in_string {
            if esc {
                esc = false;
                i += 1;
                continue;
            }
            if c == '\\' {
                esc = true;
                i += 1;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == ';' {
            in_comment = true;
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c != ':' {
            i += 1;
            continue;
        }
        let key_start = i + 1;
        let mut j = key_start;
        while j < n {
            let cj = chars[j];
            if cj.is_whitespace()
                || cj == '('
                || cj == ')'
                || cj == '['
                || cj == ']'
                || cj == '"'
                || cj == ':'
            {
                break;
            }
            j += 1;
        }
        if j == key_start {
            i += 1;
            continue;
        }
        let key: String = chars[key_start..j].iter().collect();
        let mut k = j;
        while k < n && chars[k].is_whitespace() {
            k += 1;
        }
        if k >= n {
            break;
        }
        let next = chars[k];
        match next {
            '"' => {
                let mut m = k + 1;
                let mut value = String::new();
                let mut esc2 = false;
                while m < n {
                    let cm = chars[m];
                    if esc2 {
                        value.push(cm);
                        esc2 = false;
                        m += 1;
                        continue;
                    }
                    if cm == '\\' {
                        esc2 = true;
                        m += 1;
                        continue;
                    }
                    if cm == '"' {
                        m += 1;
                        break;
                    }
                    value.push(cm);
                    m += 1;
                }
                out.push((key, value));
                i = m;
            }
            '[' | '(' => {
                let open = next;
                let close = if open == '[' { ']' } else { ')' };
                let mut depth = 0i64;
                let mut m = k;
                let mut esc2 = false;
                let mut in_str = false;
                while m < n {
                    let cm = chars[m];
                    if in_str {
                        if esc2 {
                            esc2 = false;
                            m += 1;
                            continue;
                        }
                        if cm == '\\' {
                            esc2 = true;
                            m += 1;
                            continue;
                        }
                        if cm == '"' {
                            in_str = false;
                        }
                        m += 1;
                        continue;
                    }
                    if cm == '"' {
                        in_str = true;
                        m += 1;
                        continue;
                    }
                    if cm == open {
                        depth += 1;
                    } else if cm == close {
                        depth -= 1;
                        if depth == 0 {
                            m += 1;
                            break;
                        }
                    }
                    m += 1;
                }
                let value: String = chars[k..m].iter().collect();
                out.push((key, value));
                i = m;
            }
            ')' | ':' => {
                i = k;
            }
            _ => {
                let mut m = k;
                while m < n {
                    let cm = chars[m];
                    if cm.is_whitespace()
                        || cm == '('
                        || cm == ')'
                        || cm == '['
                        || cm == ']'
                        || cm == '"'
                    {
                        break;
                    }
                    m += 1;
                }
                if m > k {
                    let value: String = chars[k..m].iter().collect();
                    out.push((key, value));
                    i = m;
                } else {
                    i = k;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_keyword_pairs_ignores_strings_and_comments() {
        let pairs = scan_keyword_pairs(
            r#"(plan
              :note "debug :target wrong"
              ; :target comment-wrong
              :target "mission_task_delegate")"#,
        );
        assert_eq!(
            pairs
                .iter()
                .find(|(key, _)| key == "target")
                .map(|(_, value)| value.as_str()),
            Some("mission_task_delegate")
        );
        assert!(!pairs.iter().any(|(_, value)| value == "comment-wrong"));
    }
}

/// Map a free-form target string from a plan hint to the canonical 3-target
/// surface. `flow_id_present` gates `mission_flow_run` because the inner
/// dispatcher refuses to run without a flow_id.
pub(crate) fn normalize_target(raw: &str, flow_id_present: bool) -> Option<&'static str> {
    let lower = raw.to_ascii_lowercase();
    // task_delegate keywords are most specific — check first so a plan hint
    // like "claudecode" or "code-alignment" doesn't get swallowed by the
    // generic "execution" branch.
    if lower.contains("mission_task_delegate")
        || lower.contains("task_delegate")
        || lower.contains("task-delegate")
        || lower.contains("claudecode")
        || lower.contains("code-alignment")
    {
        return Some("mission_task_delegate");
    }
    if flow_id_present
        && (lower.contains("mission_flow_run")
            || lower.contains("flow_run")
            || lower.contains("flow-run")
            || lower.contains("flow"))
    {
        return Some("mission_flow_run");
    }
    if lower.contains("mission_execution") || lower.contains("execution") {
        return Some("mission_execution");
    }
    None
}

/// Map a free-form strategy hint to one of `VALID_DISPATCH_STRATEGIES`.
/// `unknown` is treated as "no signal" so callers can fall back to the next
/// priority source. Returns `None` when the string carries no usable hint.
pub(crate) fn canonicalize_strategy(raw: &str) -> Option<&'static str> {
    let lower = raw.to_ascii_lowercase();
    for &valid in VALID_DISPATCH_STRATEGIES {
        if lower == valid {
            if valid == "unknown" {
                return None;
            }
            return Some(valid);
        }
    }
    if lower.contains("agent-team") || lower.contains("agent_team") {
        return Some("agent-team");
    }
    if lower.contains("code-alignment")
        || lower.contains("code_alignment")
        || lower.contains("fresh")
    {
        return Some("fresh-code-alignment");
    }
    if lower.contains("resident") || lower.contains("lisp-architect") || lower.contains("architect")
    {
        return Some("resident-lisp");
    }
    if lower.contains("mixed") {
        return Some("mixed");
    }
    if lower.contains("prompt") || lower.contains("fallback") {
        return Some("prompt-fallback");
    }
    None
}

/// Resolve the dispatch strategy with source-tracking precedence:
///   explicit arg > plan hint :dispatch-strategy > plan hint :parallelism > default unknown
pub(crate) fn resolve_dispatch_strategy(
    explicit: Option<&str>,
    hints: &ParsedPlanHints,
) -> (&'static str, &'static str) {
    if let Some(s) = explicit {
        let canonical = canonicalize_strategy(s).unwrap_or("unknown");
        return (canonical, "explicit_arg");
    }
    if let Some(s) = hints.dispatch_strategy.as_deref() {
        if let Some(c) = canonicalize_strategy(s) {
            return (c, "plan_hint");
        }
    }
    if let Some(p) = hints.parallelism.as_deref() {
        if let Some(c) = canonicalize_strategy(p) {
            return (c, "plan_hint");
        }
    }
    ("unknown", "default")
}
