pub(super) const ALLOW_SOURCE_FALLBACK_ENV: &str = "MISSIOND_V3_ALLOW_SOURCE_FALLBACK";
pub(super) const COMPILE_RUNTIME_ACTION: &str = "node scripts/compile-v3-runtime.mjs --json";

pub(super) fn allowed() -> bool {
    if !(cfg!(debug_assertions) || cfg!(test)) {
        return false;
    }
    std::env::var(ALLOW_SOURCE_FALLBACK_ENV)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}
