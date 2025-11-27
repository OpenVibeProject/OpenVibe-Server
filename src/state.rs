use serde_json::Value as JsonValue;
use tokio::sync::broadcast;
use tracing::info;

pub struct MasterChannel {
    pub tx: broadcast::Sender<String>,
    pub subscribers: usize,
}

pub struct SlaveChannel {
    pub tx: broadcast::Sender<String>,
    pub subscribers: usize,
}

pub async fn subscribe_master(connections: &crate::Connections, device_id: &str) -> broadcast::Receiver<String> {
    let mut conn = connections.write().await;
    let entry = conn.entry(device_id.to_string()).or_insert((None, None));
    if let Some(chan) = &mut entry.0 {
        chan.subscribers += 1;
        chan.tx.subscribe()
    } else {
        let (tx, _rx) = broadcast::channel(100);
        entry.0 = Some(MasterChannel { tx: tx.clone(), subscribers: 1 });
        tx.subscribe()
    }
}

pub async fn subscribe_slave(connections: &crate::Connections, device_id: &str) -> broadcast::Receiver<String> {
    let mut conn = connections.write().await;
    let entry = conn.entry(device_id.to_string()).or_insert((None, None));
    if let Some(chan) = &mut entry.1 {
        chan.subscribers += 1;
        chan.tx.subscribe()
    } else {
        let (tx, _rx) = broadcast::channel(100);
        entry.1 = Some(SlaveChannel { tx: tx.clone(), subscribers: 1 });
        tx.subscribe()
    }
}

pub fn log_forward(device_id: &str, direction: &str, text: &str) {
    let pretty = match serde_json::from_str::<JsonValue>(text) {
        Ok(v) => serde_json::to_string(&v).unwrap_or_else(|_| text.to_string()),
        Err(_) => text.to_string(),
    };
    if pretty.contains('\n') {
        info!("[{}] {} |\n{}", device_id, direction, pretty);
    } else {
        info!("[{}] {} | {}", device_id, direction, pretty);
    }
}
