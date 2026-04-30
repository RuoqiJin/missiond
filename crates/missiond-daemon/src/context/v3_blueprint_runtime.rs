use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_MODEL_PROFILE: &str = "coding-default-opus-4-7";
pub(crate) const DEFAULT_TIMEOUT_SECS: i64 = 1800;
pub(crate) const MIN_TIMEOUT_SECS: i64 = 60;
pub(crate) const MAX_TIMEOUT_SECS: i64 = 7200;
pub(crate) const WATCHDOG_GRACE_SECS: i64 = 120;
pub(crate) const MISSING_SESSION_PROBE_SECS: i64 = 120;
pub(crate) const DEFAULT_SLOT_TTL_SECS: i64 = 14400;
pub(crate) const MIN_SLOT_TTL_SECS: i64 = 300;
pub(crate) const MAX_SLOT_TTL_SECS: i64 = 28800;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkstationRuntimeConfig {
    slot_default_profiles: HashMap<String, String>,
    pub timeout_policy: TimeoutPolicy,
    pub slot_ttl_policy: SlotTtlPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TimeoutPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
    pub watchdog_grace_secs: i64,
    pub missing_session_probe_secs: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotTtlPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
}

#[derive(Debug)]
pub(crate) enum BlueprintConfigError {
    MissingBlueprint(PathBuf),
    Read { path: PathBuf, message: String },
    Parse(String),
}

impl fmt::Display for BlueprintConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBlueprint(path) => {
                write!(f, "V3 blueprint missing at {}", path.display())
            }
            Self::Read { path, message } => {
                write!(
                    f,
                    "failed to read V3 blueprint {}: {}",
                    path.display(),
                    message
                )
            }
            Self::Parse(message) => write!(f, "failed to parse V3 workstation-config: {}", message),
        }
    }
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_TIMEOUT_SECS,
            min_secs: MIN_TIMEOUT_SECS,
            max_secs: MAX_TIMEOUT_SECS,
            watchdog_grace_secs: WATCHDOG_GRACE_SECS,
            missing_session_probe_secs: MISSING_SESSION_PROBE_SECS,
        }
    }
}

impl Default for SlotTtlPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_SLOT_TTL_SECS,
            min_secs: MIN_SLOT_TTL_SECS,
            max_secs: MAX_SLOT_TTL_SECS,
        }
    }
}

impl Default for WorkstationRuntimeConfig {
    fn default() -> Self {
        let mut slot_default_profiles = HashMap::new();
        slot_default_profiles.insert("coder".to_string(), DEFAULT_MODEL_PROFILE.to_string());
        slot_default_profiles.insert("researcher".to_string(), DEFAULT_MODEL_PROFILE.to_string());
        slot_default_profiles.insert("ops".to_string(), "daily-sonnet".to_string());
        Self {
            slot_default_profiles,
            timeout_policy: TimeoutPolicy::default(),
            slot_ttl_policy: SlotTtlPolicy::default(),
        }
    }
}

impl WorkstationRuntimeConfig {
    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_workstation_config(&source)
    }

    pub(crate) fn default_model_profile_for_template(&self, template: &str) -> Option<&str> {
        self.slot_default_profiles.get(template).map(String::as_str)
    }

    pub(crate) fn clamp_timeout_secs(&self, timeout_secs: Option<i64>) -> i64 {
        let raw = match timeout_secs {
            Some(value) if value > 0 => value,
            _ => self.timeout_policy.default_secs,
        };
        raw.clamp(self.timeout_policy.min_secs, self.timeout_policy.max_secs)
    }

    pub(crate) fn clamp_slot_ttl_secs(&self, ttl_secs: Option<i64>) -> i64 {
        let raw = match ttl_secs {
            Some(value) if value > 0 => value,
            _ => self.slot_ttl_policy.default_secs,
        };
        raw.clamp(self.slot_ttl_policy.min_secs, self.slot_ttl_policy.max_secs)
    }
}

pub(crate) fn parse_workstation_config(
    source: &str,
) -> Result<WorkstationRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "workstation-config")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (workstation-config ...)".into()))?;
    let mut config = WorkstationRuntimeConfig::default();
    for form in find_forms(&block, "slot-template") {
        let tokens = tokenize_lisp(&form);
        if tokens.len() < 3 {
            continue;
        }
        let template = tokens[2].clone();
        if let Some(profile) = keyword_value(&tokens, ":default-model-profile") {
            config.slot_default_profiles.insert(template, profile);
        }
    }
    let timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens
                .get(2)
                .is_some_and(|name| name == "boardtask-dispatch")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy boardtask-dispatch ...) in workstation-config".into(),
            )
        })?;
    let timeout_tokens = tokenize_lisp(&timeout_form);
    config.timeout_policy = TimeoutPolicy {
        default_secs: int_keyword(&timeout_tokens, ":default_secs")?,
        min_secs: int_keyword(&timeout_tokens, ":min_secs")?,
        max_secs: int_keyword(&timeout_tokens, ":max_secs")?,
        watchdog_grace_secs: int_keyword(&timeout_tokens, ":watchdog_grace_secs")?,
        missing_session_probe_secs: int_keyword(&timeout_tokens, ":missing_session_probe_secs")?,
    };
    let ttl_form = find_forms(&block, "ttl-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens.get(2).is_some_and(|name| name == "dynamic-slot")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (ttl-policy dynamic-slot ...) in workstation-config".into(),
            )
        })?;
    let ttl_tokens = tokenize_lisp(&ttl_form);
    config.slot_ttl_policy = SlotTtlPolicy {
        default_secs: int_keyword(&ttl_tokens, ":default_secs")?,
        min_secs: int_keyword(&ttl_tokens, ":min_secs")?,
        max_secs: int_keyword(&ttl_tokens, ":max_secs")?,
    };
    Ok(config)
}

