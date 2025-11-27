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
use tokio::sync::RwLock;

mod types;
use types::{ConnectParams, ClientType};

mod state;
use state::{MasterChannel, SlaveChannel, subscribe_master, subscribe_slave, log_forward};

type DeviceId = String;
type ConnectionPair = (Option<MasterChannel>, Option<SlaveChannel>);
type Connections = Arc<RwLock<HashMap<DeviceId, ConnectionPair>>>;

#[derive(Clone)]
struct AppState {
    connections: Connections,
}

pub async fn run_server() {
    let port = env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    run_server_on(&addr).await;
}

pub async fn run_server_on(addr: &str) {
    let _ = tracing_subscriber::fmt::try_init();

    let connections: Connections = Arc::new(RwLock::new(HashMap::new()));
    let state = AppState { connections: connections.clone() };

    let app = Router::new()
        .route("/register", get(register_handler))
        .route("/pair", get(pair_handler))
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

async fn handle_connection(mut socket: WebSocket, device_id: DeviceId, client_type: ClientType, state: AppState) {
    let is_master = matches!(client_type, ClientType::Master);
    let name = client_type.to_string();

    if is_master {
        let mut rx = subscribe_master(&state.connections, &device_id).await;
        info!("{} {} connected", name, device_id);

        loop {
            tokio::select! {
                Ok(msg) = rx.recv() => {
                    if socket.send(axum::extract::ws::Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(axum::extract::ws::Message::Text(text))) => {
                            forward_message(&state.connections, &device_id, text.to_string(), true).await;
                        }
                        _ => break,
                    }
                }
            }
        }
    } else {
        let mut rx = subscribe_slave(&state.connections, &device_id).await;
        info!("{} {} connected", name, device_id);

        loop {
            tokio::select! {
                Ok(msg) = rx.recv() => {
                    if socket.send(axum::extract::ws::Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(axum::extract::ws::Message::Text(text))) => {
                            forward_message(&state.connections, &device_id, text.to_string(), false).await;
                        }
                        _ => break,
                    }
                }
            }
        }
    }

    unregister_client(&state.connections, &device_id, is_master).await;
    info!("{} {} disconnected", name, device_id);
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