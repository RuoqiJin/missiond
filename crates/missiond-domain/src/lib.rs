//! Pure MissionD domain contracts.
//!
//! This crate is the first landing zone for the staged `missiond-core` split.
//! Phase 1 keeps `missiond-core` as the compatibility facade while new domain
//! contracts move here without depending on database, transport, provider, or
//! daemon infrastructure crates.

pub mod architecture {
    /// Current split phase: `missiond-core` still re-exports broad legacy APIs.
    pub const MISSIOND_CORE_COMPAT_FACADE: &str = "missiond-core-compat-facade";

    /// Domain crates must not depend on adapter crates such as daemon, store,
    /// transport, provider, or workspace-specific runtime implementations.
    pub const DOMAIN_DEPENDENCY_RULE: &str = "domain-must-not-depend-on-adapters";
}

pub mod ids {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct BoardTaskId(pub String);

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct SlotId(pub String);

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct WorkflowRunId(pub String);
}

#[cfg(test)]
mod tests {
    use super::architecture::MISSIOND_CORE_COMPAT_FACADE;
    use super::ids::BoardTaskId;

    #[test]
    fn domain_ids_are_wire_transparent() {
        let value = serde_json::to_value(BoardTaskId("task-1".to_string())).unwrap();
        assert_eq!(value, "task-1");
    }

    #[test]
    fn split_phase_is_explicit() {
        assert_eq!(MISSIOND_CORE_COMPAT_FACADE, "missiond-core-compat-facade");
    }
}