fn int_keyword(tokens: &[String], key: &str) -> Result<i64, BlueprintConfigError> {
    let value = keyword_value(tokens, key)
        .ok_or_else(|| BlueprintConfigError::Parse(format!("missing {}", key)))?;
    value
        .parse::<i64>()
        .map_err(|_| BlueprintConfigError::Parse(format!("{} must be an integer", key)))
}

fn keyword_value(tokens: &[String], key: &str) -> Option<String> {
    tokens
        .windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

fn find_forms(source: &str, head: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let Some((start, end)) = find_form_span(&source[offset..], head) else {
            break;
        };
        let absolute_start = offset + start;
        let absolute_end = offset + end;
        out.push(source[absolute_start..absolute_end].to_string());
        offset = absolute_end;
    }
    out
}

fn find_form(source: &str, head: &str) -> Option<String> {
    let (start, end) = find_form_span(source, head)?;
    Some(source[start..end].to_string())
}

fn find_form_span(source: &str, head: &str) -> Option<(usize, usize)> {
    let needle = format!("({}", head);
    let mut offset = 0;
    while offset < source.len() {
        let rel = source[offset..].find(&needle)?;
        let start = offset + rel;
        let after = source[start + needle.len()..].chars().next();
        if after.is_none_or(|c| c.is_whitespace() || c == ')' || c == '(') {
            let end = scan_form_end(source, start)?;
            return Some((start, end));
        }
        offset = start + needle.len();
    }
    None
}

fn scan_form_end(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    let mut in_comment = false;
    for (idx, ch) in source[start..].char_indices() {
        let abs = start + idx;
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            ';' => in_comment = true,
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(abs + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn tokenize_lisp(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut escape = false;
    let mut in_comment = false;
    while let Some(ch) = chars.next() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escape {
                current.push(ch);
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                tokens.push(std::mem::take(&mut current));
                in_string = false;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            ';' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                in_comment = true;
            }
            '"' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                in_string = true;
            }
            '(' | ')' | '[' | ']' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUEPRINT: &str = r#"
(missiond-blueprint
  (workstation-config
    (model-profile coding-default-opus-4-7 :spawn-model-arg nil)
    (slot-template coder :role coder :default-model-profile coding-default-opus-4-7)
    (slot-template researcher :role coder :default-model-profile coding-default-opus-4-7)
    (slot-template ops :role operator :default-model-profile daily-sonnet)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800)))
"#;

    #[test]
    fn parses_workstation_config_defaults() {
        let cfg = parse_workstation_config(BLUEPRINT).expect("parse");
        assert_eq!(
            cfg.default_model_profile_for_template("coder"),
            Some(DEFAULT_MODEL_PROFILE)
        );
        assert_eq!(
            cfg.default_model_profile_for_template("researcher"),
            Some(DEFAULT_MODEL_PROFILE)
        );
        assert_eq!(
            cfg.default_model_profile_for_template("ops"),
            Some("daily-sonnet")
        );
        assert_eq!(cfg.timeout_policy.default_secs, 1800);
        assert_eq!(cfg.timeout_policy.min_secs, 60);
        assert_eq!(cfg.timeout_policy.max_secs, 7200);
        assert_eq!(cfg.timeout_policy.watchdog_grace_secs, 120);
        assert_eq!(cfg.slot_ttl_policy.default_secs, 14400);
        assert_eq!(cfg.slot_ttl_policy.min_secs, 300);
        assert_eq!(cfg.slot_ttl_policy.max_secs, 28800);
    }

    #[test]
    fn timeout_policy_clamps_values() {
        let cfg = parse_workstation_config(BLUEPRINT).expect("parse");
        assert_eq!(cfg.clamp_timeout_secs(None), 1800);
        assert_eq!(cfg.clamp_timeout_secs(Some(5)), 60);
        assert_eq!(cfg.clamp_timeout_secs(Some(99999)), 7200);
        assert_eq!(cfg.clamp_timeout_secs(Some(3300)), 3300);
        assert_eq!(cfg.clamp_slot_ttl_secs(None), 14400);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(5)), 300);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(99_999)), 28800);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(3600)), 3600);
    }

    #[test]
    fn missing_timeout_policy_is_rejected() {
        let err = parse_workstation_config("(missiond-blueprint (workstation-config))")
            .expect_err("missing policy");
        assert!(err
            .to_string()
            .contains("timeout-policy boardtask-dispatch"));
    }

    #[test]
    fn missing_ttl_policy_is_rejected() {
        let source = r#"
(missiond-blueprint
  (workstation-config
    (slot-template coder :role coder :default-model-profile coding-default-opus-4-7)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)))
"#;
        let err = parse_workstation_config(source).expect_err("missing ttl policy");
        assert!(err.to_string().contains("ttl-policy dynamic-slot"));
    }
}
