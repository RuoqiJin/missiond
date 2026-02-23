//! WebSocket module for PTY streaming and Claude Code Tasks events
//!
//! Allows external clients to:
//! 1. Attach to PTY sessions like `docker attach` or `tmux attach`
//! 2. Subscribe to Claude Code Tasks change events

mod screenshot_broker;
mod server;

pub use screenshot_broker::{ScreenshotBroker, ScreenshotResult};
pub use server::{PTYWebSocketServer, WSServerOptions};
