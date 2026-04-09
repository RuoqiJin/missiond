//! Core types for missiond
//!
//! Mirrors the TypeScript definitions in packages/missiond/src/types.ts

mod slot;
mod task;
mod board;
mod question;
mod knowledge;
mod skill;
mod conversation;
mod incident;
mod infra;
mod dynamic_slot;
mod async_job;
mod project;

pub use slot::*;
pub use task::*;
pub use board::*;
pub use question::*;
pub use knowledge::*;
pub use skill::*;
pub use conversation::*;
pub use incident::*;
pub use infra::*;
pub use dynamic_slot::*;
pub use async_job::*;
pub use project::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_roundtrip() {
        let statuses = [
            TaskStatus::Queued,
            TaskStatus::Running,
            TaskStatus::Done,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ];

        for status in statuses {
            let s = status.as_str();
            let parsed = TaskStatus::from_str(s).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_event_type_roundtrip() {
        let types = [
            EventType::TaskCreated,
            EventType::TaskStarted,
            EventType::TaskProgress,
            EventType::TaskDone,
            EventType::TaskFailed,
            EventType::TaskCancelled,
        ];

        for t in types {
            let s = t.as_str();
            let parsed = EventType::from_str(s).unwrap();
            assert_eq!(t, parsed);
        }
    }

    #[test]
    fn test_task_serialization() {
        let task = Task {
            id: "task-123".to_string(),
            role: "worker".to_string(),
            prompt: "Do something".to_string(),
            status: TaskStatus::Queued,
            slot_id: None,
            session_id: None,
            result: None,
            error: None,
            created_at: 1234567890,
            started_at: None,
            finished_at: None,
        };

        let json = serde_json::to_string(&task).unwrap();
        let parsed: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(task.id, parsed.id);
        assert_eq!(task.role, parsed.role);
        assert_eq!(task.status, parsed.status);
    }

    #[test]
    fn test_slot_config_serialization() {
        let config = SlotConfig {
            id: "slot-1".to_string(),
            role: "worker".to_string(),
            description: "A worker slot".to_string(),
            engine: CliEngine::default(),
            cwd: Some("/path/to/work".to_string()),
            mcp_config: None,
            lifecycle: None,
            auto_start: Some(true),
            dangerously_skip_permissions: None,
            model: None,
            traits: vec![],
            env: None,
            category: None,
            initial_prompt: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"id\":\"slot-1\""));
        assert!(json.contains("\"autoStart\":true"));
        assert!(json.contains("\"engine\":\"claude_code\""));
    }

    #[test]
    fn test_cli_engine_backward_compat() {
        // JSON without engine field should default to ClaudeCode
        let json = r#"{"id":"s1","role":"worker","description":"test"}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.engine, CliEngine::ClaudeCode);

        // JSON with engine: gemini
        let json = r#"{"id":"s2","role":"researcher","description":"test","engine":"gemini"}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.engine, CliEngine::Gemini);

        // JSON with engine: codex
        let json = r#"{"id":"s3","role":"vision","description":"test","engine":"codex"}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.engine, CliEngine::Codex);
    }

    #[test]
    fn test_cli_engine_display() {
        assert_eq!(CliEngine::ClaudeCode.to_string(), "claude_code");
        assert_eq!(CliEngine::Gemini.to_string(), "gemini");
        assert_eq!(CliEngine::Codex.to_string(), "codex");
    }

    #[test]
    fn test_engine_capability_inference() {
        // Claude Code should get SupportsMcp
        let mut config = SlotConfig {
            id: "s1".into(), role: "worker".into(), description: "test".into(),
            engine: CliEngine::ClaudeCode,
            cwd: None, mcp_config: None, lifecycle: None, auto_start: None,
            dangerously_skip_permissions: None, model: None, traits: vec![], env: None, category: None, initial_prompt: None,
        };
        config.apply_default_traits();
        assert!(config.supports_mcp());
        assert!(!config.supports_vision());

        // Codex should get SupportsVision
        let mut config = SlotConfig {
            id: "s2".into(), role: "vision".into(), description: "test".into(),
            engine: CliEngine::Codex,
            cwd: None, mcp_config: None, lifecycle: None, auto_start: None,
            dangerously_skip_permissions: None, model: None, traits: vec![], env: None, category: None, initial_prompt: None,
        };
        config.apply_default_traits();
        assert!(config.supports_vision());
        assert!(!config.supports_mcp());

        // Memory role + Claude engine should get both IsMetaAgent and SupportsMcp
        let mut config = SlotConfig {
            id: "s3".into(), role: "memory".into(), description: "test".into(),
            engine: CliEngine::ClaudeCode,
            cwd: None, mcp_config: None, lifecycle: None, auto_start: None,
            dangerously_skip_permissions: None, model: None, traits: vec![], env: None, category: None, initial_prompt: None,
        };
        config.apply_default_traits();
        assert!(config.is_meta_agent());
        assert!(config.supports_mcp());
    }

    #[test]
    fn test_lifecycle_is_persistent() {
        // lifecycle: persistent → true
        let json = r#"{"id":"s1","role":"w","description":"t","lifecycle":"persistent"}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert!(config.is_persistent());

        // lifecycle: on_demand → false
        let json = r#"{"id":"s2","role":"w","description":"t","lifecycle":"on_demand"}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert!(!config.is_persistent());

        // No lifecycle, auto_start: true → true (legacy)
        let json = r#"{"id":"s3","role":"w","description":"t","autoStart":true}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert!(config.is_persistent());

        // No lifecycle, no auto_start → false
        let json = r#"{"id":"s4","role":"w","description":"t"}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert!(!config.is_persistent());

        // lifecycle overrides auto_start
        let json = r#"{"id":"s5","role":"w","description":"t","lifecycle":"on_demand","autoStart":true}"#;
        let config: SlotConfig = serde_json::from_str(json).unwrap();
        assert!(!config.is_persistent());
    }

    #[test]
    fn test_parse_ssh_targets_privatecloud() {
        let server = InfraServer {
            id: "privatecloud".into(),
            name: "私有云构建机".into(),
            provider: "self-hosted".into(),
            host: Some("198.51.100.1".into()),
            lan: Some("192.168.1.100".into()),
            location: None,
            roles: vec![],
            tags: vec![],
            description: Some(
                "Build server. Access: \
                 1) LAN: ssh testuser@192.168.1.100 (密码 testpass, 同局域网最快) \
                 2) Tailscale: ssh testuser@100.64.0.1 (testuser-buildbox) \
                 3) Tunnel: ssh -p 2222 testuser@198.51.100.1 (密码 testpass, 公网)"
                    .into(),
            ),
            health_endpoint: None,
        };

        let targets = server.parse_ssh_targets();
        assert_eq!(targets.len(), 3);

        // LAN first
        assert_eq!(targets[0].via, "lan");
        assert_eq!(targets[0].host, "192.168.1.100");
        assert_eq!(targets[0].user, "testuser");
        assert_eq!(targets[0].port, 22);
        assert_eq!(targets[0].password.as_deref(), Some("testpass"));

        // Tailscale second
        assert_eq!(targets[1].via, "tailscale");
        assert_eq!(targets[1].host, "100.64.0.1");

        // Public last
        assert_eq!(targets[2].via, "public");
        assert_eq!(targets[2].host, "198.51.100.1");
        assert_eq!(targets[2].port, 2222);
        assert_eq!(targets[2].password.as_deref(), Some("testpass"));
    }

    #[test]
    fn test_parse_ssh_targets_windows() {
        let server = InfraServer {
            id: "win-3090ti".into(),
            name: "Windows".into(),
            provider: "self-hosted".into(),
            host: Some("192.168.1.101".into()),
            lan: None,
            location: None,
            roles: vec![],
            tags: vec![],
            description: Some(
                "访问: 1) LAN: ssh testuser@192.168.1.101 (密码 testpass) \
                 2) Tailscale: ssh testuser@100.64.0.2 (testuser-gpu)"
                    .into(),
            ),
            health_endpoint: None,
        };

        let targets = server.parse_ssh_targets();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].user, "testuser");
        assert_eq!(targets[0].host, "192.168.1.101");
        // Tailscale
        assert_eq!(targets[1].via, "tailscale");
        assert_eq!(targets[1].host, "100.64.0.2");
    }

    #[test]
    fn test_parse_ssh_targets_no_description() {
        let server = InfraServer {
            id: "test".into(),
            name: "Test".into(),
            provider: "gcp".into(),
            host: None,
            lan: None,
            location: None,
            roles: vec![],
            tags: vec![],
            description: None,
            health_endpoint: None,
        };
        assert!(server.parse_ssh_targets().is_empty());
    }
}
