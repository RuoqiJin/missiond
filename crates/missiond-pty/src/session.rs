//! PTY Session - Interactive terminal session for Claude Code
//!
//! Architecture: portable-pty (process) + alacritty_terminal (emulation) + semantic (detection)
//!
//! - portable-pty: Handles low-level PTY process communication
//! - alacritty_terminal: Parses ANSI sequences, maintains virtual screen
//! - semantic: State detection and confirmation dialog parsing

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write as IoWrite};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config as TermConfig, Term};
use anyhow::{anyhow, Result};

/// Terminal size for creating Term
struct TermSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows + 10_000 // scrollback capacity
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}
use chrono::Utc;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock};
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::extractor::{IncrementalExtractor, StableTextOp, TextAssembler};
use semantic_terminal::{
    default_compiled, maybe_reload_global_patterns, registry_from, ClaudeCodeConfirmParser,
    ClaudeCodeStateParser, ClaudeCodeStatus, ClaudeCodeStatusParser, ClaudeCodeTitle,
    ClaudeCodeToolOutput, ClaudeCodeToolOutputParser, ConfirmInfo as SemanticConfirmInfo,
    ConfirmParser, ParserContext, State as SemanticState, StateParser, StatusParser,
    ToolOutputParser, ToolStatus,
};

use crate::pty_recognition::{
    recognize_screen, CodexCliStateParser, GeminiCliUpstreamStateParser, PtyRecognitionSnapshot,
};

// ========== Types ==========

/// Session state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Starting up
    Starting,
    /// Waiting for input (shows >)
    Idle,
    /// Slash command autocomplete menu is open
    SlashMenu,
    /// Claude is thinking (shows spinner)
    Thinking,
    /// Claude is outputting response
    Responding,
    /// Tool is executing
    ToolRunning,
    /// Waiting for confirmation (Y/n)
    Confirming,
    /// Error state
    Error,
    /// Session has exited
    Exited,
}

impl SessionState {
    /// Check if this is a processing state (Claude is active)
    pub fn is_processing(&self) -> bool {
        matches!(
            self,
            SessionState::Thinking | SessionState::ToolRunning | SessionState::Responding
        )
    }
}

/// Max number of messages to keep in PTY session history.
/// Oldest messages are evicted when this limit is exceeded.
const MAX_HISTORY_MESSAGES: usize = 1000;

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// Source of screen text
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenTextSource {
    Assistant,
    User,
    Tool,
    Ui,
    Unknown,
}

/// Text output event (streaming or complete)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextOutputEvent {
    Stream {
        turn_id: u64,
        seq: u64,
        content: String,
        timestamp: i64,
    },
    Complete {
        turn_id: u64,
        content: String,
        timestamp: i64,
    },
}

/// Screen text event for non-assistant content
#[derive(Debug, Clone, Serialize)]
pub struct ScreenTextEvent {
    pub source: ScreenTextSource,
    pub kind: String,
    pub y: usize,
    pub content: String,
    pub timestamp: i64,
    pub turn_id: Option<u64>,
}

/// Tool information from confirmation dialog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub mcp_server: Option<String>,
    pub params: HashMap<String, serde_json::Value>,
}

/// Confirmation dialog information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmInfo {
    #[serde(rename = "type")]
    pub confirm_type: String,
    pub tool: Option<ToolInfo>,
    pub options: Vec<String>,
    pub selected: usize,
}

/// Permission decision for tool execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Auto-approve the tool
    Allow,
    /// Auto-deny the tool
    Deny,
    /// Require manual confirmation
    Confirm,
}

/// PTY session options
#[derive(Debug, Clone)]
pub struct PTYSessionOptions {
    pub slot_id: String,
    pub cwd: PathBuf,
    pub env: Option<HashMap<String, String>>,
    pub log_file: Option<PathBuf>,
    pub cols: u16,
    pub rows: u16,
    /// CLI engine type (determines which binary to spawn and state parser to use)
    pub engine: missiond_shared::CliEngine,
    /// Path to MCP config JSON file (passed as --mcp-config to claude)
    pub mcp_config: Option<PathBuf>,
    /// Skip all permission prompts and trust dialogs
    pub dangerously_skip_permissions: bool,
    /// Model override (e.g., "sonnet", "opus"). Passed as --model to Claude Code.
    pub model: Option<String>,
}

impl Default for PTYSessionOptions {
    fn default() -> Self {
        Self {
            slot_id: Uuid::new_v4().to_string(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            env: None,
            log_file: None,
            cols: 120,
            rows: 30,
            engine: missiond_shared::CliEngine::default(),
            mcp_config: None,
            dangerously_skip_permissions: false,
            model: None,
        }
    }
}

/// Event listener for alacritty terminal
struct SessionEventListener {
    sender: mpsc::UnboundedSender<TermEvent>,
}

impl EventListener for SessionEventListener {
    fn send_event(&self, event: TermEvent) {
        let _ = self.sender.send(event);
    }
}

// ========== PTYSession ==========

/// Interactive PTY session for AI CLI agents.
///
/// Manages a CLI process (Claude Code, Gemini CLI, or Codex CLI) with
/// terminal emulation, state detection, and streaming text extraction.
pub struct PTYSession {
    /// Unique session ID
    pub id: String,
    /// Slot ID this session belongs to
    pub slot_id: String,
    /// Working directory
    pub cwd: PathBuf,
    /// Terminal dimensions
    pub cols: u16,
    pub rows: u16,
    /// CLI engine type (determines spawn command and state parser)
    pub engine: missiond_shared::CliEngine,

    // Internal state
    state: Arc<RwLock<SessionState>>,
    history: Arc<RwLock<Vec<Message>>>,
    terminal_title: Arc<RwLock<String>>,
    pending_tool_confirm: Arc<RwLock<Option<ConfirmInfo>>>,
    permission_check:
        Arc<RwLock<Option<Box<dyn Fn(&ConfirmInfo) -> PermissionDecision + Send + Sync>>>>,

    // PTY process
    pty_writer: Arc<Mutex<Option<Box<dyn IoWrite + Send>>>>,
    pty_pid: Arc<RwLock<Option<u32>>>,
    running: Arc<AtomicBool>,

    // Terminal emulation
    term: Arc<Mutex<Term<SessionEventListener>>>,

    // Text extraction
    extractor: Arc<Mutex<IncrementalExtractor>>,
    text_assembler: Arc<Mutex<TextAssembler>>,
    current_turn_id: Arc<RwLock<Option<u64>>>,
    stream_seq: Arc<RwLock<u64>>,
    turn_counter: Arc<RwLock<u64>>,
    line_source_by_y: Arc<RwLock<HashMap<usize, ScreenTextSource>>>,
    assistant_block_active: Arc<AtomicBool>,

    // Event channels
    event_tx: broadcast::Sender<SessionEvent>,
    state_change_tx: broadcast::Sender<(SessionState, SessionState)>,
    shutdown_tx: Option<oneshot::Sender<()>>,

    // MCP config
    mcp_config: Option<PathBuf>,

    // Permission bypass
    dangerously_skip_permissions: bool,

    // Model override (--model flag)
    model: Option<String>,

    // Extra environment variables (slot tracking, etc.)
    env: Option<HashMap<String, String>>,

    // Logging
    #[allow(dead_code)]
    log_file: Option<PathBuf>,

    // Raw output replay buffer (for WebSocket late-join)
    raw_output_buffer: Arc<std::sync::Mutex<VecDeque<u8>>>,
    raw_output_max: usize,
}

/// Events emitted by the session
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Raw data from PTY
    Data(Vec<u8>),
    /// State changed
    StateChange {
        new_state: SessionState,
        prev_state: SessionState,
    },
    /// Text output (stream or complete)
    TextOutput(TextOutputEvent),
    /// Screen text (non-assistant)
    ScreenText(ScreenTextEvent),
    /// Confirmation required
    ConfirmRequired {
        prompt: String,
        info: Option<ConfirmInfo>,
    },
    /// Status bar update (spinner + status text)
    StatusUpdate(ClaudeCodeStatus),
    /// Provider-aware PTY recognition update.
    RecognitionUpdate(PtyRecognitionSnapshot),
    /// Tool output parsed
    ToolOutput(ClaudeCodeToolOutput),
    /// Terminal title changed
    TitleChange(ClaudeCodeTitle),
    /// Session exited
    Exit(i32),
}

/// Build CLI launch command based on engine type.
///
/// Each engine has different binary, arguments, and flag support.
/// The working directory is set via CommandBuilder::cwd(), not via CLI flags.
fn build_cli_command(
    engine: missiond_shared::CliEngine,
    cwd: &std::path::Path,
    mcp_config: Option<&std::path::Path>,
    dangerously_skip_permissions: bool,
    model: Option<&str>,
) -> String {
    use missiond_shared::CliEngine;

    match engine {
        CliEngine::ClaudeCode => {
            let mut parts = format!("claude --add-dir \"{}\"", cwd.display());
            if let Some(mcp) = mcp_config {
                parts.push_str(&format!(" --mcp-config \"{}\"", mcp.display()));
                info!(mcp_config = %mcp.display(), "MCP config will be injected");
            }
            if dangerously_skip_permissions {
                parts.push_str(" --dangerously-skip-permissions");
                info!("Dangerous mode: skipping all permission prompts");
            }
            if let Some(m) = model {
                parts.push_str(&format!(" --model {}", m));
                info!(model = %m, "Model override for session");
            }
            parts
        }
        CliEngine::Gemini => {
            // Gemini CLI: interactive mode, --yolo skips tool authorization
            // Working directory is set via CommandBuilder::cwd(), not CLI flag
            let mut parts = "gemini".to_string();
            parts.push_str(" --yolo");
            if let Some(m) = model {
                parts.push_str(&format!(" -m {}", m));
                info!(model = %m, "Gemini CLI: model override");
            }
            if let Some(mcp) = mcp_config {
                info!(mcp_config = %mcp.display(), "MCP config ignored for Gemini CLI (not supported)");
            }
            parts
        }
        CliEngine::Codex => {
            // Codex CLI: interactive mode
            // Working directory is set via CommandBuilder::cwd(), not CLI flag
            let parts = "codex".to_string();
            if let Some(mcp) = mcp_config {
                info!(mcp_config = %mcp.display(), "MCP config ignored for Codex CLI (not supported)");
            }
            parts
        }
    }
}

