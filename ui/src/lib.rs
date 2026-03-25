use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    http::Method,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json,
};
use kernel::shared_state::SharedState;
use kernel::types::Capability;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

pub type UiState = Arc<Mutex<SharedState>>;

// Embedded at compile time — no filesystem dependency at runtime
const DASHBOARD_HTML: &str = include_str!("dashboard.html");
const HOME_HTML: &str      = include_str!("home.html");

pub async fn start(port: u16, state: UiState) {
    let cors = CorsLayer::new()
        .allow_origin(Any)          // tighten this once you add auth
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    let app = Router::new()
        // HTML pages
        .route("/",          get(dashboard))
        .route("/index.html",get(dashboard))
        .route("/home",      get(home))
        // JSON state endpoints — same paths as before, Flutter can hit these now
        .route("/state",          get(get_state))
        .route("/conflicts",      get(get_conflicts))
        .route("/commands",       get(get_commands))
        .route("/reconciliation", get(get_reconciliation))
        .route("/rules",          get(get_rules))
        .route("/devices", get(get_devices))
        // WebSocket — Flutter will connect here for real-time push
        .route("/ws",             get(ws_handler))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    info!(port = port, "UI server listening");

    axum::serve(listener, app).await.unwrap();
}

// ── HTML handlers ──────────────────────────────────────────────

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn home() -> Html<&'static str> {
    Html(HOME_HTML)
}

// ── JSON state handlers ────────────────────────────────────────

async fn get_state(State(state): State<UiState>) -> impl IntoResponse {
    let devices = state.lock().unwrap().devices.clone();
    Json(devices)
}

async fn get_conflicts(State(state): State<UiState>) -> impl IntoResponse {
    let conflicts = state.lock().unwrap().conflicts.clone();
    Json(conflicts)
}

async fn get_commands(State(state): State<UiState>) -> impl IntoResponse {
    let commands = state.lock().unwrap().pending_commands.clone();
    Json(commands)
}

async fn get_reconciliation(State(state): State<UiState>) -> impl IntoResponse {
    let report = state.lock().unwrap().last_reconciliation.clone();
    match report {
        Some(r) => Json(serde_json::to_value(r).unwrap_or_default()),
        None    => Json(serde_json::json!({ "status": "not_yet_reconciled" })),
    }
}

async fn get_rules(State(state): State<UiState>) -> impl IntoResponse {
    let s = state.lock().unwrap();
    Json(serde_json::json!({
        "rules":     s.rule_summaries,
        "in_flight": s.in_flight,
    }))
}

async fn get_devices(State(state): State<UiState>) -> impl IntoResponse {
    #[derive(serde::Serialize)]
    struct DeviceInfo {
        id: String,
        name: String,
        kind: String,
        writable: bool,
    }

    let registry = state.lock().unwrap().registry.clone();
    let info: Vec<DeviceInfo> = registry.iter().map(|d| DeviceInfo {
        id: d.id.0.clone(),
        name: d.name.clone(),
        kind: format!("{:?}", d.kind),
        writable: d.capabilities.iter().any(|c| matches!(c, Capability::Writable(_))),
    }).collect();

    Json(info)
}

// ── WebSocket handler ──────────────────────────────────────────
// Flutter connects here and receives state snapshots on every change.
// You'll expand this once SharedState has a change-notification channel.

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<UiState>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: UiState) {
    // For now: send a full state snapshot immediately on connect,
    // then poll every 2 seconds until you wire up a proper change channel.
    // Replace the interval with a tokio::sync::watch receiver once ready.
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));

    loop {
        interval.tick().await;

        let snapshot = {
            let s = state.lock().unwrap();
            serde_json::json!({
                "devices":  s.devices,
                "conflicts": s.conflicts,
            })
        };

        let msg = Message::Text(snapshot.to_string().into());
        if socket.send(msg).await.is_err() {
            // Client disconnected — exit cleanly
            break;
        }
    }
}