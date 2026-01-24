//! Sync Client
//!
//! Connects to an upstream sync relay to share task events across machines.

use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};
use tokio::time::interval;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use super::relay::{MachineInfo, SyncMessage};
use crate::cc_tasks::{CCTaskChangeEvent, CCTasksOverview, CCTasksWatcher, WatcherEvent};

/// Sync client options
pub struct SyncClientOptions {
    /// Upstream relay URL (e.g., ws://relay.example.com:9121/sync)
    pub upstream_url: String,
    /// Machine identifier (defaults to hostname)
    pub machine_id: Option<String>,
    /// Reconnect interval on disconnect
    pub reconnect_interval: Duration,
}

impl Default for SyncClientOptions {
    fn default() -> Self {
        Self {
            upstream_url: String::new(),
            machine_id: None,
            reconnect_interval: Duration::from_secs(5),
        }
    }
}

/// Events from sync client
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Connected to relay
    Connected,
    /// Disconnected from relay
    Disconnected,
    /// Received overview from another machine
    RemoteOverview {
        machine_id: String,
        overview: CCTasksOverview,
    },
    /// Received task change from another machine
    RemoteTasksChanged {
        machine_id: String,
        event: CCTaskChangeEvent,
    },
    /// All machines info
    AllMachines(Vec<MachineInfo>),
}

/// Sync client that connects to upstream relay
pub struct SyncClient {
    options: SyncClientOptions,
    hostname: String,
    event_tx: broadcast::Sender<SyncEvent>,
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl SyncClient {
    /// Create a new sync client
    pub fn new(options: SyncClientOptions) -> Self {
        let hostname = gethostname::gethostname()
            .to_string_lossy()
            .to_string();
        let (event_tx, _) = broadcast::channel(256);

        Self {
            options,
            hostname,
            event_tx,
            shutdown_tx: None,
        }
    }

    /// Subscribe to sync events
    pub fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.event_tx.subscribe()
    }

    /// Start the sync client with a CC Tasks watcher
    pub async fn start(&mut self, cc_tasks_watcher: Arc<Mutex<CCTasksWatcher>>) -> anyhow::Result<()> {
        if self.options.upstream_url.is_empty() {
            return Err(anyhow::anyhow!("No upstream URL configured"));
        }

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        let url = self.options.upstream_url.clone();
        let machine_id = self.options.machine_id.clone()
            .unwrap_or_else(|| self.hostname.clone());
        let hostname = self.hostname.clone();
        let reconnect_interval = self.options.reconnect_interval;
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();

            loop {
                tokio::select! {
                    _ = Self::connect_loop(&url, &machine_id, &hostname, &event_tx, cc_tasks_watcher.clone()) => {
                        warn!("Sync connection lost, reconnecting...");
                        let _ = event_tx.send(SyncEvent::Disconnected);
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Sync client shutting down");
                        break;
                    }
                }

                // Wait before reconnecting
                tokio::time::sleep(reconnect_interval).await;
            }
        });

        Ok(())
    }

    /// Stop the sync client
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        info!("Sync client stopped");
    }

    async fn connect_loop(
        url: &str,
        machine_id: &str,
        hostname: &str,
        event_tx: &broadcast::Sender<SyncEvent>,
        cc_tasks_watcher: Arc<Mutex<CCTasksWatcher>>,
    ) -> anyhow::Result<()> {
        let (ws_stream, _) = connect_async(url).await?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        info!(url, machine_id, "Connected to sync relay");
        let _ = event_tx.send(SyncEvent::Connected);

        // Register with relay
        let register = SyncMessage::Register {
            machine_id: machine_id.to_string(),
            hostname: hostname.to_string(),
        };
        let text = serde_json::to_string(&register)?;
        ws_tx.send(Message::Text(text)).await?;

        // Send initial overview
        let overview = cc_tasks_watcher.lock().await.get_overview().await;
        let msg = SyncMessage::Overview {
            machine_id: machine_id.to_string(),
            payload: overview,
        };
        let text = serde_json::to_string(&msg)?;
        ws_tx.send(Message::Text(text)).await?;

        // Subscribe to local task events
        let mut local_events = cc_tasks_watcher.lock().await.subscribe();

        // Heartbeat interval
        let mut heartbeat = interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                // Forward local events to relay
                local_event = local_events.recv() => {
                    let local_event = match local_event {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    let msg = match local_event {
                        WatcherEvent::TasksChanged(e) => Some(SyncMessage::TasksChanged {
                            machine_id: machine_id.to_string(),
                            payload: e,
                        }),
                        _ => None,
                    };

                    if let Some(msg) = msg {
                        let text = serde_json::to_string(&msg)?;
                        ws_tx.send(Message::Text(text)).await?;
                    }
                }

                // Receive events from relay
                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(sync_msg) = serde_json::from_str::<SyncMessage>(&text) {
                                match sync_msg {
                                    SyncMessage::Overview { machine_id, payload } => {
                                        let _ = event_tx.send(SyncEvent::RemoteOverview {
                                            machine_id,
                                            overview: payload,
                                        });
                                    }
                                    SyncMessage::TasksChanged { machine_id, payload } => {
                                        let _ = event_tx.send(SyncEvent::RemoteTasksChanged {
                                            machine_id,
                                            event: payload,
                                        });
                                    }
                                    SyncMessage::AllMachines { machines } => {
                                        let _ = event_tx.send(SyncEvent::AllMachines(machines));
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            error!(error = %e, "Sync WebSocket error");
                            break;
                        }
                        _ => {}
                    }
                }

                // Send heartbeat
                _ = heartbeat.tick() => {
                    let msg = SyncMessage::Heartbeat {
                        machine_id: machine_id.to_string(),
                    };
                    let text = serde_json::to_string(&msg)?;
                    if ws_tx.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}
