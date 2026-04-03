use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    http::{Method, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json,
};
use kernel::shared_state::{SharedState, UiCommand};
use kernel::types::Capability;
use serde::Deserialize;
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use ulid::Ulid;

pub type UiState = Arc<Mutex<SharedState>>;

const DASHBOARD_HTML: &str = include_str!("dashboard.html");
const HOME_HTML: &str      = include_str!("home.html");

// ── start ─────────────────────────────────────────────────────────────────────
//
// cmd_tx — channel into main's dispatch loop.
// POST /command sends UiCommand here; main enqueues through dispatcher.

pub async fn start(
    port:   u16,
    state:  UiState,
    cmd_tx: mpsc::Sender<UiCommand>,
    reload_tx: mpsc::Sender<()>,
) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    // Pack both state and cmd_tx into a single axum state tuple
    let app_state = AppState { shared: state, cmd_tx, reload_tx };

    let app = Router::new()
        // HTML pages
        .route("/",           get(dashboard))
        .route("/index.html", get(dashboard))
        .route("/home",       get(home))
        // JSON read endpoints
        .route("/state",          get(get_state))
        .route("/conflicts",      get(get_conflicts))
        .route("/commands",       get(get_commands))
        .route("/events", get(get_events))
        .route("/reconciliation", get(get_reconciliation))
        .route("/devices",        get(get_devices))
        // Write endpoints
        .route("/rules",              get(get_rules).post(post_rule))
        .route("/rules/:id/enable",   post(enable_rule))
        .route("/rules/:id/disable",  post(disable_rule))
        .route("/rules/:id",          axum::routing::put(put_rule))
        .route("/command",        post(post_command))
        // WebSocket push
        .route("/ws",             get(ws_handler))
        .layer(cors)
        .with_state(app_state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    info!(port = port, "UI server listening");

    axum::serve(listener, app).await.unwrap();
}

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    shared: UiState,
    cmd_tx: mpsc::Sender<UiCommand>,
    reload_tx: mpsc::Sender<()>,
}

// ── HTML handlers ─────────────────────────────────────────────────────────────

async fn dashboard(State(_s): State<AppState>) -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn home(State(_s): State<AppState>) -> Html<&'static str> {
    Html(HOME_HTML)
}

// ── JSON read handlers ────────────────────────────────────────────────────────

async fn get_state(State(s): State<AppState>) -> impl IntoResponse {
    let devices = s.shared.lock().unwrap().devices.clone();
    Json(devices)
}

async fn get_conflicts(State(s): State<AppState>) -> impl IntoResponse {
    let conflicts = s.shared.lock().unwrap().conflicts.clone();
    Json(conflicts)
}

async fn get_commands(State(s): State<AppState>) -> impl IntoResponse {
    let commands = s.shared.lock().unwrap().pending_commands.clone();
    Json(commands)
}

async fn get_events(State(s): State<AppState>) -> impl IntoResponse {
    let history = s.shared.lock().unwrap().event_history.clone();
    Json(history)
}

async fn get_reconciliation(State(s): State<AppState>) -> impl IntoResponse {
    let report = s.shared.lock().unwrap().last_reconciliation.clone();
    match report {
        Some(r) => Json(serde_json::to_value(r).unwrap_or_default()),
        None    => Json(serde_json::json!({ "status": "not_yet_reconciled" })),
    }
}

async fn get_rules(State(s): State<AppState>) -> impl IntoResponse {
    let rules_path = s.shared.lock().unwrap().rules_path.clone();
    let in_flight  = s.shared.lock().unwrap().in_flight.clone();

    let rules: Vec<kernel::rule_types::RuleEntry> = match std::fs::read_to_string(&rules_path) {
        Ok(contents) => toml::from_str::<kernel::rule_types::RuleFile>(&contents)
            .map(|f| f.rules)
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    Json(serde_json::json!({
        "rules":     rules,
        "in_flight": in_flight,
    }))
}

async fn get_devices(State(s): State<AppState>) -> impl IntoResponse {
    #[derive(serde::Serialize)]
    struct DeviceInfo {
        id:       String,
        name:     String,
        kind:     String,
        writable: bool,
    }

    let registry = s.shared.lock().unwrap().registry.clone();
    let info: Vec<DeviceInfo> = registry.iter().map(|d| DeviceInfo {
        id:       d.id.0.clone(),
        name:     d.name.clone(),
        kind:     format!("{:?}", d.kind),
        writable: d.capabilities.iter().any(|c| matches!(c, Capability::Writable(_))),
    }).collect();

    Json(info)
}

// ── POST /command ─────────────────────────────────────────────────────────────
//
// Accepts a manual command from the dashboard or Flutter app.
// Sends it through the ui_cmd_rx channel in main, which enqueues it
// through the dispatcher exactly like a rule-triggered command.
//
// Request:  { "device_id": "main_gate", "attribute": "state", "value": "locked" }
// Response: { "command_id": "01KM...", "status": "dispatched" }


#[derive(Deserialize)]
struct CommandRequest {
    device_id: String,
    attribute: String,
    value:     String,
}

async fn post_command(
    State(s): State<AppState>,
    Json(req): Json<CommandRequest>,
) -> impl IntoResponse {
    // Validate device exists in registry
    let known = {
        let state = s.shared.lock().unwrap();
        state.registry.iter().any(|d| d.id.0 == req.device_id)
    };

    if !known {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error":   "unknown_device",
                "message": format!("device_id '{}' not found in registry", req.device_id),
                "status":  400,
            })),
        ).into_response();
    }

    let command_id = Ulid::new().to_string();

    let ui_cmd = UiCommand {
        device_id:  req.device_id,
        attribute:  req.attribute,
        value:      req.value,
        command_id: command_id.clone(),
    };

    match s.cmd_tx.try_send(ui_cmd) {
        Ok(_) => {
            info!(command_id = %command_id, "UI command dispatched");
            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "command_id": command_id,
                    "status":     "dispatched",
                })),
            ).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "UI command channel full or closed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error":   "adapter_unavailable",
                    "message": "command channel unavailable — daemon may be shutting down",
                    "status":  503,
                })),
            ).into_response()
        }
    }
}

