//! missiond-attach - PTY attach CLI
//!
//! Attach to a running PTY session like `docker attach` or `tmux attach`
//!
//! Usage:
//!   missiond-attach <slot-id>
//!   missiond-attach slot-secret-1 --host localhost --port 9120

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::io::{stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_PORT: u16 = 9120;

#[derive(Parser, Debug)]
#[command(name = "missiond-attach")]
#[command(about = "Attach to a Mission Control PTY session")]
#[command(version)]
struct Args {
    /// The slot ID to attach to (e.g., slot-secret-1)
    slot_id: String,

    /// WebSocket server host
    #[arg(short = 'H', long, default_value = DEFAULT_HOST)]
    host: String,

    /// WebSocket server port
    #[arg(short, long, default_value_t = DEFAULT_PORT)]
    port: u16,
}

/// Messages from PTY server
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PtyMessage {
    Screen { data: Option<String> },
    Data { data: Option<String> },
    State {
        state: String,
        #[serde(default, alias = "prevState")]
        prev_state: Option<String>,
    },
    Exit { code: Option<i32> },
}

/// Messages to PTY server
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
    Input { data: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Build WebSocket URL
    let url = format!("ws://{}:{}/pty/{}", args.host, args.port, args.slot_id);

    eprintln!("\x1b[90mConnecting to {}...\x1b[0m", url);

    // Connect to WebSocket
    let (ws_stream, _) = connect_async(&url)
        .await
        .context("Failed to connect to WebSocket server")?;

    eprintln!("\x1b[32mAttached to PTY session: {}\x1b[0m", args.slot_id);
    eprintln!("\x1b[90mPress Ctrl+C to detach (session continues running)\x1b[0m");
    eprintln!();

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Track connection state
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    // Channel for keyboard input
    let (input_tx, mut input_rx) = mpsc::channel::<String>(32);

    // Spawn keyboard input handler
    let running_input = running.clone();
    let input_handle = tokio::task::spawn_blocking(move || {
        if let Err(e) = enable_raw_mode() {
            eprintln!("\x1b[31mFailed to enable raw mode: {}\x1b[0m", e);
            return;
        }

        while running_input.load(Ordering::SeqCst) {
            // Poll for events with timeout
            if event::poll(std::time::Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key_event)) = event::read() {
                    // Ctrl+C to detach
                    if key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && key_event.code == KeyCode::Char('c')
                    {
                        eprintln!("\n\x1b[33mDetaching from PTY session (session continues running)\x1b[0m");
                        running_input.store(false, Ordering::SeqCst);
                        break;
                    }

                    // Convert key to string
                    let data = match key_event.code {
                        KeyCode::Char(c) => {
                            if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                // Convert Ctrl+X to control character
                                let ctrl_char = (c as u8 - b'a' + 1) as char;
                                ctrl_char.to_string()
                            } else {
                                c.to_string()
                            }
                        }
                        KeyCode::Enter => "\r".to_string(),
                        KeyCode::Backspace => "\x7f".to_string(),
                        KeyCode::Tab => "\t".to_string(),
                        KeyCode::Esc => "\x1b".to_string(),
                        KeyCode::Up => "\x1b[A".to_string(),
                        KeyCode::Down => "\x1b[B".to_string(),
                        KeyCode::Right => "\x1b[C".to_string(),
                        KeyCode::Left => "\x1b[D".to_string(),
                        KeyCode::Home => "\x1b[H".to_string(),
                        KeyCode::End => "\x1b[F".to_string(),
                        KeyCode::PageUp => "\x1b[5~".to_string(),
                        KeyCode::PageDown => "\x1b[6~".to_string(),
                        KeyCode::Delete => "\x1b[3~".to_string(),
                        KeyCode::Insert => "\x1b[2~".to_string(),
                        KeyCode::F(n) => format!("\x1b[{}~", n + 10),
                        _ => continue,
                    };

                    if input_tx.blocking_send(data).is_err() {
                        break;
                    }
                }
            }
        }

        let _ = disable_raw_mode();
    });

    // Main event loop
    let result = tokio::select! {
        // Handle WebSocket messages
        result = async {
            while let Some(msg) = ws_rx.next().await {
                match msg {
                    Ok(WsMessage::Text(text)) => {
                        match serde_json::from_str::<PtyMessage>(&text) {
                            Ok(pty_msg) => match pty_msg {
                                PtyMessage::Screen { data } => {
                                    if let Some(content) = data {
                                        // Clear screen and show content
                                        print!("\x1b[2J\x1b[H{}", content);
                                        stdout().flush()?;
                                    }
                                }
                                PtyMessage::Data { data } => {
                                    if let Some(content) = data {
                                        print!("{}", content);
                                        stdout().flush()?;
                                    }
                                }
                                PtyMessage::State { state, prev_state } => {
                                    // Optional: show state changes
                                    // eprintln!("\x1b[90m[state: {:?} -> {}]\x1b[0m", prev_state, state);
                                    let _ = (state, prev_state);
                                }
                                PtyMessage::Exit { code } => {
                                    eprintln!("\n\x1b[33mPTY session exited with code {:?}\x1b[0m", code.unwrap_or(0));
                                    running_clone.store(false, Ordering::SeqCst);
                                    return Ok::<_, anyhow::Error>(code.unwrap_or(0));
                                }
                            },
                            Err(_) => {
                                // Raw data fallback
                                print!("{}", text);
                                stdout().flush()?;
                            }
                        }
                    }
                    Ok(WsMessage::Binary(data)) => {
                        stdout().write_all(&data)?;
                        stdout().flush()?;
                    }
                    Ok(WsMessage::Close(_)) => {
                        eprintln!("\n\x1b[33mDisconnected from PTY session\x1b[0m");
                        running_clone.store(false, Ordering::SeqCst);
                        return Ok(0);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("\n\x1b[31mWebSocket error: {}\x1b[0m", e);
                        running_clone.store(false, Ordering::SeqCst);
                        return Err(e.into());
                    }
                }
            }
            Ok(0)
        } => result,

        // Handle keyboard input
        _ = async {
            while let Some(data) = input_rx.recv().await {
                let msg = ClientMessage::Input { data };
                let json = serde_json::to_string(&msg)?;
                ws_tx.send(WsMessage::Text(json.into())).await?;
            }
            Ok::<_, anyhow::Error>(())
        } => {
            running.store(false, Ordering::SeqCst);
            Ok(0)
        },

        // Handle Ctrl+C signal
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\n\x1b[33mDetaching...\x1b[0m");
            running.store(false, Ordering::SeqCst);
            Ok(0)
        }
    };

    // Cleanup
    running.store(false, Ordering::SeqCst);
    let _ = input_handle.await;

    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("\x1b[31mError: {}\x1b[0m", e);
            std::process::exit(1);
        }
    }
}
