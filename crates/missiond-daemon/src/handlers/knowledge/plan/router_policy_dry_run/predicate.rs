use super::super::ResolvedExec;
use super::Clause;
use missiond_core::types::Plan;
use serde_json::Value;

/// Pure projection of the execute context into the predicate input.
/// Mirrors the wave24-03 CLI's task-projection: the live execute
/// `args` + resolved dispatch + plan status are treated like a
/// task-contract for predicate matching.
#[derive(Debug, Clone, Default)]
pub(super) struct PredicateContext {
    kind: String,
    dispatch_strategy: String,
    owner: String,
    status: String,
    write_scope: Vec<String>,
}

pub(super) fn project_context(
    args: &Value,
    resolved: &ResolvedExec,
    plan: &Plan,
) -> PredicateContext {
    let kind = arg_string(args, "kind").unwrap_or_default();
    // dispatch_strategy: prefer the resolved value (which already
    // reflects explicit_arg > plan_hint > parallelism > default
    // precedence). The wave24-03 CLI projects from the task contract
    // value; here the resolved value IS the live equivalent.
    let dispatch_strategy = resolved.dispatch_strategy.to_string();
    let owner = arg_string(args, "owner").unwrap_or_default();
    let status = arg_string(args, "status").unwrap_or_else(|| plan.status.as_str().to_string());
    let write_scope = arg_string_array(args, "owned_files");
    PredicateContext {
        kind,
        dispatch_strategy,
        owner,
        status,
        write_scope,
    }
}

pub(super) fn arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn arg_string_array(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => {
            // Tolerate single-string-as-array (mirrors many of the
            // wave-15+ workstation-dispatch knobs).
            vec![s.trim().to_string()]
        }
        _ => Vec::new(),
    }
}

pub(super) fn evaluate_clause(clause: &Clause, ctx: &PredicateContext) -> Result<(), &'static str> {
    match clause {
        Clause::Kind(v) => {
            if &ctx.kind == v {
                Ok(())
            } else {
                Err("kind mismatch")
            }
        }
        Clause::DispatchStrategy(v) => {
            if &ctx.dispatch_strategy == v {
                Ok(())
            } else {
                Err("dispatch_strategy mismatch")
            }
        }
        Clause::Owner(v) => {
            if &ctx.owner == v {
                Ok(())
            } else {
                Err("owner mismatch")
            }
        }
        Clause::Status(v) => {
            if &ctx.status == v {
                Ok(())
            } else {
                Err("status mismatch")
            }
        }
        Clause::PathGlob(pat) => {
            let re = glob_to_regex(pat);
            let any = ctx.write_scope.iter().any(|p| re.is_match(p));
            if any {
                Ok(())
            } else {
                Err("path-glob no match")
            }
        }
        Clause::All(children) => {
            if children.is_empty() {
                return Err("empty all");
            }
            for c in children {
                evaluate_clause(c, ctx)?;
            }
            Ok(())
        }
        Clause::Any(children) => {
            if children.is_empty() {
                return Err("empty any");
            }
            let mut last: Result<(), &'static str> = Err("any: no child matched");
            for c in children {
                if evaluate_clause(c, ctx).is_ok() {
                    return Ok(());
                }
                last = Err("any: no child matched");
            }
            last
        }
    }
}

/// Minimal glob-to-regex shim. Mirrors the wave24-03 CLI / scripts/lib
/// shape: `**` matches any sequence including `/`; `*` matches any
/// non-`/` sequence; `?` matches a single non-`/` char. Other regex
/// metacharacters are escaped. Inputs and candidates are normalised
/// to forward slashes and stripped of leading `./` / `/`.
struct GlobRegex {
    pattern: String,
}

impl GlobRegex {
    fn is_match(&self, candidate: &str) -> bool {
        let candidate = normalize_path(candidate);
        // Tiny custom matcher (no regex crate dependency added). We
        // implement the predicate via a recursive descent over the
        // pattern. The pattern shape is small and exhaustive; this is
        // enough for path-glob predicates.
        glob_match(&self.pattern, &candidate)
    }
}

fn glob_to_regex(pattern: &str) -> GlobRegex {
    GlobRegex {
        pattern: normalize_path(pattern),
    }
}

fn normalize_path(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    if let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// Recursive matcher: returns true iff `pattern` matches all of
/// `candidate` from start to end. Supports `**`, `*`, `?` per the
/// shared glob shape. Iterative-style with explicit indices to keep
/// the complexity bounded (no backtracking blowup on pathological
/// inputs because the patterns we accept are small and well-formed).
fn glob_match(pattern: &str, candidate: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let c: Vec<char> = candidate.chars().collect();
    glob_match_inner(&p, 0, &c, 0)
}

fn glob_match_inner(p: &[char], pi: usize, c: &[char], ci: usize) -> bool {
    let mut pi = pi;
    let mut ci = ci;
    loop {
        if pi >= p.len() {
            return ci >= c.len();
        }
        let pc = p[pi];
        if pc == '*' {
            if pi + 1 < p.len() && p[pi + 1] == '*' {
                // `**` matches any sequence including `/`. Try every
                // possible split. Skip a following `/` so `**/foo`
                // matches both `foo` and `a/foo`.
                let mut next_pi = pi + 2;
                if next_pi < p.len() && p[next_pi] == '/' {
                    next_pi += 1;
                }
                // Try matching zero characters first, then progressively
                // more from the candidate.
                if glob_match_inner(p, next_pi, c, ci) {
                    return true;
                }
                let mut k = ci;
                while k < c.len() {
                    k += 1;
                    if glob_match_inner(p, next_pi, c, k) {
                        return true;
                    }
                }
                return false;
            } else {
                // `*` matches any non-`/` sequence.
                if glob_match_inner(p, pi + 1, c, ci) {
                    return true;
                }
                let mut k = ci;
                while k < c.len() && c[k] != '/' {
                    k += 1;
                    if glob_match_inner(p, pi + 1, c, k) {
                        return true;
                    }
                }
                return false;
            }
        }
        if pc == '?' {
            if ci >= c.len() || c[ci] == '/' {
                return false;
            }
            pi += 1;
            ci += 1;
            continue;
        }
        // Literal char.
        if ci >= c.len() || c[ci] != pc {
            return false;
        }
        pi += 1;
        ci += 1;
    }
}
