use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn test_device_mobile_communication() {

    let server_handle = tokio::spawn(async {
        unsafe { std::env::set_var("SERVER_PORT", "3001"); }
        openvibe_server::run_server().await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let device_id = "test_device_123";

    let (device_ws, _) = connect_async(&format!("ws://127.0.0.1:3001/register?id={}", device_id))
        .await
        .expect("Failed to connect device");
    let (mut device_tx, mut device_rx) = device_ws.split();

    let (mobile_ws, _) = connect_async(&format!("ws://127.0.0.1:3001/pair?id={}", device_id))
        .await
        .expect("Failed to connect mobile");
    let (mut mobile_tx, mut mobile_rx) = mobile_ws.split();

    mobile_tx.send(Message::Text("Hello from mobile".to_string())).await.unwrap();
    
    let msg = timeout(Duration::from_secs(1), device_rx.next()).await
        .expect("Timeout waiting for device message")
        .expect("Device connection closed")
        .expect("Device message error");
    assert_eq!(msg, Message::Text("Hello from mobile".to_string()));

    device_tx.send(Message::Text("Hello from device".to_string())).await.unwrap();
    
    let msg = timeout(Duration::from_secs(1), mobile_rx.next()).await
        .expect("Timeout waiting for mobile message")
        .expect("Mobile connection closed")
        .expect("Mobile message error");
    assert_eq!(msg, Message::Text("Hello from device".to_string()));

    server_handle.abort();
}