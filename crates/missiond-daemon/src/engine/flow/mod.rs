//! General-purpose Flow Engine — declarative node-based workflow orchestration.
//!
//! Flow definitions are YAML files in `.missiond/flows/`. Each flow is a sequence
//! of typed nodes (LlmCall, SlotTask, McpTool, DaemonAction) connected by
//! variable interpolation (`${key}` references to previous node outputs).
//!
//! Architecture:
//! - FlowDefinition: declarative description loaded from YAML
//! - FlowRunner: stateful executor that drives nodes sequentially
//! - FlowContext: runtime variable store (persisted to BoardTask.flow_context)
//! - NodeType handlers: delegate to existing infrastructure (Gemini, SlotManager, MCP)

pub mod runner;
pub mod handlers;
pub mod loader;

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Error handling policy for a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorPolicy {
    /// Halt the entire flow on error (default).
    Stop,
    /// Skip this node and continue to the next.
    Skip,
    /// Retry up to N times with exponential backoff.
    Retry(u32),
}

impl Default for ErrorPolicy {
    fn default() -> Self {
        Self::Stop
    }
}

/// A single task in a ParallelSlotTasks node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelTaskSpec {
    pub id: String,
    pub prompt: String,
}

/// How to aggregate ParallelSlotTasks fan-out results.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GatherStrategy {
    /// Collect every result (success or failure) into a JSON array (default).
    #[default]
    Aggregate,
    /// Succeed only if all tasks succeed; otherwise fail the node.
    AllSuccess,
    /// Succeed if at least one task succeeds; fail only if all fail.
    AnySuccess,
}

/// The type of work a flow node performs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeType {
    /// Daemon-side LLM call (Gemini/Sonnet via HTTP, no PTY slot).
    LlmCall {
        provider: String,
        prompt: String,
        #[serde(default = "default_max_tokens")]
        max_tokens: u32,
    },
    /// Spawn a PTY slot, send prompt, wait for completion via submit_phase_result.
    SlotTask {
        #[serde(default = "default_model")]
        model: String,
        prompt: String,
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
    },
    /// Call an MCP tool (reuses WorkflowExecutor's tool dispatch).
    McpTool {
        tool_name: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Pure Rust daemon action (read files, close slots, update KB, etc.).
    DaemonAction {
        action: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Fan out N prompts to M running slots with bounded parallelism (POC: fire-and-forget).
    ParallelSlotTasks {
        #[serde(default = "default_parallelism")]
        parallelism: usize,
        tasks: Vec<ParallelTaskSpec>,
        #[serde(default)]
        gather: GatherStrategy,
        #[serde(default = "default_parallel_timeout")]
        timeout_secs: u64,
    },
}

fn default_max_tokens() -> u32 { 65536 }
fn default_model() -> String { "opus".to_string() }
fn default_timeout() -> u64 { 3600 }
fn default_parallelism() -> usize { 3 }
fn default_parallel_timeout() -> u64 { 1800 }

/// A single node in a flow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowNode {
    pub id: String,
    #[serde(flatten)]
    pub node_type: NodeType,
    /// Store this node's output in context under this key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_as: Option<String>,
    /// DAG dependencies (v1: linear execution, reserved for future DAG scheduler).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Error handling policy.
    #[serde(default)]
    pub on_error: ErrorPolicy,
}

/// A complete flow definition, loaded from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDefinition {
    pub id: String,
    pub name: String,
    pub nodes: Vec<FlowNode>,
}

/// Runtime context passed between nodes. Persisted to BoardTask.flow_context.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlowContext {
    /// Key-value store: each node's output stored under its `save_as` key.
    pub vars: HashMap<String, String>,
    /// Current node index (for resume after crash).
    #[serde(default)]
    pub current_node: usize,
    /// Node execution log.
    #[serde(default)]
    pub completed_nodes: Vec<String>,
    /// Error from last failed node (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl FlowContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve `${key}` references in a template string.
    pub fn resolve_vars(&self, template: &str) -> String {
        let mut result = template.to_string();
        for (key, value) in &self.vars {
            result = result.replace(&format!("${{{}}}", key), value);
        }
        result
    }

    /// Set a variable in the context.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    /// Get a variable from the context.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(|s| s.as_str())
    }
}
