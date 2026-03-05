use std::sync::{Arc, Mutex};
use kernel::shared_state::SharedState;
use tracing::info;

pub type UiState = Arc<Mutex<SharedState>>;

pub async fn start(port: u16, state: UiState) {
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

            let response = if request.starts_with("GET /state") {
                // Clone under lock — release before serializing
                let devices = {
                    let s = state.lock().unwrap();
                    s.devices.clone()
                };
                let body = serde_json::to_string_pretty(&devices).unwrap_or_default();
                http_200(body)

            } else if request.starts_with("GET /conflicts") {
                let conflicts = {
                    let s = state.lock().unwrap();
                    s.conflicts.clone()
                };
                let body = serde_json::to_string_pretty(&conflicts).unwrap_or_default();
                http_200(body)

            } else if request.starts_with("GET /commands") {
                let commands = {
                    let s = state.lock().unwrap();
                    s.pending_commands.clone()
                };
                let body = serde_json::to_string_pretty(&commands).unwrap_or_default();
                http_200(body)

            } else {
                let body = r#"{"routes":["/state","/conflicts","/commands"]}"#.to_string();
                http_200(body)
            };

            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}

fn http_200(body: String) -> String {
    format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body
    )
}