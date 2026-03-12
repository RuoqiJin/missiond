//! Core module for missiond
//!
//! This module provides the core management components:
//! - SlotManager: Workstation configuration management
//! - PermissionPolicy: Tool permission checking
//! - Inbox: Message inbox management
//! - MissionControl: Main coordinator

mod inbox;
mod mission_control;
mod permission;
mod slot_manager;

pub use inbox::Inbox;
pub use mission_control::{ExecutionMode, MissionControl, MissionControlOptions};
pub use permission::{PermissionConfig, PermissionDecision, PermissionPolicy, PermissionRule};
pub use slot_manager::{SlotManager, SlotReloadResult};
