//! Engine layer — two consolidated engines.
//!
//! Intent Engine:   意图→阶段路由→DAG解析→派发 (autopilot, flow, scheduler, workflow)
//! Learning Engine: 事件监听→知识提取→决策泛化→模式分析 (decision, extraction, harvest, timeline)

pub mod flow;
pub mod intent_engine;
pub mod learning_engine;

// ── Backward-compatible re-exports ──
// Keeps `use engine::autopilot` / `use crate::autopilot` working everywhere.
pub use intent_engine::autopilot;
pub use intent_engine::flow_engine;
pub use intent_engine::memory_scheduler;
pub use learning_engine::decision_engine;
pub use learning_engine::decision_harvest;
pub use learning_engine::extraction;
