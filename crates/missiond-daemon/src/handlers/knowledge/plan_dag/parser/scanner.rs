use super::super::acceptance::{AcceptanceMode, AcceptanceRequires};
use super::super::rollback::{RollbackCascadeMode, RollbackPolicy};
use super::types::{DagNode, ParsedDag, FAILURE_POLICY_CONTINUE, FAILURE_POLICY_FAIL_FAST};

/// Top-level entry: parse plan.sexp_text for `(node ...)` forms only.
pub(in crate::handlers::knowledge::plan_dag) fn parse_plan_dag(sexp: &str) -> ParsedDag {
    let mut out = ParsedDag::default();
    for form in scan_top_level_forms(sexp) {
        let head = top_form_head(&form).unwrap_or_default();
        let head_lc = head.to_ascii_lowercase();
        if head_lc == "node" {
            if let Some(node) = parse_node_form(&form) {
                out.nodes.push(node);
            }
        } else if !head.is_empty() {
            // Non-node sibling — record verbatim so authors can see what the
            // scheduler skipped (e.g., :goal, :phases, :tasks, comments).
            out.unsupported_top_forms.push(form);
        }
    }
    out
}

/// Walk through the outer plan envelope and yield the s-expressions sitting at
/// "top level" inside it. We treat anything inside the outermost paren of the
/// plan envelope as a sibling to be considered. This is intentionally
/// shallow — we do NOT recurse into nested forms looking for `(node ...)`,
/// because that would silently consume nodes meant for sub-phases.
fn scan_top_level_forms(sexp: &str) -> Vec<String> {
    let trimmed = sexp.trim();
    let bytes: Vec<char> = trimmed.chars().collect();
    let n = bytes.len();
    if n == 0 || bytes[0] != '(' {
        return Vec::new();
    }
    // Find the slice immediately inside the outermost paren.
    // Strategy: skip the head symbol of the outer envelope, then collect
    // sibling forms until we close the outer paren.
    let mut i = 1usize;
    // Skip whitespace
    while i < n && bytes[i].is_whitespace() {
        i += 1;
    }
    // Skip the head symbol (e.g. `plan`, `plan-draft`, `PLAN`).
    while i < n
        && !bytes[i].is_whitespace()
        && bytes[i] != '('
        && bytes[i] != ')'
        && bytes[i] != '"'
    {
        i += 1;
    }
    let mut forms: Vec<String> = Vec::new();
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut esc = false;
    let mut current_start: Option<usize> = None;
    while i < n {
        let c = bytes[i];
        if in_string {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == '(' {
            if depth == 0 {
                current_start = Some(i);
            }
            depth += 1;
            i += 1;
            continue;
        }
        if c == ')' {
            depth -= 1;
            if depth == 0 {
                if let Some(start) = current_start.take() {
                    let form: String = bytes[start..=i].iter().collect();
                    forms.push(form);
                }
                i += 1;
                continue;
            }
            if depth < 0 {
                // Closing the outer envelope — stop.
                break;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    forms
}

/// Get the head symbol of a top-level form like `(node :id ...)` -> `node`.
fn top_form_head(form: &str) -> Option<String> {
    let trimmed = form.trim_start();
    let inner = trimmed.strip_prefix('(')?.trim_start();
    let mut end = 0usize;
    for (idx, ch) in inner.char_indices() {
        if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' {
            break;
        }
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        None
    } else {
        Some(inner[..end].to_string())
    }
}

/// Parse one `(node :k v :k v ...)` form into a `DagNode`. Returns None when
/// the form is missing `:id` or `:target` (the two required fields). Unknown
/// keyword fields are captured into `unsupported_fields`.
fn parse_node_form(form: &str) -> Option<DagNode> {
    let pairs = scan_keyword_pairs(form);
    let mut id: Option<String> = None;
    let mut target: Option<String> = None;
    let mut objective: Option<String> = None;
    let mut depends_on: Vec<String> = Vec::new();
    let mut condition: Option<String> = None;
    let mut failure_policy: Option<String> = None;
    let mut timeout_ms: Option<i64> = None;
    let mut dispatch_strategy: Option<String> = None;
    let mut target_project: Option<String> = None;
    let mut requested_cwd: Option<String> = None;
    let mut flow_id: Option<String> = None;
    let mut scope: Option<String> = None;
    let mut commit_policy: Option<String> = None;
    let mut owned_files_raw: Option<String> = None;
    let mut forbidden_files_raw: Option<String> = None;
    let mut acceptance_commands_raw: Option<String> = None;
    // wave-17 / task 03 — declarative acceptance evaluator hints.
    let mut acceptance_mode_raw: Option<String> = None;
    let mut acceptance_evidence_keys_raw: Option<String> = None;
    // wave-18 / task 03 — cross-node acceptance fan-in hints.
    let mut acceptance_depends_on: Vec<String> = Vec::new();
    let mut acceptance_requires_raw: Option<String> = None;
    let mut acceptance_source_node: Option<String> = None;
    let mut workstation_dispatch_flag: Option<String> = None;
    let mut review_gate: Option<String> = None;
    let mut review_action: Option<String> = None;
    let mut review_text: Option<String> = None;
    // wave-16 / task 05 — bounded per-node retry hints. Both the count
    // and the delay are parsed strictly inside this loop; the first
    // hint failure is captured into `retry_parse_error` so the
    // validator can fail-fast at `build_validated_dag` time without
    // re-tokenising the form. We keep only the FIRST error (later
    // hints still flow through their normal handler / unsupported
    // path so the audit trail captures every signal).
    let mut retry_count: Option<u32> = None;
    let mut retry_delay_ms: Option<u64> = None;
    let mut retry_parse_error: Option<(String, String, String)> = None;
    // wave-17 / task 04 — conservative rollback descriptor hints.
    let mut rollback_policy: Option<String> = None;
    let mut rollback_objective: Option<String> = None;
    let mut rollback_owned_files_raw: Option<String> = None;
    let mut rollback_acceptance_commands_raw: Option<String> = None;
    // wave-18 / task 04 — cascade rollback hints.
    let mut compensates: Option<String> = None;
    let mut rollback_cascade: Option<String> = None;
    let mut rollback_after: Vec<String> = Vec::new();
    // wave-19 / task 10 — forward `:compensate-node` declaration on the
    // failing-node side (alias `:compensate-ref`). Validated against the
    // reverse `:compensates` direction in `build_validated_dag`.
    let mut compensate_node: Option<String> = None;
    let mut unsupported_fields: Vec<(String, String)> = Vec::new();

    for (raw_key, value) in pairs {
        let key = raw_key.to_ascii_lowercase();
        match key.as_str() {
            "id" => set_first(&mut id, &value),
            "target" | "target-tool" | "tool" => set_first(&mut target, &value),
            "objective" => set_first(&mut objective, &value),
            "depends-on" | "depends_on" | "deps" => {
                depends_on = parse_id_list(&value);
            }
            "condition" => set_first(&mut condition, &value),
            "failure-policy" | "failure_policy" => set_first(&mut failure_policy, &value),
            "timeout-ms" | "timeout_ms" => {
                if let Ok(n) = value.trim().parse::<i64>() {
                    if timeout_ms.is_none() {
                        timeout_ms = Some(n);
                    }
                }
            }
            "dispatch-strategy" | "dispatch_strategy" => set_first(&mut dispatch_strategy, &value),
            "target-project" | "target_project" | "project" => {
                set_first(&mut target_project, &value)
            }
            "requested-cwd" | "requested_cwd" | "cwd" => set_first(&mut requested_cwd, &value),
            "flow-id" | "flow_id" => set_first(&mut flow_id, &value),
            // wave-15 / task 05 — workstation-dispatch hint contract.
            // Captured here so the scheduler can route eligible nodes
            // without a second parse pass; only consumed when
            // `:workstation-dispatch true` is also set.
            "scope" => set_first(&mut scope, &value),
            "commit-policy" | "commit_policy" => set_first(&mut commit_policy, &value),
            "owned-files" | "owned_files" => set_first(&mut owned_files_raw, &value),
            "forbidden-files" | "forbidden_files" => set_first(&mut forbidden_files_raw, &value),
            "acceptance-commands" | "acceptance_commands" => {
                set_first(&mut acceptance_commands_raw, &value)
            }
            // wave-17 / task 03 — declarative acceptance evaluator hints.
            // `:acceptance-mode` is parsed strictly: unknown values land
            // BOTH on the typed slot AND in `unsupported_fields` so the
            // scheduler safely degrades to the manual-required default
            // while the typo surfaces through `node_hint_summary`.
            "acceptance-mode" | "acceptance_mode" => {
                let raw = value.trim();
                if !raw.is_empty() && AcceptanceMode::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut acceptance_mode_raw, &value);
            }
            "acceptance-evidence-keys" | "acceptance_evidence_keys" => {
                set_first(&mut acceptance_evidence_keys_raw, &value)
            }
            // wave-18 / task 03 — cross-node acceptance fan-in hints.
            // `:acceptance-depends-on` accepts the same shapes as
            // `:depends-on` (`["a" "b"]` / `(a b)` / bareword run);
            // `:acceptance-requires` is parsed strictly so a typo lands
            // BOTH on the typed slot AND in `unsupported_fields` while
            // the validator raises a structured error before the
            // scheduler dispatches the node. Single
            // `:acceptance-source-node` is captured verbatim; only
            // consumed under `evidence_keys` mode.
            "acceptance-depends-on" | "acceptance_depends_on" => {
                if acceptance_depends_on.is_empty() {
                    acceptance_depends_on = parse_id_list(&value);
                }
            }
            "acceptance-requires" | "acceptance_requires" => {
                let raw = value.trim();
                if !raw.is_empty() && AcceptanceRequires::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut acceptance_requires_raw, &value);
            }
            "acceptance-source-node" | "acceptance_source_node" => {
                set_first(&mut acceptance_source_node, &value)
            }
            "workstation-dispatch" | "workstation_dispatch" => {
                set_first(&mut workstation_dispatch_flag, &value)
            }
            // wave-16 / task 04 — review-gate hint contract. `:review-gate`
            // is the gate kind (recognised: "none", "question-event");
            // unrecognised raw values still land on the typed slot AND
            // get recorded into `unsupported_fields` so the typo surfaces
            // through `node_hint_summary` while the scheduler safely
            // dispatches as if no gate was set.
            "review-gate" | "review_gate" => {
                let raw = value.trim();
                if !raw.is_empty() {
                    let lc = raw.to_ascii_lowercase();
                    if !matches!(lc.as_str(), "none" | "question-event" | "question_event") {
                        unsupported_fields.push((raw_key.clone(), value.clone()));
                    }
                }
                set_first(&mut review_gate, &value);
            }
            "review-action" | "review_action" => set_first(&mut review_action, &value),
            "review-text" | "review_text" => set_first(&mut review_text, &value),
            // wave-16 / task 05 — bounded per-node retry policy. Two
            // spellings, distinct semantics:
            //   `:retry-count N`   = N **additional** attempts beyond
            //                        the first (so total = N+1).
            //   `:max-attempts N`  = N **total** attempts including
            //                        the first (so retry_count = N-1).
            // Both lower into `retry_count` (additional retries) so the
            // runtime has a single source of truth; the parser
            // converts on the way in. First hint wins; later ones are
            // ignored so a duplicate doesn't silently shadow the author's
            // earlier declaration.
            //
            // Strict parsing: any non-numeric / negative value lands
            // in `retry_parse_error` and the validator raises a
            // structured `DagBuildError::InvalidRetryHint` BEFORE the
            // scheduler ever sees the node — silent fall-through to
            // "no retry" would lose the author's policy.
            "retry-count" | "retry_count" => {
                if retry_count.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        Ok(n) if n >= 0 => {
                            // Preserve the raw upper bound so callers
                            // can see what they declared; the cap is
                            // applied by `effective_max_attempts`.
                            retry_count = Some(n.min(u32::MAX as i64) as u32);
                        }
                        Ok(_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a non-negative integer".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            "max-attempts" | "max_attempts" => {
                if retry_count.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        // `:max-attempts 0` is meaningless (zero
                        // attempts = never run) — we reject it as a
                        // structured parse error so the author sees
                        // the typo instead of a silently-skipped node.
                        Ok(n) if n >= 1 => {
                            // Convert total attempts → additional
                            // retries. Subtract one then clamp to u32.
                            let extra = (n - 1).min(u32::MAX as i64) as u32;
                            retry_count = Some(extra);
                        }
                        Ok(_zero_or_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a positive integer (>= 1)".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            "retry-delay-ms" | "retry_delay_ms" => {
                if retry_delay_ms.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        Ok(n) if n >= 0 => {
                            retry_delay_ms = Some(n as u64);
                        }
                        Ok(_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a non-negative integer (ms)".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            // wave-17 / task 04 — conservative rollback descriptor
            // contract. Strict parsing: unrecognised raw values land
            // BOTH on the typed slot AND in `unsupported_fields` so
            // the scheduler safely degrades to "no rollback" while
            // the typo surfaces through `node_hint_summary`.
            "rollback-policy" | "rollback_policy" => {
                let raw = value.trim();
                if !raw.is_empty() && RollbackPolicy::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut rollback_policy, &value);
            }
            "rollback-objective" | "rollback_objective" => {
                set_first(&mut rollback_objective, &value)
            }
            "rollback-owned-files" | "rollback_owned_files" => {
                set_first(&mut rollback_owned_files_raw, &value)
            }
            "rollback-acceptance-commands" | "rollback_acceptance_commands" => {
                set_first(&mut rollback_acceptance_commands_raw, &value)
            }
            // wave-18 / task 04 — cascade rollback hint contract.
            //
            // `:compensates "<failed-node-id>"` declares THIS node as a
            // candidate compensation step for the named failed node. The
            // cascade evaluator (which runs AFTER the named node's final
            // failed attempt) consumes the field; outside that flow it
            // is pure metadata.
            //
            // `:rollback-cascade "none|plan|dispatch-safe"` opts the
            // failed (cascade-root) node into the cascade evaluator.
            // Strict parsing: unrecognised raw values land BOTH on the
            // typed slot AND in `unsupported_fields` so the scheduler
            // safely degrades to "no cascade" while the typo surfaces
            // through `node_hint_summary`.
            //
            // `:rollback-after ["node-a" "node-b"]` is an additional
            // ordering hint for cascade compensation order. Same shape
            // as `:depends-on` (paren / bracket / bareword run); never
            // promoted to a real `:depends-on` so forward dispatch order
            // is unaffected.
            "compensates" => set_first(&mut compensates, &value),
            // wave-19 / task 10 — forward compensate ref. Two spellings,
            // identical semantics: `:compensate-node "<comp-id>"` /
            // `:compensate-ref "<comp-id>"` declared on the failing node
            // points AT the compensation node id. First hint wins; later
            // duplicates are ignored so a typo cannot silently shadow
            // the author's first declaration. Plan-level validation
            // (declared-id resolution, self-ref rejection, agreement
            // with reverse `:compensates`) runs in `build_validated_dag`.
            "compensate-node" | "compensate_node" | "compensate-ref" | "compensate_ref" => {
                set_first(&mut compensate_node, &value)
            }
            "rollback-cascade" | "rollback_cascade" => {
                let raw = value.trim();
                if !raw.is_empty() && RollbackCascadeMode::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut rollback_cascade, &value);
            }
            "rollback-after" | "rollback_after" => {
                if rollback_after.is_empty() {
                    rollback_after = parse_id_list(&value);
                }
            }
            _ => {
                unsupported_fields.push((raw_key, value));
            }
        }
    }

    let id = id?;
    let target = target?;
    let policy = failure_policy.unwrap_or_else(|| FAILURE_POLICY_FAIL_FAST.to_string());
    let policy = match policy.as_str() {
        FAILURE_POLICY_FAIL_FAST | FAILURE_POLICY_CONTINUE => policy,
        _ => {
            // Unknown policy → record into unsupported_fields and fall back
            // to fail-fast (the safe default).
            unsupported_fields.push(("failure-policy".to_string(), policy));
            FAILURE_POLICY_FAIL_FAST.to_string()
        }
    };
    Some(DagNode {
        id,
        target,
        objective,
        depends_on,
        condition,
        failure_policy: policy,
        timeout_ms,
        dispatch_strategy,
        target_project,
        requested_cwd,
        flow_id,
        scope,
        commit_policy,
        owned_files_raw,
        forbidden_files_raw,
        acceptance_commands_raw,
        acceptance_mode_raw,
        acceptance_evidence_keys_raw,
        acceptance_depends_on,
        acceptance_requires_raw,
        acceptance_source_node,
        workstation_dispatch_flag,
        review_gate,
        review_action,
        review_text,
        retry_count,
        retry_delay_ms,
        retry_parse_error,
        rollback_policy,
        rollback_objective,
        rollback_owned_files_raw,
        rollback_acceptance_commands_raw,
        compensates,
        compensate_node,
        rollback_cascade,
        rollback_after,
        unsupported_fields,
    })
}

fn set_first(slot: &mut Option<String>, value: &str) {
    if slot.is_none() {
        let v = value.trim();
        if !v.is_empty() {
            *slot = Some(v.to_string());
        }
    }
}

/// Parse a depends-on value of the shape `["a" "b"]` or `(a b)`. Both shapes
/// are common in PLAN.lisp authoring; we accept either and split on whitespace.
/// Quoted strings have their quotes stripped; bare-words pass through.
fn parse_id_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .or_else(|| trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')))
        .unwrap_or(trimmed);
    let mut out = Vec::new();
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
            // quoted
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
                i += 1; // consume closing quote
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

/// Local copy of the keyword/value scanner — simpler than the one in plan.rs
/// because this one is scoped to a single `(node :k v ...)` form. Recognises
/// quoted strings and bareword values; list-shaped values like
/// `:depends-on ["a" "b"]` are also captured (the whole bracket span becomes
/// the value string).
fn scan_keyword_pairs(form: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = form.chars().collect();
    let n = chars.len();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0usize;
    let mut in_string = false;
    let mut esc = false;
    while i < n {
        let c = chars[i];
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
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c != ':' {
            i += 1;
            continue;
        }
        // start of keyword
        let key_start = i + 1;
        let mut j = key_start;
        while j < n {
            let cj = chars[j];
            if cj.is_whitespace() || cj == '(' || cj == ')' || cj == '"' || cj == ':' {
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
                // Capture the entire bracket/paren span as the value so
                // `:depends-on ["a" "b"]` and `:depends-on (a b)` round-trip.
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
            ':' | ')' => {
                // Bare keyword without a value — skip.
                i = k;
            }
            _ => {
                let mut m = k;
                while m < n {
                    let cm = chars[m];
                    if cm.is_whitespace() || cm == '(' || cm == ')' || cm == '"' {
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
