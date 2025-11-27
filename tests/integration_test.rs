use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio::time::{sleep, timeout, Duration};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn device_broadcasts_to_multiple_masters() {
    let port = 4001u16;

    // start server on a specific address to avoid env var races between tests
    let bind = format!("0.0.0.0:{}", port);
    let _server = tokio::spawn(async move { openvibe_server::run_server_on(&bind).await });

    sleep(Duration::from_millis(300)).await;

    // slave connects to /register
    let (device_ws_stream, _) = connect_async(&format!("ws://127.0.0.1:{}/register?id={}", port, "device123")).await.expect("device connect");
    let (mut device_sink, mut device_stream) = device_ws_stream.split();

    // masters connect to /pair
    let mut mobile_sinks = Vec::new();
    let mut mobile_receivers = Vec::new();
    for _ in 0..3 {
        let (ws_stream, _) = connect_async(&format!("ws://127.0.0.1:{}/pair?id={}", port, "device123")).await.expect("mobile connect");
        let (s, mut r) = ws_stream.split();

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(1);
        tokio::spawn(async move {
            if let Some(Ok(Message::Text(txt))) = r.next().await {
                let _ = tx.send(txt).await;
            }
        });

        mobile_sinks.push(s);
        mobile_receivers.push(rx);
    }

    // Device reader
    let (dev_tx, mut dev_rx) = tokio::sync::mpsc::channel::<String>(1);
    tokio::spawn(async move {
        if let Some(Ok(Message::Text(txt))) = device_stream.next().await {
            let _ = dev_tx.send(txt).await;
        }
    });

    // Device broadcasts to all mobiles
    let msg = "hello mobiles".to_string();
    device_sink.send(Message::Text(msg.clone())).await.expect("send from device");

    for mut rx in mobile_receivers {
        let got = timeout(Duration::from_secs(1), rx.recv()).await.expect("recv timeout");
        assert_eq!(got.unwrap(), msg);
    }

    // Mobile sends command to device
    let mut first_mobile_sink = mobile_sinks.swap_remove(0);
    let mmsg = "hello device".to_string();
    first_mobile_sink.send(Message::Text(mmsg.clone())).await.expect("send from mobile");

    let got = timeout(Duration::from_secs(1), dev_rx.recv()).await.expect("dev recv timeout");
    assert_eq!(got.unwrap(), mmsg);
}

#[tokio::test]
async fn test_device_mobile_communication() {
    // use a different port for this small test
    let bind = "0.0.0.0:3001".to_string();
    let server_handle = tokio::spawn(async move { openvibe_server::run_server_on(&bind).await });

    sleep(Duration::from_millis(200)).await;

    let device_id = "test_device_123";

    // slave connects to /register
    let (device_ws, _) = connect_async(&format!("ws://127.0.0.1:3001/register?id={}", device_id))
        .await
        .expect("Failed to connect device");
    let (mut device_tx, mut device_rx) = device_ws.split();

    // master connects to /pair
    let (mobile_ws, _) = connect_async(&format!("ws://127.0.0.1:3001/pair?id={}", device_id))
        .await
        .expect("Failed to connect mobile");
    let (mut mobile_tx, mut mobile_rx) = mobile_ws.split();

    // Mobile sends command to device
    mobile_tx.send(Message::Text("Hello from mobile".to_string())).await.unwrap();
    
    let msg = timeout(Duration::from_secs(1), device_rx.next()).await
        .expect("Timeout waiting for device message")
        .expect("Device connection closed")
        .expect("Device message error");
    assert_eq!(msg, Message::Text("Hello from mobile".to_string()));

    // Device responds to mobile
    device_tx.send(Message::Text("Hello from device".to_string())).await.unwrap();
    
    let msg = timeout(Duration::from_secs(1), mobile_rx.next()).await
        .expect("Timeout waiting for mobile message")
        .expect("Mobile connection closed")
        .expect("Mobile message error");
    assert_eq!(msg, Message::Text("Hello from device".to_string()));

    server_handle.abort();
}

#[tokio::test]
async fn mobile_messages_do_not_go_to_other_masters() {
    // start server on a dedicated port
    let bind = "0.0.0.0:4002".to_string();
    let _server = tokio::spawn(async move { openvibe_server::run_server_on(&bind).await });
    sleep(Duration::from_millis(200)).await;

    let device_id = "device_no_cross";

    // slave connects to /register
    let (device_ws, _) = connect_async(&format!("ws://127.0.0.1:4002/register?id={}", device_id))
        .await
        .expect("Device connect");
    let (mut _device_tx, mut _device_rx) = device_ws.split();

    // masters connect to /pair
    let (m1_ws, _) = connect_async(&format!("ws://127.0.0.1:4002/pair?id={}", device_id)).await.expect("m1 connect");
    let (mut m1_tx, _m1_rx) = m1_ws.split();

    let (m2_ws, _) = connect_async(&format!("ws://127.0.0.1:4002/pair?id={}", device_id)).await.expect("m2 connect");
    let (_m2_tx, mut m2_rx) = m2_ws.split();

    // Spawn reader for master2 that attempts to read a message (should NOT get messages from master1)
    let (m2_chan_tx, mut m2_chan_rx) = tokio::sync::mpsc::channel::<String>(1);
    tokio::spawn(async move {
        if let Some(Ok(Message::Text(txt))) = m2_rx.next().await {
            let _ = m2_chan_tx.send(txt).await;
        }
    });

    // master1 sends command to device
    let text = "from mobile1".to_string();
    m1_tx.send(Message::Text(text.clone())).await.expect("send from m1");

    // master2 should NOT receive mobile1's message (we expect a timeout)
    let res = timeout(Duration::from_millis(200), m2_chan_rx.recv()).await;
    assert!(res.is_err(), "mobile2 should NOT receive messages from another mobile");
}