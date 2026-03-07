use axum::{
    extract::{ws::WebSocket, WebSocketUpgrade, Query},
    response::Response,
    routing::get,
    Router,
};
use tracing::info;
use std::collections::HashMap;
use std::sync::Arc;
use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{broadcast, RwLock};
use tower_http::services::ServeDir;

mod types;
use types::{ConnectParams, ClientType};

mod state;
use state::{MasterChannel, SlaveChannel, subscribe_master, subscribe_slave, log_forward, SystemEvent};

type DeviceId = String;
type ConnectionPair = (Option<MasterChannel>, Option<SlaveChannel>);
type Connections = Arc<RwLock<HashMap<DeviceId, ConnectionPair>>>;

#[derive(Clone)]
struct AppState {
    connections: Connections,
    tx: broadcast::Sender<SystemEvent>,
    bytes_in: Arc<AtomicU64>,
    bytes_out: Arc<AtomicU64>,
}

pub async fn run_server() {
    let port = env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    run_server_on(&addr).await;
}

pub async fn run_server_on(addr: &str) {
    let _ = tracing_subscriber::fmt::try_init();

    let connections: Connections = Arc::new(RwLock::new(HashMap::new()));
    let (tx, _rx) = broadcast::channel(100);
    let state = AppState { 
        connections: connections.clone(), 
        tx: tx.clone(),
        bytes_in: Arc::new(AtomicU64::new(0)),
        bytes_out: Arc::new(AtomicU64::new(0)),
    };

    let app = Router::new()
        .route("/register", get(register_handler))
        .route("/pair", get(pair_handler))
        .route("/monitor", get(monitor_handler))
        .fallback_service(ServeDir::new("public"))
        .with_state(state);

    info!("WebSocket server starting on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn register_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<ConnectParams>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let device_id = params.id.clone();
    ws.on_upgrade(move |socket| handle_connection(socket, device_id, ClientType::Slave, state))
}

async fn pair_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<ConnectParams>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let device_id = params.id.clone();
    ws.on_upgrade(move |socket| handle_connection(socket, device_id, ClientType::Master, state))
}

async fn monitor_handler(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_monitor(socket, state))
}

async fn handle_monitor(mut socket: WebSocket, state: AppState) {
    let mut rx = state.tx.subscribe();
    
    // Send initial state
    {
        let conn = state.connections.read().await;
        let mut masters = Vec::new();
        let mut slaves = Vec::new();
        let mut connections_list = Vec::new();

        for (device_id, (master, slave)) in conn.iter() {
            if let Some(chan) = master {
                masters.push((device_id.clone(), chan.subscribers));
            }
            if let Some(chan) = slave {
                slaves.push((device_id.clone(), chan.subscribers));
            }
            if master.is_some() && slave.is_some() {
                 connections_list.push((device_id.clone(), device_id.clone()));
            }
        }
        
        let init_event = SystemEvent::Init {
            masters,
            slaves,
            connections: connections_list,
            bytes_in: state.bytes_in.load(Ordering::Relaxed),
            bytes_out: state.bytes_out.load(Ordering::Relaxed),
        };

        if let Ok(msg) = serde_json::to_string(&init_event) {
            state.bytes_out.fetch_add(msg.len() as u64, Ordering::Relaxed);
            let _ = socket.send(axum::extract::ws::Message::Text(msg.into())).await;
        }
    }

    // Stream events
    while let Ok(msg) = rx.recv().await {
        if let Ok(json) = serde_json::to_string(&msg) {
             state.bytes_out.fetch_add(json.len() as u64, Ordering::Relaxed);
             if socket.send(axum::extract::ws::Message::Text(json.into())).await.is_err() {
                 break;
             }
        }
    }
}