impl PTYSession {
    /// Create a new PTY session
    pub fn new(options: PTYSessionOptions) -> Result<Self> {
        let id = format!(
            "pty-{}-{}",
            Utc::now().timestamp_millis(),
            &Uuid::new_v4().to_string()[..8]
        );

        // Create terminal event channel
        let (term_event_tx, _term_event_rx) = mpsc::unbounded_channel();
        let event_listener = SessionEventListener {
            sender: term_event_tx,
        };

        // Create virtual terminal
        let term_config = TermConfig::default();
        let term_size = TermSize {
            cols: options.cols as usize,
            rows: options.rows as usize,
        };
        let term = Term::new(term_config, &term_size, event_listener);

        // Create event channels
        let (event_tx, _) = broadcast::channel(1000);
        let (state_change_tx, _) = broadcast::channel(100);

        Ok(Self {
            id,
            slot_id: options.slot_id,
            cwd: options.cwd,
            cols: options.cols,
            rows: options.rows,
            engine: options.engine,

            state: Arc::new(RwLock::new(SessionState::Starting)),
            history: Arc::new(RwLock::new(Vec::new())),
            terminal_title: Arc::new(RwLock::new(String::new())),
            pending_tool_confirm: Arc::new(RwLock::new(None)),
            permission_check: Arc::new(RwLock::new(None)),

            pty_writer: Arc::new(Mutex::new(None)),
            pty_pid: Arc::new(RwLock::new(None)),
            running: Arc::new(AtomicBool::new(false)),

            term: Arc::new(Mutex::new(term)),
            extractor: Arc::new(Mutex::new(IncrementalExtractor::new(
                options.rows as usize,
                None,
            ))),
            text_assembler: Arc::new(Mutex::new(TextAssembler::new())),
            current_turn_id: Arc::new(RwLock::new(None)),
            stream_seq: Arc::new(RwLock::new(0)),
            turn_counter: Arc::new(RwLock::new(0)),
            line_source_by_y: Arc::new(RwLock::new(HashMap::new())),
            assistant_block_active: Arc::new(AtomicBool::new(false)),

            event_tx,
            state_change_tx,
            shutdown_tx: None,
            mcp_config: options.mcp_config,
            dangerously_skip_permissions: options.dangerously_skip_permissions,
            model: options.model,
            env: options.env,
            log_file: options.log_file,

            raw_output_buffer: Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(512 * 1024))),
            raw_output_max: 512 * 1024,
        })
    }

    // ========== Getters ==========

    /// Get current state
    pub async fn state(&self) -> SessionState {
        *self.state.read().await
    }

    /// Get chat history
    pub async fn history(&self) -> Vec<Message> {
        self.history.read().await.clone()
    }

    /// Check if session is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get process ID
    pub async fn pid(&self) -> Option<u32> {
        *self.pty_pid.read().await
    }

    /// Get pending tool confirmation
    pub async fn pending_tool_confirm(&self) -> Option<ConfirmInfo> {
        self.pending_tool_confirm.read().await.clone()
    }

    /// Get terminal title
    pub async fn terminal_title(&self) -> String {
        self.terminal_title.read().await.clone()
    }

    // ========== Screen Reading ==========

    /// Capture terminal screenshot as PNG, return file path
    pub async fn screenshot(
        &self,
        output_dir: &std::path::Path,
    ) -> anyhow::Result<std::path::PathBuf> {
        let captured = {
            let term = self.term.lock().await;
            super::screenshot::capture_grid(&*term)
        };
        super::screenshot::save_screenshot(&captured, output_dir, &self.slot_id)
    }

    /// Get current screen text
    pub async fn get_screen_text(&self) -> String {
        let term = self.term.lock().await;
        let grid = term.grid();
        let mut lines = Vec::new();

        let rows = grid.screen_lines();

        // Line(0) = top of visible screen, Line(rows-1) = bottom
        for y in 0..rows {
            let Ok(line_idx) = i32::try_from(y) else {
                break;
            };
            let line = alacritty_terminal::index::Line(line_idx);
            let row = &grid[line];
            let text: String = row.into_iter().map(|cell| cell.c).collect();
            lines.push(text.trim_end().to_string());
        }

        lines.join("\n")
    }

    /// Get raw output replay buffer for late-joining WebSocket clients
    pub fn get_replay_buffer(&self) -> Vec<u8> {
        let buf = self
            .raw_output_buffer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        buf.iter().copied().collect()
    }

    /// Get last N lines (visible + scrollback)
    pub async fn get_last_lines(&self, n: usize) -> Vec<String> {
        let term = self.term.lock().await;
        let grid = term.grid();
        let mut lines = Vec::new();

        let screen_lines = grid.screen_lines();
        let history = grid.history_size();
        let available = screen_lines + history;
        let n = n.min(available);

        // How many scrollback lines we need above visible area
        let scroll_needed = n.saturating_sub(screen_lines).min(history);
        let visible_start = if n > screen_lines {
            0
        } else {
            screen_lines - n
        };

        // Read scrollback lines (oldest first: Line(-scroll_needed) .. Line(-1))
        for i in (1..=scroll_needed).rev() {
            let line = alacritty_terminal::index::Line(-(i as i32));
            let row = &grid[line];
            let text: String = row.into_iter().map(|cell| cell.c).collect();
            lines.push(text.trim_end().to_string());
        }

        // Read visible lines
        for y in visible_start..screen_lines {
            let Ok(line_idx) = i32::try_from(y) else {
                break;
            };
            let line = alacritty_terminal::index::Line(line_idx);
            let row = &grid[line];
            let text: String = row.into_iter().map(|cell| cell.c).collect();
            lines.push(text.trim_end().to_string());
        }

        lines
    }

    // ========== Lifecycle ==========

    /// Start the PTY session
    pub async fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Err(anyhow!("Session already started"));
        }

        info!(slot_id = %self.slot_id, cwd = %self.cwd.display(), "Starting PTY session");

        // Create PTY
        let pty_system = native_pty_system();
        let pty_pair = pty_system.openpty(PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Build CLI command based on engine type
        let cli_cmd = build_cli_command(
            self.engine,
            &self.cwd,
            self.mcp_config.as_deref(),
            self.dangerously_skip_permissions,
            self.model.as_deref(),
        );

        #[cfg(unix)]
        let mut cmd = {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
            info!(shell = %shell, engine = %self.engine, cwd = %self.cwd.display(), "Spawning CLI via login shell");

            let mut c = CommandBuilder::new(&shell);
            c.args([
                "-l", // login shell (loads .zprofile, .zshrc)
                "-i", // interactive (needed for proper PTY behavior)
                "-c", &cli_cmd,
            ]);
            c
        };

        #[cfg(windows)]
        let mut cmd = {
            let mut c = CommandBuilder::new("cmd.exe");
            c.args(["/C", &cli_cmd]);
            c
        };

        cmd.cwd(&self.cwd);

        // Inherit ALL environment variables from parent process.
        // portable-pty's CommandBuilder starts with an empty env,
        // so we must explicitly copy everything.
        for (key, value) in std::env::vars() {
            cmd.env(key, value);
        }
        // Ensure TERM is set for proper terminal behavior
        cmd.env("TERM", "xterm-256color");
        // Remove CLAUDECODE env var so nested claude sessions can start
        cmd.env_remove("CLAUDECODE");
        // Inject extra environment variables (slot tracking, etc.)
        if let Some(ref extra) = self.env {
            for (key, value) in extra {
                cmd.env(key, value);
            }
        }

        // #5: Pre-spawn credential permission check
        #[cfg(unix)]
        {
            let home = std::env::var("HOME").unwrap_or_default();
            if !home.is_empty() {
                let cred_path = std::path::Path::new(&home)
                    .join(".claude")
                    .join(".credentials.json");
                if cred_path.exists() {
                    // Check if credentials file is readable by current process
                    match std::fs::File::open(&cred_path) {
                        Ok(_) => {}
                        Err(e) => {
                            warn!(
                                slot_id = %self.slot_id,
                                path = %cred_path.display(),
                                error = %e,
                                "Cannot read credentials file — Claude Code may fail to authenticate"
                            );
                        }
                    }
                } else {
                    warn!(
                        slot_id = %self.slot_id,
                        path = %cred_path.display(),
                        "Credentials file not found — Claude Code may require interactive login"
                    );
                }
            }
        }

        // Spawn child process
        let child = pty_pair.slave.spawn_command(cmd)?;
        let pid = child.process_id().unwrap_or(0);
        *self.pty_pid.write().await = Some(pid);
        info!(pid = pid, "PTY spawned");

        // Get writer
        let writer = pty_pair.master.take_writer()?;
        *self.pty_writer.lock().await = Some(writer);

        // Get reader
        let reader = pty_pair.master.try_clone_reader()?;

        self.running.store(true, Ordering::SeqCst);

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Create channel for feeding PTY output to terminal emulator
        let (term_feed_tx, term_feed_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        // Spawn terminal feed task (bridges blocking read → async term mutex)
        let term_for_feed = Arc::clone(&self.term);
        tokio::spawn(Self::term_feed_loop(term_for_feed, term_feed_rx));

        // Spawn read task
        let term = Arc::clone(&self.term);
        let event_tx = self.event_tx.clone();
        let running = Arc::clone(&self.running);
        let replay_buf = Arc::clone(&self.raw_output_buffer);
        let replay_max = self.raw_output_max;

        tokio::spawn(async move {
            Self::read_loop(
                reader,
                term,
                event_tx,
                running,
                shutdown_rx,
                term_feed_tx,
                replay_buf,
                replay_max,
            )
            .await;
        });

        // Spawn state check task
        let session_state = Arc::clone(&self.state);
        let term_for_check = Arc::clone(&self.term);
        let extractor = Arc::clone(&self.extractor);
        let text_assembler = Arc::clone(&self.text_assembler);
        let current_turn = Arc::clone(&self.current_turn_id);
        let stream_seq = Arc::clone(&self.stream_seq);
        let turn_counter = Arc::clone(&self.turn_counter);
        let line_source = Arc::clone(&self.line_source_by_y);
        let assistant_active = Arc::clone(&self.assistant_block_active);
        let state_change_tx = self.state_change_tx.clone();
        let event_tx_for_check = self.event_tx.clone();
        let running_for_check = Arc::clone(&self.running);
        let pending_confirm = Arc::clone(&self.pending_tool_confirm);
        let permission_check = Arc::clone(&self.permission_check);
        let pty_writer = Arc::clone(&self.pty_writer);

        let slot_id_for_check = self.slot_id.clone();
        let engine_for_check = self.engine;
        tokio::spawn(async move {
            Self::state_check_loop(
                slot_id_for_check,
                engine_for_check,
                session_state,
                term_for_check,
                extractor,
                text_assembler,
                current_turn,
                stream_seq,
                turn_counter,
                line_source,
                assistant_active,
                state_change_tx,
                event_tx_for_check,
                running_for_check,
                pending_confirm,
                permission_check,
                pty_writer,
            )
            .await;
        });

        // Wait for child exit in background using async try_wait polling.
        // Previous approach used spawn_blocking(child.wait()) which blocked a
        // tokio thread pool thread, preventing runtime shutdown (UE state).
        let event_tx_for_exit = self.event_tx.clone();
        let running_for_exit = Arc::clone(&self.running);
        let state_for_exit = Arc::clone(&self.state);

        tokio::spawn(async move {
            let mut child = child;
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            let exit_code = loop {
                interval.tick().await;
                match child.try_wait() {
                    Ok(Some(status)) => break status.exit_code() as i32,
                    Err(_) => break -1,
                    Ok(None) => {} // still running
                }
            };

            running_for_exit.store(false, Ordering::SeqCst);
            *state_for_exit.write().await = SessionState::Exited;

            let _ = event_tx_for_exit.send(SessionEvent::Exit(exit_code));
            info!(exit_code = exit_code, "PTY exited");
        });

        // NOTE: We no longer block here waiting for Idle state.
        // The caller (PTYManager::spawn) decides whether to wait or return immediately.

        Ok(())
    }

    /// Feed loop - receives data from read_loop and feeds it into the terminal emulator
    async fn term_feed_loop(
        term: Arc<Mutex<Term<SessionEventListener>>>,
        mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
    ) {
        use alacritty_terminal::vte::ansi::Processor;
        let mut processor: Processor = Processor::new();

        while let Some(data) = rx.recv().await {
            let mut term_guard = term.lock().await;
            processor.advance(&mut *term_guard, &data);
        }
    }

    /// Read loop - reads from PTY and feeds to terminal
    async fn read_loop(
        reader: Box<dyn Read + Send>,
        _term: Arc<Mutex<Term<SessionEventListener>>>,
        event_tx: broadcast::Sender<SessionEvent>,
        running: Arc<AtomicBool>,
        _shutdown_rx: oneshot::Receiver<()>,
        term_feed_tx: mpsc::UnboundedSender<Vec<u8>>,
        replay_buf: Arc<std::sync::Mutex<VecDeque<u8>>>,
        replay_max: usize,
    ) {
        // Move reader into a thread that will do blocking reads
        let running_clone = Arc::clone(&running);

        tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];

            while running_clone.load(Ordering::SeqCst) {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        // Append to replay buffer for late-joining WS clients
                        if let Ok(mut rb) = replay_buf.lock() {
                            rb.extend(&data);
                            if rb.len() > replay_max {
                                let drain = rb.len() - replay_max;
                                rb.drain(..drain);
                            }
                        }
                        // Feed to terminal emulator (for state detection + screen)
                        let _ = term_feed_tx.send(data.clone());
                        // Broadcast to WebSocket clients
                        let _ = event_tx.send(SessionEvent::Data(data));
                    }
                    Err(e) => {
                        error!(error = %e, "PTY read error");
                        break;
                    }
                }
            }
        });
    }

    /// State check loop - periodically checks terminal state
    #[allow(clippy::too_many_arguments)]
    async fn state_check_loop(
        slot_id: String,
        engine: missiond_shared::CliEngine,
        state: Arc<RwLock<SessionState>>,
        term: Arc<Mutex<Term<SessionEventListener>>>,
        extractor: Arc<Mutex<IncrementalExtractor>>,
        text_assembler: Arc<Mutex<TextAssembler>>,
        current_turn_id: Arc<RwLock<Option<u64>>>,
        stream_seq: Arc<RwLock<u64>>,
        turn_counter: Arc<RwLock<u64>>,
        line_source_by_y: Arc<RwLock<HashMap<usize, ScreenTextSource>>>,
        assistant_block_active: Arc<AtomicBool>,
        state_change_tx: broadcast::Sender<(SessionState, SessionState)>,
        event_tx: broadcast::Sender<SessionEvent>,
        running: Arc<AtomicBool>,
        pending_tool_confirm: Arc<RwLock<Option<ConfirmInfo>>>,
        permission_check: Arc<
            RwLock<Option<Box<dyn Fn(&ConfirmInfo) -> PermissionDecision + Send + Sync>>>,
        >,
        pty_writer: Arc<Mutex<Option<Box<dyn IoWrite + Send>>>>,
    ) {
        let mut check_interval = interval(Duration::from_millis(100));

        // Create parsers based on engine type, sharing one Arc<CompiledPatterns> snapshot.
        // Patterns are loaded from external YAML with periodic hot-reload.
        use missiond_shared::CliEngine;

        // Get initial compiled patterns for this engine
        let compiled_patterns = Arc::new(
            default_compiled(engine).unwrap_or_else(|| {
                warn!(slot = %slot_id, ?engine, "No compiled patterns for engine, using ClaudeCode fallback");
                default_compiled(CliEngine::ClaudeCode).expect("embedded claude-code patterns must parse")
            }),
        );

        let state_parser: Box<dyn StateParser + Send + Sync> = match engine {
            CliEngine::ClaudeCode => Box::new(ClaudeCodeStateParser::with_patterns(
                compiled_patterns.clone(),
            )),
            CliEngine::Gemini => Box::new(GeminiCliUpstreamStateParser::new()),
            CliEngine::Codex => Box::new(CodexCliStateParser::new()),
        };
        let confirm_parser: Option<Box<dyn ConfirmParser + Send + Sync>> = match engine {
            CliEngine::ClaudeCode => Some(Box::new(ClaudeCodeConfirmParser::with_patterns(
                compiled_patterns.clone(),
            ))),
            CliEngine::Gemini | CliEngine::Codex => None,
        };
        let status_parser = ClaudeCodeStatusParser::with_patterns(compiled_patterns.clone());
        let tool_parser = ClaudeCodeToolOutputParser::with_patterns(compiled_patterns.clone());
        let fingerprint_registry = registry_from(&compiled_patterns);

        // Hot-reload counter: check every 100 ticks (10 seconds at 100ms interval)
        let mut reload_tick: u64 = 0;
        const RELOAD_INTERVAL: u64 = 100;

        // Counter for consecutive empty-screen detections while in a processing state.
        // When screen is empty, detect_state returns None and state gets stuck.
        // After enough consecutive empty checks, fall back to Idle.
        let mut empty_screen_count: u32 = 0;
        const EMPTY_SCREEN_IDLE_THRESHOLD: u32 = 30; // 30 * 100ms = 3 seconds
        let mut diag_tick: u32 = 0; // diagnostic log counter

        // Debounce for processing sub-state transitions (Thinking↔ToolRunning).
        // The ⏺ tool line flickers in alacritty's virtual terminal, causing
        // rapid alternation at 100ms level. Require N consecutive ticks of the
        // same new state before committing the transition.
        let mut debounce_target: Option<SessionState> = None;
        let mut debounce_count: u32 = 0;
        const DEBOUNCE_THRESHOLD: u32 = 3; // 3 * 100ms = 300ms

        let mut heartbeat_tick: u64 = 0;
        let mut starting_since: Option<std::time::Instant> = Some(std::time::Instant::now());
        let mut starting_warned = false;

        // ToolOutput deduplication: only emit when tool name or status changes.
        // Without this, every 100ms tick where a tool header is visible emits
        // a new ToolOutput(Running), flooding the Jarvis SSE stream.
        let mut last_tool_name: Option<String> = None;
        let mut last_tool_status: Option<ToolStatus> = None;
        let mut last_recognition: Option<PtyRecognitionSnapshot> = None;
        // Block-scoped context classifier: tracks whether we're inside a tool
        // output block or assistant text block, so Unknown lines inherit context.
        let mut block_classifier = BlockClassifier::new();
        while running.load(Ordering::SeqCst) {
            check_interval.tick().await;
            heartbeat_tick += 1;
            reload_tick += 1;

            // Periodic hot-reload check for pattern files (every 10s)
            if reload_tick >= RELOAD_INTERVAL {
                reload_tick = 0;
                if maybe_reload_global_patterns() {
                    info!(slot = %slot_id, "PTY patterns hot-reloaded from disk");
                    // Note: current session parsers keep their Arc<CompiledPatterns> snapshot.
                    // New sessions will pick up the reloaded patterns.
                }
            }

            // Extract frame delta
            let delta = {
                let term_guard = term.lock().await;
                let mut extractor_guard = extractor.lock().await;
                extractor_guard.extract(&*term_guard)
            };

            // Get screen text for state detection (read ALL visible lines)
            let (last_lines, is_alt_screen) = {
                let term_guard = term.lock().await;
                let is_alt = term_guard
                    .mode()
                    .contains(alacritty_terminal::term::TermMode::ALT_SCREEN);
                let grid = term_guard.grid();
                let mut lines = Vec::new();
                let rows = grid.screen_lines();
                // Read visible area only: Line(0) to Line(screen_lines - 1)
                for y in 0..rows {
                    let Ok(line_idx) = i32::try_from(y) else {
                        break;
                    };
                    let line = alacritty_terminal::index::Line(line_idx);
                    let row = &grid[line];
                    // Skip wide-char spacer cells (CJK/emoji second cells)
                    let text: String = row
                        .into_iter()
                        .filter(|cell| {
                            !cell
                                .flags
                                .contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER)
                        })
                        .map(|cell| cell.c)
                        .collect();
                    lines.push(text.trim_end().to_string());
                }
                (lines, is_alt)
            };

            // Create ParserContext with current state
            let current_state = *state.read().await;
            let context = ParserContext::new(last_lines.clone())
                .with_state(current_state_to_semantic(current_state));

            let recognition = recognize_screen(engine, &last_lines, current_state);
            if last_recognition.as_ref() != Some(&recognition) {
                let _ = event_tx.send(SessionEvent::RecognitionUpdate(recognition.clone()));
                last_recognition = Some(recognition);
            }

            // Use FingerprintRegistry for quick hints
            let hints = fingerprint_registry.extract(&context).hints;

            // Detect state using semantic StateParser
            let detected_result = state_parser.detect_state(&context);
            let detected_state = detected_result
                .as_ref()
                .map(|r| semantic_state_to_session_state(r.state));

            // Periodic heartbeat (every 5s) — log detected state and screen sample
            if heartbeat_tick % 50 == 0 {
                let non_empty: Vec<_> = last_lines
                    .iter()
                    .filter(|l| !l.trim().is_empty())
                    .take(5)
                    .map(|s| {
                        let t: String = s.chars().take(80).collect();
                        t
                    })
                    .collect();
                // #4: Promote to INFO when Starting (aids remote debugging)
                if current_state == SessionState::Starting {
                    info!(
                        slot = %slot_id,
                        tick = heartbeat_tick,
                        current = ?current_state,
                        detected = ?detected_state,
                        screen_sample = ?non_empty,
                        "state_check_loop heartbeat (Starting)"
                    );
                } else {
                    debug!(
                        slot = %slot_id,
                        tick = heartbeat_tick,
                        current = ?current_state,
                        detected = ?detected_state,
                        screen_sample = ?non_empty,
                        "state_check_loop heartbeat"
                    );
                }
            }

            // #3: Starting state timeout — progressive warnings
            if current_state == SessionState::Starting {
                if let Some(since) = starting_since {
                    let elapsed = since.elapsed();
                    if !starting_warned && elapsed.as_secs() >= 30 {
                        let screen_snapshot: Vec<_> = last_lines
                            .iter()
                            .filter(|l| !l.trim().is_empty())
                            .take(5)
                            .map(|s| s.chars().take(80).collect::<String>())
                            .collect();
                        warn!(
                            slot = %slot_id,
                            elapsed_secs = elapsed.as_secs(),
                            screen_snapshot = ?screen_snapshot,
                            "PTY stuck in Starting state for >30s — check screen output"
                        );
                        starting_warned = true;
                    }
                }
            }

            // Diagnostic logging (once per second = every 10 ticks at 100ms)
            diag_tick += 1;
            if diag_tick % 10 == 0
                && !matches!(current_state, SessionState::Idle | SessionState::Exited)
            {
                let non_empty_count = last_lines.iter().filter(|l| !l.trim().is_empty()).count();
                // Show last_non_empty_lines (what state detection actually uses)
                let active = context.last_non_empty_lines(5);
                let active_sample = active
                    .iter()
                    .map(|s| {
                        let truncated: String = s.chars().take(60).collect();
                        if truncated.len() < s.len() {
                            format!("{}...", truncated)
                        } else {
                            s.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                // Extract spinner status line for diagnostics
                let spinner_line = active
                    .iter()
                    .find(|l| l.trim().starts_with(|c: char| "·✻✽✶✳✢*".contains(c)))
                    .map(|s| {
                        let t: String = s.chars().take(80).collect();
                        t
                    })
                    .unwrap_or_default();
                debug!(
                    state = ?current_state,
                    detected = ?detected_state,
                    alt_screen = is_alt_screen,
                    non_empty = non_empty_count,
                    spinner = %spinner_line,
                    active = %active_sample,
                    "PTY state diag"
                );
            }

            // Process stable ops for text streaming BEFORE state transitions,
            // so text_assembler is populated when Complete fires.
            if !delta.stable_ops.is_empty() && current_state.is_processing() {
                let turn_id = *current_turn_id.read().await;
                for op in &delta.stable_ops {
                    // Use stateful block classifier: Unknown lines inherit current block
                    let source = block_classifier.classify(op);
                    let text_preview: String = op.text().chars().take(60).collect();
                    debug!(
                        slot = %slot_id,
                        kind = op.kind(),
                        y = op.y(),
                        source = ?source,
                        text = %text_preview,
                        "stable_op during processing"
                    );
                    // Only accumulate Assistant text (not Tool/Unknown/UI)
                    if source == ScreenTextSource::Assistant {
                        if let Some(turn_id) = turn_id {
                            let chunk = text_assembler.lock().await.apply(op);
                            if !chunk.is_empty() {
                                let seq = {
                                    let mut seq_guard = stream_seq.write().await;
                                    let s = *seq_guard;
                                    *seq_guard += 1;
                                    s
                                };
                                let _ = event_tx.send(SessionEvent::TextOutput(
                                    TextOutputEvent::Stream {
                                        turn_id,
                                        seq,
                                        content: chunk,
                                        timestamp: Utc::now().timestamp_millis(),
                                    },
                                ));
                            }
                        }
                    }
                }
            }

            // Handle state transitions
            if let Some(new_state) = detected_state {
                // Check for trust confirmation during startup (auto-confirm)
                if let Some(ref result) = detected_result {
                    if let Some(ref meta) = result.meta {
                        if meta.needs_trust_confirm == Some(true) {
                            debug!("Auto-confirming trust dialog");
                            if let Some(writer) = pty_writer.lock().await.as_mut() {
                                let _ = writer.write_all(b"\r");
                            }
                            continue;
                        }
                    }
                }

                if new_state == current_state {
                    // State is stable — reset any pending debounce
                    debounce_target = None;
                    debounce_count = 0;
                } else {
                    // Debounce state transitions that are prone to flickering:
                    // 1. Thinking↔ToolRunning: ⏺ tool line flickers in alacritty
                    // 2. Processing→Idle: spinner briefly disappears between tool calls,
                    //    causing premature turn end while prompt ❯ is always visible
                    let needs_debounce = matches!(
                        (current_state, new_state),
                        (SessionState::Thinking, SessionState::ToolRunning)
                            | (SessionState::ToolRunning, SessionState::Thinking)
                    ) || (current_state.is_processing()
                        && !new_state.is_processing());

                    if needs_debounce {
                        if debounce_target == Some(new_state) {
                            debounce_count += 1;
                        } else {
                            debounce_target = Some(new_state);
                            debounce_count = 1;
                        }
                        if debounce_count < DEBOUNCE_THRESHOLD {
                            // Not yet stable — keep current state, don't transition
                            empty_screen_count = 0;
                            continue;
                        }
                        // Threshold met — commit transition
                        debounce_target = None;
                        debounce_count = 0;
                    } else {
                        debounce_target = None;
                        debounce_count = 0;
                    }
                }

                if new_state != current_state {
                    // #3: Track Starting state entry/exit for timeout warning
                    if new_state == SessionState::Starting {
                        starting_since = Some(std::time::Instant::now());
                        starting_warned = false;
                    } else if current_state == SessionState::Starting {
                        starting_since = None;
                        starting_warned = false;
                    }

                    // Diagnostic: dump active screen on state transition
                    let active = context.last_non_empty_lines(8);
                    let active_dump = active
                        .iter()
                        .map(|s| {
                            let truncated: String = s.chars().take(80).collect();
                            if truncated.len() < s.len() {
                                format!("{}...", truncated)
                            } else {
                                s.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    info!(
                        slot = %slot_id,
                        from = ?current_state,
                        to = ?new_state,
                        active = %active_dump,
                        "PTY state transition"
                    );

                    // Begin turn when entering processing state
                    if new_state.is_processing() && !current_state.is_processing() {
                        let mut counter = turn_counter.write().await;
                        *counter += 1;
                        let new_turn_id = *counter;
                        *current_turn_id.write().await = Some(new_turn_id);
                        *stream_seq.write().await = 0;
                        text_assembler.lock().await.reset();
                        block_classifier.reset();
                        line_source_by_y.write().await.clear();
                        assistant_block_active.store(false, Ordering::SeqCst);
                        debug!(slot = %slot_id, turn_id = new_turn_id, "Begin turn");

                        // Retroactively process stable_ops from this frame.
                        // On the Idle→Processing transition frame, the ops processing
                        // above was skipped (current_state was still Idle). Now that
                        // the turn is set up, process any assistant output that appeared
                        // on this same frame as the state transition.
                        if !delta.stable_ops.is_empty() {
                            for op in &delta.stable_ops {
                                let source = block_classifier.classify(op);
                                if source == ScreenTextSource::Assistant {
                                    let chunk = text_assembler.lock().await.apply(op);
                                    if !chunk.is_empty() {
                                        let seq = {
                                            let mut seq_guard = stream_seq.write().await;
                                            let s = *seq_guard;
                                            *seq_guard += 1;
                                            s
                                        };
                                        let _ = event_tx.send(SessionEvent::TextOutput(
                                            TextOutputEvent::Stream {
                                                turn_id: new_turn_id,
                                                seq,
                                                content: chunk,
                                                timestamp: Utc::now().timestamp_millis(),
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    *state.write().await = new_state;

                    // End turn when leaving processing state
                    if current_state.is_processing() && !new_state.is_processing() {
                        if let Some(turn_id) = *current_turn_id.read().await {
                            let content = text_assembler.lock().await.finalize();
                            info!(
                                slot = %slot_id,
                                turn_id = turn_id,
                                content_len = content.len(),
                                "End turn — emitting Complete"
                            );
                            let _ = event_tx.send(SessionEvent::TextOutput(
                                TextOutputEvent::Complete {
                                    turn_id,
                                    content,
                                    timestamp: Utc::now().timestamp_millis(),
                                },
                            ));
                        }
                        *current_turn_id.write().await = None;
                        *stream_seq.write().await = 0;
                        text_assembler.lock().await.reset();
                        block_classifier.reset();
                        line_source_by_y.write().await.clear();
                        assistant_block_active.store(false, Ordering::SeqCst);
                    }

                    // Handle confirming state using semantic ConfirmParser
                    if new_state == SessionState::Confirming {
                        let semantic_confirm = confirm_parser
                            .as_ref()
                            .and_then(|p| p.detect_confirm(&context));
                        let confirm_info =
                            semantic_confirm.as_ref().map(convert_semantic_confirm_info);
                        *pending_tool_confirm.write().await = confirm_info.clone();

                        // Check permission if callback is set
                        if let Some(ref info) = confirm_info {
                            let permission = permission_check.read().await;
                            if let Some(ref check_fn) = *permission {
                                let decision = check_fn(info);
                                match decision {
                                    PermissionDecision::Allow => {
                                        // Auto-approve
                                        if let Some(writer) = pty_writer.lock().await.as_mut() {
                                            let _ = writer.write_all(b"\r");
                                        }
                                        continue;
                                    }
                                    PermissionDecision::Deny => {
                                        // Auto-deny (down, down, enter)
                                        if let Some(writer) = pty_writer.lock().await.as_mut() {
                                            let _ = writer.write_all(b"\x1b[B\x1b[B\r");
                                        }
                                        continue;
                                    }
                                    PermissionDecision::Confirm => {
                                        // Require manual confirmation
                                    }
                                }
                            }
                        }

                        let _ = event_tx.send(SessionEvent::ConfirmRequired {
                            prompt: last_lines.join("\n"),
                            info: confirm_info,
                        });
                    }

                    let _ = state_change_tx.send((new_state, current_state));
                    let _ = event_tx.send(SessionEvent::StateChange {
                        new_state,
                        prev_state: current_state,
                    });
                }

                // Reset empty screen counter when we get a valid detection
                empty_screen_count = 0;
            } else if current_state.is_processing() {
                // detect_state returned None while in a processing state.
                // This happens when Claude Code's TUI clears the screen between phases.
                // "Nearly empty" = only bottom bar (⏵⏵ bypass) and separators (────) remain,
                // or truly all lines empty. Either way, no meaningful content to detect.
                let non_empty = last_lines.iter().filter(|l| !l.trim().is_empty()).count();
                let screen_is_nearly_empty = non_empty <= 3
                    && last_lines.iter().all(|l| {
                        let t = l.trim();
                        t.is_empty() || t.starts_with('─') || t.starts_with("⏵")
                    });
                if screen_is_nearly_empty {
                    empty_screen_count += 1;
                    if empty_screen_count >= EMPTY_SCREEN_IDLE_THRESHOLD {
                        debug!(
                            prev_state = ?current_state,
                            empty_ticks = empty_screen_count,
                            "Empty screen fallback: transitioning to Idle"
                        );
                        let new_state = SessionState::Idle;
                        *state.write().await = new_state;

                        // End turn
                        if let Some(turn_id) = *current_turn_id.read().await {
                            let content = text_assembler.lock().await.finalize();
                            let _ = event_tx.send(SessionEvent::TextOutput(
                                TextOutputEvent::Complete {
                                    turn_id,
                                    content,
                                    timestamp: Utc::now().timestamp_millis(),
                                },
                            ));
                            debug!(turn_id = turn_id, "End turn (empty screen fallback)");
                        }
                        *current_turn_id.write().await = None;
                        *stream_seq.write().await = 0;
                        text_assembler.lock().await.reset();
                        line_source_by_y.write().await.clear();
                        assistant_block_active.store(false, Ordering::SeqCst);

                        let _ = state_change_tx.send((new_state, current_state));
                        let _ = event_tx.send(SessionEvent::StateChange {
                            new_state,
                            prev_state: current_state,
                        });
                        empty_screen_count = 0;
                    }
                } else {
                    empty_screen_count = 0;
                }
            } else {
                empty_screen_count = 0;
            }

            // Emit StatusUpdate event if spinner is detected
            if hints.has_spinner {
                if let Some(status) = status_parser.parse(&context) {
                    let _ = event_tx.send(SessionEvent::StatusUpdate(status));
                }
            }

            // Emit ToolOutput event if tool output is detected (with deduplication)
            if hints.has_tool_output {
                if let Some(result) = tool_parser.parse(&context) {
                    let name = &result.data.tool_name;
                    let status = result.data.status;
                    let is_duplicate =
                        last_tool_name.as_deref() == Some(name) && last_tool_status == Some(status);
                    if !is_duplicate {
                        last_tool_name = Some(name.clone());
                        last_tool_status = Some(status);
                        let _ = event_tx.send(SessionEvent::ToolOutput(result.data));
                    }
                }
            } else {
                // Tool header disappeared from screen — reset dedup state
                last_tool_name = None;
                last_tool_status = None;
            }
        }
    }

    /// Write data to PTY
    pub async fn write(&self, data: &str) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(anyhow!("Session not running"));
        }

        let mut writer_guard = self.pty_writer.lock().await;
        if let Some(ref mut writer) = *writer_guard {
            writer.write_all(data.as_bytes())?;
            writer.flush()?;
            debug!(data_len = data.len(), "Wrote to PTY");
            Ok(())
        } else {
            Err(anyhow!("PTY writer not available"))
        }
    }

    /// Poll screen after paste to confirm the CLI has received the pasted text.
    /// Claude Code shows "[Pasted text #N +M lines]" for multi-line pastes.
    /// Gemini CLI (Ink TUI) handles bracketed paste natively — short settle only.
    /// Falls back after 10s timeout.
    async fn wait_for_paste_confirmation(&self, pre_paste_prompt: &str) {
        let slot_id = &self.slot_id;

        // Gemini/Codex: Ink TUI handles bracketed paste natively, no polling needed.
        // Just a brief settle for the TUI to process the input.
        if self.engine != missiond_shared::CliEngine::ClaudeCode {
            tokio::time::sleep(Duration::from_millis(300)).await;
            tracing::debug!(slot = %slot_id, engine = %self.engine, "Paste settle (non-Claude CLI)");
            return;
        }

        // Claude Code: poll for paste confirmation
        tokio::time::sleep(Duration::from_millis(500)).await;

        for attempt in 0..38 {
            // Poll every 250ms, up to ~10s total (500ms initial + 38*250ms = ~10s)
            let screen = self.get_screen_text().await;

            // Check 1: Claude Code shows "[Pasted text #N" for multi-line pastes
            if screen.contains("[Pasted text") {
                tracing::debug!(slot = %slot_id, attempt, "Paste confirmed: [Pasted text] detected");
                tokio::time::sleep(Duration::from_millis(200)).await;
                return;
            }

            // Check 2: the prompt line changed from pre-paste snapshot (single-line paste)
            // Find the last prompt line (starts with ❯ or >)
            let current_prompt = screen
                .lines()
                .rev()
                .find(|l| {
                    let trimmed = l.trim();
                    trimmed.starts_with('❯') || trimmed.starts_with('>')
                })
                .unwrap_or("")
                .trim();
            if !current_prompt.is_empty() && current_prompt != pre_paste_prompt {
                tracing::debug!(
                    slot = %slot_id, attempt,
                    before = %pre_paste_prompt,
                    after = %current_prompt,
                    "Paste confirmed: prompt line changed"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
                return;
            }

            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        tracing::warn!(slot = %slot_id, "Paste confirmation timed out after 10s, sending Enter anyway");
    }

    /// Capture the current prompt line content (for pre-paste snapshot).
    async fn capture_prompt_line(&self) -> String {
        let screen = self.get_screen_text().await;
        screen
            .lines()
            .rev()
            .find(|l| {
                let trimmed = l.trim();
                trimmed.starts_with('❯') || trimmed.starts_with('>')
            })
            .unwrap_or("")
            .trim()
            .to_string()
    }

    /// Send message (fire-and-forget): paste + enter, return immediately
    pub async fn send_fire_and_forget(&self, message: &str) -> Result<()> {
        let prev_state = self.state().await;
        if prev_state != SessionState::Idle {
            return Err(anyhow!("Cannot send message in state: {:?}", prev_state));
        }

        // Record user message
        {
            let mut history = self.history.write().await;
            history.push(Message {
                role: MessageRole::User,
                content: message.trim().to_string(),
                timestamp: Utc::now().timestamp_millis(),
            });
            if history.len() > MAX_HISTORY_MESSAGES {
                let drain = history.len() - MAX_HISTORY_MESSAGES;
                history.drain(..drain);
            }
        }

        // Set session state to Thinking and begin a new turn.
        // Must set turn_id here because state_check_loop's begin-turn detection
        // (new_state.is_processing() && !current_state.is_processing()) won't fire
        // when current_state is already Thinking (which is_processing() = true).
        {
            *self.state.write().await = SessionState::Thinking;

            // Begin turn
            let mut counter = self.turn_counter.write().await;
            *counter += 1;
            *self.current_turn_id.write().await = Some(*counter);
            *self.stream_seq.write().await = 0;
            self.text_assembler.lock().await.reset();
            self.line_source_by_y.write().await.clear();
            self.assistant_block_active.store(false, Ordering::SeqCst);

            let _ = self
                .state_change_tx
                .send((SessionState::Thinking, prev_state));
            let _ = self.event_tx.send(SessionEvent::StateChange {
                new_state: SessionState::Thinking,
                prev_state,
            });
        }

        // Snapshot prompt line before paste for change detection
        let pre_paste_prompt = self.capture_prompt_line().await;
        // Send message using bracketed paste mode
        let paste_payload = format!("\x1b[200~{}\x1b[201~", message);
        self.write(&paste_payload).await?;
        // Poll screen to confirm paste was received before sending Enter.
        self.wait_for_paste_confirmation(&pre_paste_prompt).await;
        self.write("\r").await?;

        Ok(())
    }

    /// Send message and wait for response.
    ///
    /// Design: subscribe to event channel BEFORE sending, then wait for Complete.
    /// Do NOT manually set state — let state_check_loop handle all transitions
    /// naturally from screen detection.
    pub async fn send(&self, message: &str, timeout_ms: u64) -> Result<String> {
        let state = self.state().await;
        if state != SessionState::Idle {
            return Err(anyhow!("Cannot send message in state: {:?}", state));
        }

        // Record user message
        {
            let mut history = self.history.write().await;
            history.push(Message {
                role: MessageRole::User,
                content: message.trim().to_string(),
                timestamp: Utc::now().timestamp_millis(),
            });
            if history.len() > MAX_HISTORY_MESSAGES {
                let drain = history.len() - MAX_HISTORY_MESSAGES;
                history.drain(..drain);
            }
        }

        // Subscribe to events BEFORE sending so we never miss the Complete event.
        let mut rx = self.event_tx.subscribe();

        // Snapshot prompt line before paste for change detection
        let pre_paste_prompt = self.capture_prompt_line().await;
        // Send message using bracketed paste mode so multi-line text is treated as one paste.
        // Write paste markers + content in one call to avoid fragmentation.
        let paste_payload = format!("\x1b[200~{}\x1b[201~", message);
        self.write(&paste_payload).await?;
        // Poll screen to confirm paste was received before sending Enter.
        self.wait_for_paste_confirmation(&pre_paste_prompt).await;
        self.write("\r").await?;

        // Wait for TextOutputEvent::Complete — emitted by state_check_loop when
        // processing state transitions back to Idle (turn end).
        // state_check_loop detects Idle→Thinking (begin turn) and Thinking/etc→Idle (end turn)
        // purely from screen content, so no manual state manipulation needed.
        let timeout_duration = Duration::from_millis(timeout_ms);
        let slot_id = self.slot_id.clone();
        let result = timeout(timeout_duration, async {
            let mut event_count = 0u64;
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        event_count += 1;
                        if let SessionEvent::TextOutput(TextOutputEvent::Complete {
                            content, ..
                        }) = event
                        {
                            info!(
                                slot = %slot_id,
                                content_len = content.len(),
                                events_processed = event_count,
                                "send() received Complete"
                            );
                            return Ok(content);
                        }
                        if let SessionEvent::Exit(code) = event {
                            return Err(anyhow!("Session exited with code: {}", code));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(
                            slot = %slot_id,
                            skipped = n,
                            events_processed = event_count,
                            "send() broadcast lagged — events skipped"
                        );
                        // Continue receiving from new position
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(anyhow!("Event channel closed"));
                    }
                }
            }
        })
        .await;

        match result {
            Ok(Ok(response)) => {
                // Provider-aware completion fallback for Gemini / Ink-style CLIs.
                //
                // Live-smoke from BoardTask 42b2385e showed that for slot-gemini-ultra
                // the screen frame after the turn ended contained the worker's final
                // closeout (`  Fix:` / `  Verification:` indented prose), but the
                // `TextOutput::Complete` event content (assembled by streaming) had
                // already truncated at `Autopilot Smoke Test: …`. Because that event
                // is what `send()` returns and what callers feed to Autopilot summary
                // extraction / `mission_pty_read(history)`, the Board note was
                // missing the closeout that was clearly on screen.
                //
                // For non-Claude CLIs we settle briefly, capture the last screen
                // frame, sanitize TUI chrome (Ink rounded boxes, status footer,
                // input echo) enough to recover the final assistant prose, and
                // prefer the screen-derived text only when it carries the closeout
                // pair the streamed event is missing. ClaudeCode keeps the streamed
                // event verbatim (`maybe_enrich_completion` short-circuits) so its
                // Summary-block extraction is unchanged.
                let enriched = if self.engine != missiond_shared::CliEngine::ClaudeCode {
                    let chosen = self.enrich_completion_from_settled_screen(response).await;
                    debug!(
                        slot = %self.slot_id,
                        engine = %self.engine,
                        chosen_len = chosen.len(),
                        "send() applied provider-aware completion fallback"
                    );
                    chosen
                } else {
                    response
                };

                // Record assistant message
                {
                    let mut history = self.history.write().await;
                    history.push(Message {
                        role: MessageRole::Assistant,
                        content: enriched.clone(),
                        timestamp: Utc::now().timestamp_millis(),
                    });
                    if history.len() > MAX_HISTORY_MESSAGES {
                        let drain = history.len() - MAX_HISTORY_MESSAGES;
                        history.drain(..drain);
                    }
                }
                Ok(enriched)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(anyhow!("Timeout waiting for response")),
        }
    }

    async fn enrich_completion_from_settled_screen(&self, response: String) -> String {
        if has_fix_verification_closeout(&response) {
            return focus_fix_verification_closeout(&response)
                .unwrap_or(response.as_str())
                .to_string();
        }

        let buffer_lines = (self.rows as usize).saturating_mul(2).max(80);
        let started = std::time::Instant::now();
        let response_was_progress = looks_like_active_tui_progress(&response);
        let max_wait = if response_was_progress {
            Duration::from_millis(45_000)
        } else {
            Duration::from_millis(2_000)
        };
        let mut delay_ms = 250;

        loop {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let screen_text = self.get_last_lines(buffer_lines).await.join("\n");
            let chosen = maybe_enrich_completion(self.engine, response.clone(), &screen_text);
            if chosen != response {
                return chosen;
            }
            if !looks_like_active_tui_progress(&screen_text)
                && started.elapsed() >= Duration::from_millis(2_000)
            {
                return response;
            }
            if started.elapsed() >= max_wait {
                return response;
            }
            delay_ms = 150;
        }
    }

    /// Send confirmation response
    pub async fn confirm(&self, response: ConfirmResponse) -> Result<()> {
        let state = self.state().await;
        if state != SessionState::Confirming {
            warn!(state = ?state, "Not in confirming state");
            return Ok(());
        }

        // Claude Code's tool-use confirm dialog accepts numeric selection
        // ("1"/"2"/"3" then Enter), but the digit and Enter must arrive as
        // two distinct PTY events with a small human-like gap — sending them
        // in a single write batches them together and the TUI's input handler
        // discards or mis-matches the chunk. Verified empirically 2026-04-12:
        // sequential write with ~80ms gap works, single write does not.
        match response {
            ConfirmResponse::Yes => self.write("\r").await,
            ConfirmResponse::No => self.press_digit_then_enter('3').await,
            ConfirmResponse::Option(n) => {
                let digit = std::char::from_digit(n as u32, 10).unwrap_or('1');
                self.press_digit_then_enter(digit).await
            }
        }
    }

    /// Send a digit followed by Enter as two separate writes with a brief
    /// inter-key delay. Simulates a human pressing the keys.
    async fn press_digit_then_enter(&self, digit: char) -> Result<()> {
        let mut buf = [0u8; 4];
        let s = digit.encode_utf8(&mut buf);
        self.write(s).await?;
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        self.write("\r").await
    }

    /// Send interrupt (Ctrl+C)
    pub async fn interrupt(&self) -> Result<()> {
        self.write("\x03").await
    }

    /// Set permission check callback
    pub async fn set_permission_check<F>(&self, callback: F)
    where
        F: Fn(&ConfirmInfo) -> PermissionDecision + Send + Sync + 'static,
    {
        *self.permission_check.write().await = Some(Box::new(callback));
    }

    /// Subscribe to session events
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.event_tx.subscribe()
    }

    /// Subscribe to state changes
    pub fn subscribe_state_changes(&self) -> broadcast::Receiver<(SessionState, SessionState)> {
        self.state_change_tx.subscribe()
    }

    /// Wait for specific state
    pub async fn wait_for_state(
        &self,
        target: SessionState,
        timeout_duration: Duration,
    ) -> Result<()> {
        let current = self.state().await;
        if current == target {
            return Ok(());
        }

        let mut rx = self.state_change_tx.subscribe();

        timeout(timeout_duration, async {
            loop {
                if let Ok((new_state, _)) = rx.recv().await {
                    if new_state == target {
                        return Ok(());
                    }
                    if matches!(new_state, SessionState::Error | SessionState::Exited) {
                        return Err(anyhow!(
                            "Session entered {:?} while waiting for {:?}",
                            new_state,
                            target
                        ));
                    }
                }
            }
        })
        .await
        .map_err(|_| anyhow!("Timeout waiting for state: {:?}", target))?
    }

    /// Close session gracefully
    pub async fn close(&mut self) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        info!("Closing PTY session");

        // Try graceful exit
        let _ = self.write("/exit\r").await;

        // Wait for exit or timeout
        let timeout_result = timeout(Duration::from_secs(3), async {
            let mut rx = self.event_tx.subscribe();
            loop {
                if let Ok(SessionEvent::Exit(_)) = rx.recv().await {
                    break;
                }
            }
        })
        .await;

        if timeout_result.is_err() {
            // Force kill
            self.kill().await;
        }

        Ok(())
    }

    /// Force kill session
    pub async fn kill(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // SIGKILL the entire process group to kill child AND any grandchildren
        // (e.g. npm, tsc, rg spawned by Claude Code). Grandchildren holding the
        // PTY slave FD would prevent reader.read() from getting EOF, deadlocking
        // the read_loop's spawn_blocking thread.
        let pid_opt = *self.pty_pid.read().await;
        if let Some(pid) = pid_opt {
            #[cfg(unix)]
            {
                // kill(-pid) sends SIGKILL to the entire process group
                unsafe {
                    libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
                }
                info!(pid, "PTY process group SIGKILL sent");
            }
            #[cfg(not(unix))]
            {
                info!(pid, "PTY child kill (non-unix, relying on writer drop)");
            }
        }

        *self.pty_writer.lock().await = None;
        info!("PTY session killed");
    }
}

/// Confirmation response types
pub enum ConfirmResponse {
    Yes,
    No,
    Option(usize),
}

// ========== Helper Functions ==========

/// Convert semantic State to SessionState
fn semantic_state_to_session_state(state: SemanticState) -> SessionState {
    match state {
        SemanticState::Starting => SessionState::Starting,
        SemanticState::Idle => SessionState::Idle,
        SemanticState::SlashMenu => SessionState::SlashMenu,
        SemanticState::Thinking => SessionState::Thinking,
        SemanticState::Responding => SessionState::Responding,
        SemanticState::ToolRunning => SessionState::ToolRunning,
        SemanticState::Confirming => SessionState::Confirming,
        SemanticState::Error => SessionState::Error,
    }
}

/// Convert SessionState to semantic State
fn current_state_to_semantic(state: SessionState) -> SemanticState {
    match state {
        SessionState::Starting => SemanticState::Starting,
        SessionState::Idle => SemanticState::Idle,
        SessionState::SlashMenu => SemanticState::SlashMenu,
        SessionState::Thinking => SemanticState::Thinking,
        SessionState::Responding => SemanticState::Responding,
        SessionState::ToolRunning => SemanticState::ToolRunning,
        SessionState::Confirming => SemanticState::Confirming,
        SessionState::Error => SemanticState::Error,
        SessionState::Exited => SemanticState::Idle, // No direct mapping
    }
}

/// Convert semantic ConfirmInfo to session ConfirmInfo
fn convert_semantic_confirm_info(info: &SemanticConfirmInfo) -> ConfirmInfo {
    let options: Vec<String> = info
        .options
        .as_ref()
        .map(|opts| opts.iter().map(|o| o.label.clone()).collect())
        .unwrap_or_default();

    let tool = info.tool.as_ref().map(|t| ToolInfo {
        name: t.name.clone(),
        mcp_server: t.mcp_server.clone(),
        params: t
            .params
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect(),
    });

    ConfirmInfo {
        confirm_type: format!("{:?}", info.confirm_type).to_lowercase(),
        tool,
        options,
        selected: 0, // Default to first option selected
    }
}

/// Classify stable op source
/// Block-Scoped Context Classifier: stateful classifier that tracks which
/// content block (Assistant text vs Tool output) we're currently inside.
/// Unknown lines inherit the current block state instead of leaking into
/// assistant text.
struct BlockClassifier {
    /// Current block context — Unknown lines inherit this classification
    current_block: ScreenTextSource,
}

impl BlockClassifier {
    fn new() -> Self {
        Self {
            current_block: ScreenTextSource::Assistant,
        }
    }

    /// Reset classifier state (call at turn boundaries)
    fn reset(&mut self) {
        self.current_block = ScreenTextSource::Assistant;
    }

    /// Classify a stable text op with block-scoped context awareness.
    /// Strong markers trigger block transitions; weak/unknown lines inherit
    /// the current block state.
    fn classify(&mut self, op: &StableTextOp) -> ScreenTextSource {
        let text = op.text();
        let trimmed = text.trim_start();

        // Empty lines keep current block
        if trimmed.is_empty() {
            return self.current_block;
        }

        // ── Strong markers: trigger block transitions ──

        // Prompt line = user input
        if trimmed.starts_with('>') || trimmed.starts_with('❯') {
            self.current_block = ScreenTextSource::User;
            return ScreenTextSource::User;
        }

        // Tool output border markers (always Tool, regardless of block)
        if trimmed.starts_with('⎿') || trimmed.starts_with('│') {
            self.current_block = ScreenTextSource::Tool;
            return ScreenTextSource::Tool;
        }

        // ⏺ marker — the key block boundary signal
        if trimmed.starts_with('⏺') || trimmed.starts_with('●') {
            if trimmed.contains('(') && !trimmed.contains("completed") {
                // Tool call header: ⏺ Read(path) or ⏺ missiond - kb_search (MCP)
                self.current_block = ScreenTextSource::Tool;
                return ScreenTextSource::Tool;
            }
            // Assistant text block: ⏺ followed by prose
            self.current_block = ScreenTextSource::Assistant;
            return ScreenTextSource::Assistant;
        }

        // UI elements — status bar, shortcuts, permission toggles, system notices
        if trimmed.contains("ctrl+")
            || trimmed.contains("Ctrl+")
            || trimmed.contains("shift+tab")
            || trimmed.contains("IDE disconnected")
            || trimmed.starts_with("⏵⏵")
            || trimmed.starts_with("✢")
            // Claude Code system notices (npm migration, auto-update, etc.)
            || trimmed.contains("switched from npm to native installer")
            || (trimmed.starts_with("Pasting") && trimmed.len() < 20)
        {
            return ScreenTextSource::Ui;
        }

        // Box drawing = UI
        if trimmed.chars().any(|c| {
            matches!(
                c,
                '╭' | '╮' | '╯' | '╰' | '┌' | '┐' | '└' | '┘' | '─' | '━' | '═'
            )
        }) {
            return ScreenTextSource::Ui;
        }

        // ── No strong marker: inherit current block state ──
        // This is the key insight: lines without prefixes (JSON fragments,
        // file paths, data rows) belong to whatever block we're currently in.
        // If we're InTool, they're tool output. If InAssistant, they're text.
        self.current_block
    }
}

// ========== Provider-aware completion fallback ==========
//
// Gemini / Codex CLIs render their conversation in an Ink-style TUI: tool
// calls and the input prompt sit inside rounded `╭ │ ╰` boxes, and the
// assistant's prose is interleaved between those boxes. The streamed
// `TextOutputEvent::Complete` content can truncate before the worker's final
// closeout (`Fix:` / `Verification:`) when the screen flickers idle as the
// last paragraph is still being painted, but the screen frame captured a
// moment later still carries the full closeout. The helpers below sanitize
// chrome out of that screen frame and decide whether the screen-derived text
// is richer than the streamed event content.

/// Apply the provider-aware enrichment rule used by `PTYSession::send`.
///
/// For ClaudeCode this is the identity (the streamed event content is already
/// authoritative because Claude Code does not hide the final answer behind
/// repaint boxes). For non-Claude engines, sanitize the raw screen frame and
/// promote it only when the streamed event lacks the `Fix:` / `Verification:`
/// closeout that the screen carries.
pub(crate) fn maybe_enrich_completion(
    engine: missiond_shared::CliEngine,
    event_content: String,
    screen_text: &str,
) -> String {
    if engine == missiond_shared::CliEngine::ClaudeCode {
        return event_content;
    }
    let sanitized = sanitize_tui_chrome(screen_text);
    choose_richer_completion(event_content, sanitized)
}

/// Strip Ink-style TUI chrome from a screen frame so the assistant prose
/// (including indented `  Fix:` / `  Verification:` lines) is preserved.
///
/// Drops:
/// - Any line whose first non-space char is a box-drawing glyph (`╭ ╮ ╰ ╯`,
///   `│ ─ ━ ═ ║`, `┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼`). Gemini wraps tool panels and the
///   input prompt in rounded boxes; assistant prose is never boxed.
/// - The Gemini status footer (path + sandbox + model + context badge),
///   recognised by the `gemini-` model id co-occurring with `context left`.
/// - The bare user-input echo (`> ` line) the prompt box leaks on partial
///   redraws when the box border has already scrolled away.
///
/// Runs of blank lines are collapsed to one so the resulting blob is easy to
/// scan for closeout anchors.
pub(crate) fn sanitize_tui_chrome(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_blank = false;
    for raw_line in raw.lines() {
        let line = raw_line.trim_end();
        if is_tui_chrome_line(line) {
            continue;
        }
        if line.trim().is_empty() {
            if prev_blank {
                continue;
            }
            prev_blank = true;
        } else {
            prev_blank = false;
        }
        let normalized = normalize_tui_prose_line(line);
        out.push_str(normalized.as_ref());
        out.push('\n');
    }
    out
}

fn normalize_tui_prose_line(line: &str) -> Cow<'_, str> {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("✦ ") {
        if rest.starts_with("Fix:") || rest.starts_with("Verification:") {
            let prefix_len = line.len() - t.len();
            return Cow::Owned(format!("{}{}", &line[..prefix_len], rest));
        }
    }
    Cow::Borrowed(line)
}

fn is_tui_chrome_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.is_empty() {
        return false;
    }
    if let Some(c) = t.chars().next() {
        if matches!(
            c,
            '╭' | '╮'
                | '╯'
                | '╰'
                | '│'
                | '─'
                | '━'
                | '═'
                | '║'
                | '┌'
                | '┐'
                | '└'
                | '┘'
                | '├'
                | '┤'
                | '┬'
                | '┴'
                | '┼'
        ) {
            return true;
        }
    }
    if t.contains("gemini-") && t.contains("context left") {
        return true;
    }
    if t.starts_with("? for shortcuts") || is_tui_progress_line(t) {
        return true;
    }
    if t.starts_with("YOLO Ctrl+Y") || (t.contains("GEMINI.md file") && t.contains("skills")) {
        return true;
    }
    if t.starts_with("*   Type your message") || t.starts_with("* Type your message") {
        return true;
    }
    if t.starts_with("workspace (/directory)") {
        return true;
    }
    if t.starts_with("~/")
        && t.contains("no sandbox")
        && (t.contains("Gemini") || t.contains("gemini-") || t.contains("Auto ("))
    {
        return true;
    }
    if t == ">" || t.starts_with("> ") {
        return true;
    }
    false
}

/// Pick between the streamed event content and the sanitized screen frame.
///
/// If the streamed event already carries a `Fix:` / `Verification:` closeout
/// pair, keep that authoritative source but focus it on the closeout region so
/// PTY history does not retain prompt echo or provider-added diagnostic tails.
/// Otherwise promote the sanitized screen text only when it carries a closeout
/// the event missed. If neither source carries the pair, keep the streamed event
/// unchanged.
pub(crate) fn choose_richer_completion(event_content: String, sanitized_screen: String) -> String {
    let event_has = has_fix_verification_closeout(&event_content);
    let screen_has = has_fix_verification_closeout(&sanitized_screen);
    if event_has {
        focus_fix_verification_closeout(&event_content)
            .unwrap_or(event_content.as_str())
            .to_string()
    } else if screen_has {
        focus_fix_verification_closeout(&sanitized_screen)
            .unwrap_or(sanitized_screen.as_str())
            .to_string()
    } else {
        event_content
    }
}

/// Mirror of `autopilot::find_fix_verification_anchor`'s qualification rule:
/// the text contains a `Fix:` line (start of line, optionally with `**`
/// markdown emphasis or whitespace before it) followed somewhere later by
/// the literal `Verification:`. Matching the autopilot rule keeps the
/// promotion decision aligned with the downstream summary-extraction anchor.
fn has_fix_verification_closeout(text: &str) -> bool {
    focus_fix_verification_closeout(text).is_some()
}

fn focus_fix_verification_closeout(text: &str) -> Option<&str> {
    find_fix_verification_anchor(text).map(|idx| trim_board_summary_tail(&text[idx..]))
}

fn find_fix_verification_anchor(text: &str) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut search_start = 0;
    while let Some(rel) = text[search_start..].find("Fix:") {
        let abs = search_start + rel;
        let line_start = text[..abs].rfind('\n').map(|nl| nl + 1).unwrap_or(0);
        let leading = &text[line_start..abs];
        let leading_trimmed = leading.trim();
        let line_start_ok =
            leading_trimmed.is_empty() || leading_trimmed == "**" || leading_trimmed == "✦";
        if line_start_ok && text[abs + "Fix:".len()..].contains("Verification:") {
            best = Some(line_start);
        }
        search_start = abs + "Fix:".len();
    }
    best
}

fn trim_board_summary_tail(text: &str) -> &str {
    let mut seen_verification = false;
    let mut separator_start: Option<usize> = None;
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let line_start = offset;
        let trimmed = line.trim();

        if !seen_verification {
            if trimmed.contains("Verification:") {
                seen_verification = true;
            }
            offset += line.len();
            continue;
        }

        if trimmed.is_empty() {
            offset += line.len();
            continue;
        }

        if trimmed == "---" {
            separator_start.get_or_insert(line_start);
            offset += line.len();
            continue;
        }

        if is_board_summary_heading(trimmed) {
            let cut = separator_start.unwrap_or(line_start);
            return text[..cut].trim_end();
        }

        separator_start = None;
        offset += line.len();
    }

    text.trim_end()
}

fn is_board_summary_heading(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains("任务诊断摘要") || compact.to_ascii_lowercase().contains("boardtasksummary")
}

fn looks_like_active_tui_progress(text: &str) -> bool {
    text.lines()
        .any(|line| is_tui_progress_line(line.trim_start()))
}

fn is_tui_progress_line(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    matches!(
        first,
        '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'
    ) && trimmed.contains("esc to cancel")
}

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_shared::CliEngine;

    /// Live shape from BoardTask 42b2385e: the streamed `TextOutput::Complete`
    /// content for slot-gemini-ultra truncated at `Autopilot Smoke Test: …`,
    /// while the screen frame still carried the worker's final indented
    /// `  Fix:` / `  Verification:` closeout. The enrichment must promote the
    /// sanitized screen text so the value returned from `send()` (and pushed
    /// into history) carries the closeout, and chrome lines (rounded boxes,
    /// status footer, prompt echo) must not leak through.
    #[test]
    fn enriches_truncated_gemini_event_with_screen_closeout() {
        let event_content =
            "Investigating BoardTask 42b2385e — running cargo test ...\n\nAutopilot Smoke Test: "
                .to_string();
        let screen = "\
 ╭───────────────────────────────────────────────────────────╮
 │ ✔ ReadFile crates/missiond-pty/src/session.rs             │
 │   1: //! PTY Session                                      │
 │   ...                                                     │
 ╰───────────────────────────────────────────────────────────╯
✦ I traced the truncation to the streamed Complete event.

  Autopilot Smoke Test: passed.

  Fix: Capture the post-Idle screen frame and promote it when the streamed
       event content is missing the closeout.
  Verification: cargo test -p missiond-pty passes; the live re-run on
                slot-gemini-ultra shows Fix/Verification in the BoardTask note.

 ╭───────────────────────────────────────────────────────────╮
 │ > _                                                       │
 ╰───────────────────────────────────────────────────────────╯
~/Projects/missiond (main*)  no sandbox (see /docs)  gemini-2.5-pro (95% context left)
";
        let result = maybe_enrich_completion(CliEngine::Gemini, event_content.clone(), screen);

        assert!(
            result.contains("Fix: Capture the post-Idle screen frame"),
            "expected enriched result to carry the Fix: closeout, got: {result}"
        );
        assert!(
            result.contains("Verification: cargo test -p missiond-pty passes"),
            "expected enriched result to carry the Verification: closeout, got: {result}"
        );
        assert!(
            !result.contains('╭') && !result.contains('╰') && !result.contains('│'),
            "chrome box characters must be stripped, got: {result}"
        );
        assert!(
            !result.contains("Investigating BoardTask 42b2385e"),
            "promoted screen result should focus on closeout only, got: {result}"
        );
        assert!(
            !result.contains("gemini-2.5-pro"),
            "Gemini status footer must be stripped, got: {result}"
        );
        assert!(
            !result.contains("> _"),
            "input prompt echo must be stripped, got: {result}"
        );
    }

    /// Live shape from BoardTask 9aeb14b6: Gemini's final answer can start the
    /// closeout line with its assistant bullet marker (`✦ Fix:`). The
    /// sanitizer must normalize that line to plain `Fix:` and the enrichment
    /// poll must still treat it as the closeout pair.
    #[test]
    fn enriches_gemini_bullet_fix_closeout_from_screen() {
        let event_content = "只读冒烟测试验证: 执行只读冒烟测试，验证部署后的状态。".to_string();
        let screen = "\
  只读冒烟测试验证: 执行只读冒烟测试，验证部署后的状态。

╭────────────────────────────────────────────────────────────────────╮
│ ✓  Shell git status --short && git rev-parse --short HEAD          │
│  M packages/board/src/App.tsx                                      │
│ 03fe34ac                                                           │
╰────────────────────────────────────────────────────────────────────╯

✦ Fix: This was a read-only smoke of MissionD Autopilot/PTY completion capture.
  Verification: Current commit is 03fe34ac and only pre-existing packages/board/src/App.tsx is dirty.

 YOLO Ctrl+Y                                                                               1 GEMINI.md file · 12 skills
 *   Type your message or @path/to/file
 workspace (/directory)               branch              sandbox                  /model                         quota
~/Projects/missiond main no sandbox Auto (Gemini 3) 3% used
";
        let result = maybe_enrich_completion(CliEngine::Gemini, event_content, screen);

        assert!(
            result.contains(
                "Fix: This was a read-only smoke of MissionD Autopilot/PTY completion capture."
            ),
            "expected Gemini bullet closeout to normalize to Fix:, got: {result}"
        );
        assert!(
            !result.contains("✦ Fix:"),
            "assistant bullet marker should not remain on the closeout line: {result}"
        );
        assert!(
            result.contains("Verification: Current commit is 03fe34ac"),
            "expected Verification closeout, got: {result}"
        );
        assert!(
            !result.contains("YOLO Ctrl+Y")
                && !result.contains("Type your message")
                && !result.contains("workspace (/directory)")
                && !result.contains("Auto (Gemini 3)"),
            "Gemini footer lines must be stripped, got: {result}"
        );
        assert!(
            !result.contains("执行只读冒烟测试"),
            "promoted screen result should drop earlier Gemini status prose: {result}"
        );
    }

    /// ClaudeCode pinning: the streamed event already carries the worker's
    /// full final answer (Claude Code does not repaint final prose behind
    /// boxes), so enrichment must be a strict no-op for it. This guards
    /// against the Gemini fallback bleeding into Claude Code's existing
    /// summary-block extraction path.
    #[test]
    fn claude_code_completion_unchanged_by_enrichment() {
        let event =
            "⏺ Summary\n\nAll acceptance gates pass.\nFix: x\nVerification: y\n".to_string();
        // Even with a screen blob that contains chrome and a different closeout,
        // ClaudeCode must short-circuit and return the streamed event byte-for-byte.
        let screen = "\
 ╭──────╮
 │ tool │
 ╰──────╯
Fix: DIFFERENT
Verification: DIFFERENT
";
        let result = maybe_enrich_completion(CliEngine::ClaudeCode, event.clone(), screen);
        assert_eq!(result, event);
    }

    /// When both the streamed event and the sanitized screen carry the
    /// closeout pair, the streamed event wins (it is the authoritative
    /// per-turn assembly) but is still focused to the closeout block so PTY
    /// history does not retain provider prelude text.
    #[test]
    fn prefers_event_when_event_already_has_closeout() {
        let event = "Done.\n\nFix: streamed-fix\nVerification: streamed-verification\n".to_string();
        let screen = "\
 ╭──────╮
 │ tool │
 ╰──────╯
Fix: screen-fix
Verification: screen-verification
";
        let result = maybe_enrich_completion(CliEngine::Gemini, event.clone(), screen);
        assert_eq!(
            result,
            "Fix: streamed-fix\nVerification: streamed-verification"
        );
    }

    /// Live shape from BoardTask 1600de56: Gemini obeyed the requested
    /// Fix/Verification closeout and then appended the generic BoardTask
    /// diagnostic summary from the task suffix. The PTY history should retain
    /// only the closeout pair so downstream BoardTask notes do not double-log
    /// the same completion in two formats.
    #[test]
    fn trims_gemini_board_summary_tail_after_closeout() {
        let event = "\
Fix: read-only smoke of MissionD Autopilot/PTY completion capture
  Verification: current commit is 182c0f7f and only pre-existing packages/board/src/App.tsx is dirty.

  ---
  任 务 诊 断 摘 要  (Board Task Summary)
  已 完 成 对 MissionD Autopilot/PTY 完 成 捕 获 的 最 终 冒 烟 检 查 。
"
        .to_string();
        let result = maybe_enrich_completion(CliEngine::Gemini, event, "");
        assert_eq!(
            result,
            "Fix: read-only smoke of MissionD Autopilot/PTY completion capture\n  Verification: current commit is 182c0f7f and only pre-existing packages/board/src/App.tsx is dirty."
        );
        assert!(!result.contains("Board Task Summary"));
        assert!(!result.contains("任 务 诊 断 摘 要"));
    }

    /// Live shape from BoardTask 531f3cd0: a premature completion frame can be
    /// made entirely of Gemini spinner/status lines while the CLI is still
    /// producing the real answer. These progress lines must be recognized as
    /// TUI chrome and as an active frame so callers do not persist them as the
    /// assistant response.
    #[test]
    fn gemini_progress_lines_are_active_tui_chrome() {
        let progress = "\
 ⠋ Thinking... (esc to cancel, 0s)                                                                      ? for shortcuts
 ⠸ Defining the Scope (esc to cancel, 10s)                                                              ? for shortcuts
 ⠴ Confirming the Closeout (esc to cancel, 12s)                                                         ? for shortcuts
";
        assert!(looks_like_active_tui_progress(progress));
        let cleaned = sanitize_tui_chrome(progress);
        assert!(
            cleaned.trim().is_empty(),
            "Gemini spinner/status lines should be stripped, got: {cleaned:?}"
        );
    }

    /// When neither the event nor the screen carry the closeout pair, the
    /// streamed event is kept (no enrichment is justified).
    #[test]
    fn keeps_event_when_neither_has_closeout() {
        let event = "Investigating ...".to_string();
        let screen = "\
 ╭──────╮
 │ tool │
 ╰──────╯
Some chatter without a closeout.
";
        let result = maybe_enrich_completion(CliEngine::Gemini, event.clone(), screen);
        assert_eq!(result, event);
    }

    /// Sanitizer keeps indented closeout prose intact while dropping every
    /// flavour of Ink chrome (rounded corners, vertical bars, the model /
    /// context-left status footer, and the bare prompt echo).
    #[test]
    fn sanitize_tui_chrome_keeps_indented_closeout_and_drops_chrome() {
        let raw = "\
 ╭───╮
 │ x │
 ╰───╯
Some prose.
  Fix: indented fix line
  Verification: indented verification line
>
~/foo  gemini-2.5-pro (90% context left)
";
        let cleaned = sanitize_tui_chrome(raw);
        assert!(cleaned.contains("Some prose."));
        assert!(cleaned.contains("  Fix: indented fix line"));
        assert!(cleaned.contains("  Verification: indented verification line"));
        assert!(!cleaned.contains('╭'));
        assert!(!cleaned.contains('│'));
        assert!(!cleaned.contains('╰'));
        assert!(!cleaned.contains("gemini-2.5-pro"));
        assert!(!cleaned.contains("> \n"));
    }

    /// The closeout detector must require both a `Fix:` line and a later
    /// `Verification:` token, mirroring the autopilot anchor rule so the
    /// promotion decision lines up with the downstream summary extractor.
    #[test]
    fn closeout_detector_requires_both_fix_and_verification() {
        assert!(has_fix_verification_closeout("Fix: a\nVerification: b\n"));
        assert!(has_fix_verification_closeout(
            "  Fix: indented\n  Verification: also indented\n"
        ));
        assert!(has_fix_verification_closeout(
            "**Fix: bold-emphasis\nVerification: ok\n"
        ));
        assert!(has_fix_verification_closeout(
            "✦ Fix: gemini bullet\n  Verification: ok\n"
        ));
        assert!(!has_fix_verification_closeout("Fix: lonely\n"));
        assert!(!has_fix_verification_closeout("Verification: b\nFix: a\n"));
        // `Fix:` must be at the start of a line (not embedded mid-prose).
        assert!(!has_fix_verification_closeout(
            "We need to Fix: something. Verification: ok\n"
        ));
    }
}
