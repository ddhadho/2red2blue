use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use kernel::types::{DeviceId, DeviceState};
use tracing::info;

pub type SharedState = Arc<Mutex<HashMap<DeviceId, DeviceState>>>;

pub async fn start(port: u16, state: SharedState) {
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    info!(port = port, "UI server listening");

    loop {
        let (mut socket, _) = listener.accept().await.unwrap();
        let state = state.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let request = String::from_utf8_lossy(&buf);

            if request.starts_with("GET /state") {
                // 1. Open a new scope to drop the lock early
                let body = {
                    let state_map = state.lock().unwrap();
                    serde_json::to_string_pretty(&*state_map).unwrap()
                }; // 2. state_map (the MutexGuard) is dropped here!

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );

                // 3. Now it is safe to await because the guard is gone
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
    }
}