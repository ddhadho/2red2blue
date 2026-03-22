use std::sync::{Arc, Mutex};
use kernel::shared_state::SharedState;
use tracing::{info, warn};

pub type UiState = Arc<Mutex<SharedState>>;

// Embedded dashboard — compiled into the binary so the UI server has
// no filesystem dependency at runtime. The HTML file is read from disk
// at compile time via include_str!. Path is relative to this source file.
const DASHBOARD_HTML: &str = include_str!("dashboard.html");

pub async fn start(port: u16, state: UiState) {
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    info!(port = port, "UI server listening");

    loop {
        let (mut socket, peer) = listener.accept().await.unwrap();
        let state = state.clone();

        tokio::spawn(async move {
            // Read the full request line — 4KB is enough for any GET request
            let mut buf = [0u8; 4096];
            let n = match socket.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };

            let request = String::from_utf8_lossy(&buf[..n]);

            // Extract just the request line for routing — "GET /path HTTP/1.1"
            let request_line = request.lines().next().unwrap_or("");

            let response = route(request_line, &state);

            if let Err(e) = socket.write_all(response.as_bytes()).await {
                warn!(peer = %peer, error = %e, "failed to write response");
            }
        });
    }
}

fn route(request_line: &str, state: &UiState) -> String {
    // Match on path only — ignore query strings and HTTP version suffix.
    // All endpoints are GET. Any non-GET returns 405.
    if !request_line.starts_with("GET ") {
        return http_405();
    }

    // Extract path — "GET /path HTTP/1.1" → "/path"
    let path = request_line
        .trim_start_matches("GET ")
        .split_whitespace()
        .next()
        .unwrap_or("/");

    // Longer prefixes first — /reconciliation before nothing ambiguous here,
    // but ordering matters if paths ever share a prefix.
    match path {
        "/" | "/index.html" => {
            http_200_html(DASHBOARD_HTML.to_string())
        }

        "/home" => http_200_html(include_str!("home.html").to_string()),

        "/state" => {
            let devices = {
                let s = state.lock().unwrap();
                s.devices.clone()
            };
            http_200_json(serde_json::to_string_pretty(&devices).unwrap_or_default())
        }

        "/conflicts" => {
            let conflicts = {
                let s = state.lock().unwrap();
                s.conflicts.clone()
            };
            http_200_json(serde_json::to_string_pretty(&conflicts).unwrap_or_default())
        }

        "/commands" => {
            let commands = {
                let s = state.lock().unwrap();
                s.pending_commands.clone()
            };
            http_200_json(serde_json::to_string_pretty(&commands).unwrap_or_default())
        }

        "/reconciliation" => {
            let report = {
                let s = state.lock().unwrap();
                s.last_reconciliation.clone()
            };
            let body = match report {
                Some(r) => serde_json::to_string_pretty(&r).unwrap_or_default(),
                None    => r#"{"status":"not_yet_reconciled"}"#.to_string(),
            };
            http_200_json(body)
        }

        "/rules" => {
            let (summaries, in_flight) = {
                let s = state.lock().unwrap();
                (s.rule_summaries.clone(), s.in_flight.clone())
            };
            let body = serde_json::to_string_pretty(&serde_json::json!({
                "rules": summaries,
                "in_flight": in_flight,
            })).unwrap_or_default();
            http_200_json(body)
        }

        _ => {
            let body = serde_json::to_string_pretty(&serde_json::json!({
                "routes": ["/", "/state", "/conflicts", "/commands",
                           "/reconciliation", "/rules"]
            })).unwrap_or_default();
            http_404(body)
        }
    }
}

// ── Response builders ─────────────────────────────────────────

fn http_200_json(body: String) -> String {
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

fn http_200_html(body: String) -> String {
    format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body
    )
}

fn http_404(body: String) -> String {
    format!(
        "HTTP/1.1 404 Not Found\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body
    )
}

fn http_405() -> String {
    let body = r#"{"error":"method not allowed"}"#;
    format!(
        "HTTP/1.1 405 Method Not Allowed\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        body.len(),
        body
    )
}