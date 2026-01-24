//! Cross-machine Tasks Synchronization
//!
//! Enables syncing CC Tasks events across multiple machines via WebSocket relay.
//!
//! # Architecture
//!
//! ```text
//! Machine A (missiond)    Relay Server    Machine B (missiond)
//!        │                     │                    │
//!        ├──TaskEvent──────────►                    │
//!        │                     ├──TaskEvent────────►│
//!        │◄──TaskEvent─────────┤                    │
//!        │                     │◄──TaskEvent────────┤
//! ```
//!
//! # Usage
//!
//! Add to ~/.xjp-mission/config.yaml:
//! ```yaml
//! sync:
//!   upstream_url: "ws://relay.example.com:9121/sync"
//!   machine_id: "macbook-pro"  # Optional, defaults to hostname
//! ```

mod relay;
mod client;

pub use client::{SyncClient, SyncClientOptions, SyncEvent};
pub use relay::{SyncRelay, SyncRelayOptions};