async fn handle_connection(mut socket: WebSocket, device_id: DeviceId, client_type: ClientType, state: AppState) {
    let is_master = matches!(client_type, ClientType::Master);
    let name = client_type.to_string();

    if is_master {
        let mut rx = subscribe_master(&state.connections, &device_id).await;
        info!("{} {} connected", name, device_id);
    let _ = state.tx.send(SystemEvent::ClientConnected { 
        device_id: device_id.clone(), 
        client_type: name.clone() 
    });

        loop {
            tokio::select! {
                Ok(msg) = rx.recv() => {
                    let msg_len = msg.len();
                    if socket.send(axum::extract::ws::Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                    state.bytes_out.fetch_add(msg_len as u64, Ordering::Relaxed);
                    broadcast_stats(&state).await;
                }
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(axum::extract::ws::Message::Text(text))) => {
                            state.bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
                            forward_message(&state.connections, &device_id, text.to_string(), true).await;
                            broadcast_stats(&state).await;
                        }
                        _ => break,
                    }
                }
            }
        }
    } else {
        let mut rx = subscribe_slave(&state.connections, &device_id).await;
        info!("{} {} connected", name, device_id);
        let _ = state.tx.send(SystemEvent::ClientConnected { 
            device_id: device_id.clone(), 
            client_type: name.clone() 
        });

        loop {
            tokio::select! {
                Ok(msg) = rx.recv() => {
                    let msg_len = msg.len();
                    if socket.send(axum::extract::ws::Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                    state.bytes_out.fetch_add(msg_len as u64, Ordering::Relaxed);
                    broadcast_stats(&state).await;
                }
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(axum::extract::ws::Message::Text(text))) => {
                            state.bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
                            forward_message(&state.connections, &device_id, text.to_string(), false).await;
                            broadcast_stats(&state).await;
                        }
                        _ => break,
                    }
                }
            }
        }
    }

    unregister_client(&state.connections, &device_id, is_master).await;
    info!("{} {} disconnected", name, device_id);
    let _ = state.tx.send(SystemEvent::ClientDisconnected { 
        device_id: device_id.clone(), 
        client_type: name.clone() 
    });
}

async fn forward_message(connections: &Connections, device_id: &str, text: String, is_master: bool) {
    let conn = connections.read().await;
    if is_master {
        if let Some((_master_opt, slave_opt)) = conn.get(device_id) {
            if let Some(slv_chan) = slave_opt {
                log_forward(device_id, "Master -> Slave", &text);
                let _ = slv_chan.tx.send(text);
            }
        }
    } else {
        if let Some((master_opt, _slave_opt)) = conn.get(device_id) {
            if let Some(master_chan) = master_opt {
                log_forward(device_id, "Slave -> Master", &text);
                let _ = master_chan.tx.send(text);
            }
        }
    }
}

async fn unregister_client(connections: &Connections, device_id: &str, is_master: bool) {
    let mut conn = connections.write().await;
    if let Some(entry) = conn.get_mut(device_id) {
        if is_master {
            if let Some(chan) = &mut entry.0 {
                chan.subscribers = chan.subscribers.saturating_sub(1);
                if chan.subscribers == 0 {
                    entry.0 = None;
                }
            }
        } else {
            if let Some(chan) = &mut entry.1 {
                chan.subscribers = chan.subscribers.saturating_sub(1);
                if chan.subscribers == 0 {
                    entry.1 = None;
                }
            }
        }
        if entry.0.is_none() && entry.1.is_none() {
            conn.remove(device_id);
        }
    }
}
async fn broadcast_stats(state: &AppState) {
    let conn = state.connections.read().await;
    let mut master_count = 0;
    let mut slave_count = 0;

    for (_, (master, slave)) in conn.iter() {
        if master.is_some() { master_count += 1; }
        if slave.is_some() { slave_count += 1; }
    }

    let stats_event = SystemEvent::StatsUpdate {
        master_count,
        slave_count,
        bytes_in: state.bytes_in.load(Ordering::Relaxed),
        bytes_out: state.bytes_out.load(Ordering::Relaxed),
    };

    let _ = state.tx.send(stats_event);
}
