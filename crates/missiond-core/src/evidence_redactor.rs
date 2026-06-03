use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EvidenceRedactionReport {
    pub redacted: bool,
    pub redaction_count: usize,
    pub fingerprints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedText {
    pub text: String,
    pub report: EvidenceRedactionReport,
}

static SSHPASS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)(sshpass\s+-p\s+)(['"]?)([^'"\s]+)(['"]?)"#).unwrap());
static KEY_VALUE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)\b(password|api[_ -]?key|token|secret)\b(\s*[:=]\s*)(['"]?)([^'",\s}\]]+)(['"]?)"#,
    )
    .unwrap()
});
static BEARER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\b(bearer\s+)([A-Za-z0-9._~+/=-]{12,})"#).unwrap());

pub fn redact_text(input: &str) -> RedactedText {
    let mut report = EvidenceRedactionReport::default();
    let mut text = input.to_string();

    text = replace_captures(&SSHPASS_RE, &text, 3, &mut report, |caps, fingerprint| {
        format!("{}<redacted:{}>", &caps[1], fingerprint)
    });
    text = replace_captures(&KEY_VALUE_RE, &text, 4, &mut report, |caps, fingerprint| {
        if looks_like_secret_ref(&caps[4]) {
            caps[0].to_string()
        } else {
            format!("{}{}<redacted:{}>", &caps[1], &caps[2], fingerprint)
        }
    });
    text = replace_captures(&BEARER_RE, &text, 2, &mut report, |caps, fingerprint| {
        format!("{}<redacted:{}>", &caps[1], fingerprint)
    });

    RedactedText { text, report }
}

pub fn redact_json_value(value: &Value) -> Value {
    redact_json_value_with_report(value).0
}

pub fn redact_json_value_with_report(value: &Value) -> (Value, EvidenceRedactionReport) {
    let mut report = EvidenceRedactionReport::default();
    let redacted = redact_json_inner(value, None, &mut report);
    (redacted, report)
}

fn redact_json_inner(
    value: &Value,
    key_hint: Option<&str>,
    report: &mut EvidenceRedactionReport,
) -> Value {
    match value {
        Value::String(text) => {
            if key_hint.is_some_and(sensitive_key) && !looks_like_secret_ref(text) {
                let fingerprint = credential_fingerprint(text);
                note_redaction(report, fingerprint.clone());
                Value::String(format!("<redacted:{}>", fingerprint))
            } else {
                let redacted = redact_text(text);
                merge_report(report, redacted.report);
                Value::String(redacted.text)
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_json_inner(item, key_hint, report))
                .collect(),
        ),
        Value::Object(object) => {
            let mut out = Map::new();
            for (key, nested) in object {
                out.insert(key.clone(), redact_json_inner(nested, Some(key), report));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn replace_captures<F>(
    regex: &Regex,
    input: &str,
    secret_capture_index: usize,
    report: &mut EvidenceRedactionReport,
    replacement: F,
) -> String
where
    F: Fn(&regex::Captures<'_>, &str) -> String,
{
    regex
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let Some(secret) = caps.get(secret_capture_index) else {
                return caps[0].to_string();
            };
            if looks_like_secret_ref(secret.as_str()) {
                return caps[0].to_string();
            }
            let fingerprint = credential_fingerprint(secret.as_str());
            note_redaction(report, fingerprint.clone());
            replacement(caps, &fingerprint)
        })
        .to_string()
}

fn note_redaction(report: &mut EvidenceRedactionReport, fingerprint: String) {
    report.redacted = true;
    report.redaction_count += 1;
    if !report.fingerprints.contains(&fingerprint) {
        report.fingerprints.push(fingerprint);
    }
}

fn merge_report(into: &mut EvidenceRedactionReport, from: EvidenceRedactionReport) {
    if from.redacted {
        into.redacted = true;
    }
    into.redaction_count += from.redaction_count;
    for fingerprint in from.fingerprints {
        if !into.fingerprints.contains(&fingerprint) {
            into.fingerprints.push(fingerprint);
        }
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "password"
            | "passwd"
            | "token"
            | "apikey"
            | "secret"
            | "secretvalue"
            | "credential"
            | "credentialvalue"
            | "authorization"
            | "bearertoken"
    )
}

fn looks_like_secret_ref(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("secret-store://")
        || trimmed.starts_with("vault://")
        || trimmed.starts_with("op://")
}

fn credential_fingerprint(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    let short = digest
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("credential:{short}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_sshpass_password_without_redacting_secret_ref() {
        let redacted = redact_text(
            "sshpass -p 'plain-password' ssh root@104.194.81.38 secret-store://infra/bwg-vps/tunnel-ssh",
        );
        assert!(redacted.report.redacted);
        assert!(redacted.text.contains("sshpass -p <redacted:credential:"));
        assert!(!redacted.text.contains("plain-password"));
        assert!(redacted
            .text
            .contains("secret-store://infra/bwg-vps/tunnel-ssh"));
    }

    #[test]
    fn redacts_json_sensitive_keys_and_inline_tokens() {
        let (value, report) = redact_json_value_with_report(&json!({
            "token": "abc123abc123abc123",
            "note": "Authorization: Bearer supersecrettoken123",
            "secretRef": "secret-store://deploy-agent/token"
        }));
        assert!(report.redacted);
        let rendered = serde_json::to_string(&value).unwrap();
        assert!(!rendered.contains("abc123abc123abc123"));
        assert!(!rendered.contains("supersecrettoken123"));
        assert!(rendered.contains("secret-store://deploy-agent/token"));
    }
}
