//! missiond-runner - Claude CLI wrapper
//!
//! Executes `claude --print --output-format stream-json` and parses the output.
//!
//! # Example
//!
//! ```no_run
//! use missiond_runner::{ClaudeRunner, RunOptions};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let runner = ClaudeRunner::new();
//!
//!     let result = runner.run(RunOptions {
//!         prompt: "Say hello".to_string(),
//!         cwd: Some("/path/to/project".into()),
//!         on_progress: Some(Box::new(|event| {
//!             println!("Event: {:?}", event);
//!         })),
//!         ..Default::default()
//!     }).await?;
//!
//!     println!("Result: {}", result.result);
//!     Ok(())
//! }
//! ```

mod runner;
mod types;

pub use runner::ClaudeRunner;
pub use types::*;