// ── POST /rules ───────────────────────────────────────────────────────────────

async fn post_rule(
    State(s): State<AppState>,
    Json(entry): Json<kernel::rule_types::RuleEntry>,
) -> impl IntoResponse {
    let rules_path = s.shared.lock().unwrap().rules_path.clone();

    // Read current file
    let mut file: kernel::rule_types::RuleFile = match std::fs::read_to_string(&rules_path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or(kernel::rule_types::RuleFile { rules: vec![] }),
        Err(_)       => kernel::rule_types::RuleFile { rules: vec![] },
    };

    // Reject duplicate ID
    if file.rules.iter().any(|r| r.id == entry.id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error":   "duplicate_id",
                "message": format!("rule '{}' already exists", entry.id),
                "status":  400,
            })),
        ).into_response();
    }

    let rule_id = entry.id.clone();
    file.rules.push(entry);

    if let Err(e) = write_rules_file(&rules_path, &file) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "write_failed", "message": e, "status": 500 })),
        ).into_response();
    }

    s.reload_tx.try_send(()).ok();

    info!(rule_id = %rule_id, "rule created via API");
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": rule_id, "status": "loaded" })),
    ).into_response()
}

// ── PUT /rules/:id ────────────────────────────────────────────────────────────

async fn put_rule(
    State(s): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(entry): Json<kernel::rule_types::RuleEntry>,
) -> impl IntoResponse {
    let rules_path = s.shared.lock().unwrap().rules_path.clone();

    let mut file: kernel::rule_types::RuleFile = match std::fs::read_to_string(&rules_path) {
        Ok(c) => toml::from_str(&c).unwrap_or(kernel::rule_types::RuleFile { rules: vec![] }),
        Err(_) => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found", "message": format!("rule '{}' not found", id), "status": 404 })),
        ).into_response(),
    };

    let pos = match file.rules.iter().position(|r| r.id == id) {
        Some(p) => p,
        None    => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found", "message": format!("rule '{}' not found", id), "status": 404 })),
        ).into_response(),
    };

    file.rules[pos] = entry;

    if let Err(e) = write_rules_file(&rules_path, &file) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "write_failed", "message": e, "status": 500 })),
        ).into_response();
    }

    s.reload_tx.try_send(()).ok();
    info!(rule_id = %id, "rule updated via API");
    (StatusCode::OK, Json(serde_json::json!({ "id": id, "status": "updated" }))).into_response()
}

// ── POST /rules/:id/enable and /disable ───────────────────────────────────────

async fn enable_rule(
    State(s): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    set_rule_enabled(s, id, true).await
}

async fn disable_rule(
    State(s): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    set_rule_enabled(s, id, false).await
}

async fn set_rule_enabled(s: AppState, id: String, enabled: bool) -> Response {
    let rules_path = s.shared.lock().unwrap().rules_path.clone();

    let mut file: kernel::rule_types::RuleFile = match std::fs::read_to_string(&rules_path) {
        Ok(c) => toml::from_str(&c).unwrap_or(kernel::rule_types::RuleFile { rules: vec![] }),
        Err(_) => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found", "message": format!("rule '{}' not found", id), "status": 404 })),
        ).into_response(),
    };

    let pos = match file.rules.iter().position(|r| r.id == id) {
        Some(p) => p,
        None    => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found", "message": format!("rule '{}' not found", id), "status": 404 })),
        ).into_response(),
    };

    file.rules[pos].enabled = enabled;

    if let Err(e) = write_rules_file(&rules_path, &file) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "write_failed", "message": e, "status": 500 })),
        ).into_response();
    }

    s.reload_tx.try_send(()).ok();
    let status = if enabled { "enabled" } else { "disabled" };
    info!(rule_id = %id, %status, "rule toggled via API");
    (StatusCode::OK, Json(serde_json::json!({ "id": id, "status": status }))).into_response()
}

// ── File write helper ─────────────────────────────────────────────────────────

fn write_rules_file(path: &str, file: &kernel::rule_types::RuleFile) -> Result<(), String> {
    let contents = toml::to_string_pretty(file)
        .map_err(|e: toml::ser::Error| e.to_string())?;
    std::fs::write(path, contents)
        .map_err(|e: std::io::Error| e.to_string())
}

// ── WebSocket handler ─────────────────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(s): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, s.shared))
}

async fn handle_socket(mut socket: WebSocket, state: UiState) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));

    loop {
        interval.tick().await;

        let snapshot = {
            let s = state.lock().unwrap();
            serde_json::json!({
                "devices":   s.devices,
                "conflicts": s.conflicts,
            })
        };

        let msg = Message::Text(snapshot.to_string());
        if socket.send(msg).await.is_err() {
            break;
        }
    }
}