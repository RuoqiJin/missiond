//! Sync Relay Server
//!
//! A simple WebSocket relay that broadcasts task events to all connected machines.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::accept_async;
use tracing::{error, info, warn};

use crate::cc_tasks::{CCTaskChangeEvent, CCTasksOverview};

/// Sync relay server options
pub struct SyncRelayOptions {
    /// Port to listen on
    pub port: u16,
}

/// Sync relay server
pub struct SyncRelay {
    port: u16,
    /// Connected machines: machine_id -> last known overview
    machines: Arc<RwLock<HashMap<String, MachineState>>>,
    /// Broadcast channel for events
    event_tx: broadcast::Sender<SyncMessage>,
    shutdown_tx: Option<broadcast::Sender<()>>,
}

#[derive(Debug, Clone)]
struct MachineState {
    machine_id: String,
    hostname: String,
    overview: Option<CCTasksOverview>,
    last_seen: std::time::Instant,
}

/// Messages exchanged between relay and clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncMessage {
    /// Client registration
    Register {
        machine_id: String,
        hostname: String,
    },
    /// Heartbeat
    Heartbeat {
        machine_id: String,
    },
    /// Tasks overview from a machine
    Overview {
        machine_id: String,
        payload: CCTasksOverview,
    },
    /// Tasks changed event
    TasksChanged {
        machine_id: String,
        payload: CCTaskChangeEvent,
    },
    /// All machines overview (sent by relay to clients)
    AllMachines {
        machines: Vec<MachineInfo>,
    },
}

/// Machine info sent to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    pub machine_id: String,
    pub hostname: String,
    pub overview: Option<CCTasksOverview>,
    pub is_online: bool,
}

impl SyncRelay {
    /// Create a new sync relay
    pub fn new(options: SyncRelayOptions) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            port: options.port,
            machines: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            shutdown_tx: None,
        }
    }

    /// Start the relay server
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;

        info!(port = self.port, "Sync relay server started");

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        let machines = self.machines.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let machines = machines.clone();
                                let event_tx = event_tx.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, addr, machines, event_tx).await {
                                        error!(?e, ?addr, "Sync relay connection error");
                                    }
                                });
                            }
                            Err(e) => {
                                error!(?e, "Failed to accept sync connection");
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Sync relay server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop the relay server
    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        info!("Sync relay server stopped");
    }

    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        machines: Arc<RwLock<HashMap<String, MachineState>>>,
        event_tx: broadcast::Sender<SyncMessage>,
    ) -> anyhow::Result<()> {
        let ws_stream = accept_async(stream).await?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        info!(?addr, "Sync client connected");

        let mut event_rx = event_tx.subscribe();
        let machine_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let machine_id_clone = machine_id.clone();

        loop {
            tokio::select! {
                // Broadcast events to client
                event = event_rx.recv() => {
                    let event = match event {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    // Don't send events back to the originating machine
                    let current_machine = machine_id_clone.lock().await;
                    let skip = match (&event, current_machine.as_ref()) {
                        (SyncMessage::TasksChanged { machine_id: sender, .. }, Some(id)) => sender == id,
                        (SyncMessage::Overview { machine_id: sender, .. }, Some(id)) => sender == id,
                        _ => false,
                    };

                    if !skip {
                        let text = serde_json::to_string(&event)?;
                        if ws_tx.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                }

                // Receive messages from client
                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(sync_msg) = serde_json::from_str::<SyncMessage>(&text) {
                                match &sync_msg {
                                    SyncMessage::Register { machine_id: id, hostname } => {
                                        *machine_id.lock().await = Some(id.clone());
                                        machines.write().await.insert(id.clone(), MachineState {
                                            machine_id: id.clone(),
                                            hostname: hostname.clone(),
                                            overview: None,
                                            last_seen: std::time::Instant::now(),
                                        });
                                        info!(?addr, machine_id = %id, "Machine registered");

                                        // Send all machines info
                                        let all = Self::get_all_machines_info(&machines).await;
                                        let response = SyncMessage::AllMachines { machines: all };
                                        let text = serde_json::to_string(&response)?;
                                        let _ = ws_tx.send(Message::Text(text)).await;
                                    }
                                    SyncMessage::Heartbeat { machine_id: id } => {
                                        if let Some(state) = machines.write().await.get_mut(id) {
                                            state.last_seen = std::time::Instant::now();
                                        }
                                    }
                                    SyncMessage::Overview { machine_id: id, payload } => {
                                        if let Some(state) = machines.write().await.get_mut(id) {
                                            state.overview = Some(payload.clone());
                                            state.last_seen = std::time::Instant::now();
                                        }
                                        // Broadcast to other machines
                                        let _ = event_tx.send(sync_msg);
                                    }
                                    SyncMessage::TasksChanged { .. } => {
                                        // Broadcast to other machines
                                        let _ = event_tx.send(sync_msg);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            warn!(?addr, error = %e, "Sync WebSocket error");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        // Cleanup on disconnect
        if let Some(id) = machine_id.lock().await.take() {
            machines.write().await.remove(&id);
            info!(?addr, machine_id = %id, "Machine disconnected");
        }

        Ok(())
    }

    async fn get_all_machines_info(
        machines: &Arc<RwLock<HashMap<String, MachineState>>>,
    ) -> Vec<MachineInfo> {
        let machines = machines.read().await;
        machines
            .values()
            .map(|state| MachineInfo {
                machine_id: state.machine_id.clone(),
                hostname: state.hostname.clone(),
                overview: state.overview.clone(),
                is_online: state.last_seen.elapsed().as_secs() < 60,
            })
            .collect()
    }
}
