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
use tokio::sync::{broadcast, RwLock};

mod types;
use types::{ConnectParams, ClientType};

type DeviceId = String;
type Sender = broadcast::Sender<String>;
type ConnectionPair = (Option<Sender>, Option<Sender>);
type Connections = Arc<RwLock<HashMap<DeviceId, ConnectionPair>>>;

pub async fn run_server() {
    tracing_subscriber::fmt::init();
    
    let connections: Connections = Arc::new(RwLock::new(HashMap::new()));
    
    let app = Router::new()
        .route("/register", get(register_handler))
        .route("/pair", get(pair_handler))
        .with_state(connections);

    let port = env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    
    info!("WebSocket server starting on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn register_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<ConnectParams>,
    axum::extract::State(connections): axum::extract::State<Connections>,
) -> Response {
    ws.on_upgrade(move |socket| handle_connection(socket, params.id, ClientType::Device, connections))
}

async fn pair_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<ConnectParams>,
    axum::extract::State(connections): axum::extract::State<Connections>,
) -> Response {
    ws.on_upgrade(move |socket| handle_connection(socket, params.id, ClientType::Mobile, connections))
}

async fn handle_connection(mut socket: WebSocket, device_id: DeviceId, client_type: ClientType, connections: Connections) {
    let (tx, mut rx) = broadcast::channel(100);
    let is_device = matches!(client_type, ClientType::Device);
    let name = if is_device { "Device" } else { "Mobile" };

    register_client(&connections, &device_id, tx, is_device).await;
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
                        forward_message(&connections, &device_id, text.to_string(), is_device).await;
                    }
                    _ => break,
                }
            }
        }
    }
    
    unregister_client(&connections, &device_id, is_device).await;
    info!("{} {} disconnected", name, device_id);
}

async fn register_client(connections: &Connections, device_id: &str, tx: Sender, is_device: bool) {
    let mut conn = connections.write().await;
    let entry = conn.entry(device_id.to_string()).or_insert((None, None));
    if is_device { entry.0 = Some(tx); } else { entry.1 = Some(tx); }
}

async fn forward_message(connections: &Connections, device_id: &str, text: String, is_device: bool) {
    let conn = connections.read().await;
    let peer_tx = conn.get(device_id).and_then(|(device, mobile)| {
        if is_device { mobile.as_ref() } else { device.as_ref() }
    });
    if let Some(tx) = peer_tx {
        let _ = tx.send(text);
    }
}

async fn unregister_client(connections: &Connections, device_id: &str, is_device: bool) {
    let mut conn = connections.write().await;
    if let Some(entry) = conn.get_mut(device_id) {
        if is_device { entry.0 = None; } else { entry.1 = None; }
        if entry.0.is_none() && entry.1.is_none() {
            conn.remove(device_id);
        }
    }
}