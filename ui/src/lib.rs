use std::{collections::HashMap, sync::{Arc, Mutex}};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::StreamExt;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

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
        .route("/dashboard",           get(dashboard))
        .route("/index.html", get(home))
        .route("/",       get(home))
        // JSON read endpoints
        .route("/state",          get(get_state))
        .route("/conflicts",      get(get_conflicts))
        .route("/commands",       get(get_commands))
        .route("/events", get(get_events))
        .route("/reconciliation", get(get_reconciliation))
        .route("/devices",      get(get_devices).post(post_device))
        .route("/ha/entities",  get(get_ha_entities))
        // Write endpoints
        .route("/rules",              get(get_rules).post(post_rule))
        .route("/rules/:id/enable",   post(enable_rule))
        .route("/rules/:id/disable",  post(disable_rule))
        .route("/rules/:id",          axum::routing::put(put_rule))
        .route("/command",        post(post_command))
        // WebSocket push
        .route("/ws",             get(ws_handler))
        .route("/stream", get(sse_handler))
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

// ── GET /ha/entities ──────────────────────────────────────────────────────────
//
// Proxies to HA /api/states and marks which entities are already configured.
// Used by the Flutter device discovery screen.

async fn get_ha_entities(State(s): State<AppState>) -> impl IntoResponse {
    let (ha_url, ha_token, configured_ids) = {
        let state = s.shared.lock().unwrap();
        let configured: std::collections::HashSet<String> = state
            .registry
            .iter()
            .map(|d| d.external_id.clone())
            .collect();
        (state.ha_url.clone(), state.ha_token.clone(), configured)
    };

    if ha_url.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error":   "no_ha_config",
                "message": "home_assistant not configured",
                "status":  503,
            })),
        ).into_response();
    }

    let url = format!("{}/api/states", ha_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let resp = match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", ha_token))
        .header("Content-Type", "application/json")
        .send()
        .await
    {
        Ok(r)  => r,
        Err(e) => return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error":   "ha_unreachable",
                "message": e.to_string(),
                "status":  503,
            })),
        ).into_response(),
    };

    let states: Vec<serde_json::Value> = match resp.json().await {
        Ok(v)  => v,
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error":   "parse_failed",
                "message": e.to_string(),
                "status":  500,
            })),
        ).into_response(),
    };

    let entities: Vec<serde_json::Value> = states.iter().map(|entity| {
        let entity_id = entity["entity_id"].as_str().unwrap_or("").to_string();
        let domain    = entity_id.split('.').next().unwrap_or("").to_string();
        let friendly  = entity["attributes"]["friendly_name"]
            .as_str()
            .unwrap_or(&entity_id)
            .to_string();
        let already   = configured_ids.contains(&entity_id);

        serde_json::json!({
            "entity_id":          entity_id,
            "state":              entity["state"],
            "friendly_name":      friendly,
            "domain":             domain,
            "already_configured": already,
        })
    }).collect();

    (StatusCode::OK, Json(serde_json::json!(entities))).into_response()
}

// ── POST /devices ─────────────────────────────────────────────────────────────
//
// Adds a new device. Writes to devices.toml and config.toml HA section.
// Does not restart the daemon — the new device is picked up on next restart.
// For V1 pilot this is acceptable. Hot-reload of devices is post-pilot.
//
// Request shape:
// {
//   "ha_entity_id":           "switch.geyser",
//   "device_id":              "geyser",
//   "name":                   "Geyser",
//   "kind":                   "SmartPlug",
//   "attribute":              "state",
//   "confidence_decay_seconds": 120,
//   "writable":               true,
//   "safe_default":           { "state": "off" },
//   "state_map":              { "on": "on", "off": "off" },
//   "service_map":            { "on": "switch/turn_on", "off": "switch/turn_off" }
// }

#[derive(Deserialize)]
struct NewDeviceRequest {
    ha_entity_id:             String,
    device_id:                String,
    name:                     String,
    kind:                     String,
    attribute:                String,
    confidence_decay_seconds: u64,
    writable:                 bool,
    safe_default:             HashMap<String, String>,
    state_map:                HashMap<String, String>,
    service_map:              HashMap<String, String>,
}

async fn post_device(
    State(s): State<AppState>,
    Json(req): Json<NewDeviceRequest>,
) -> impl IntoResponse {
    let (devices_path, config_path) = {
        let state = s.shared.lock().unwrap();
        (state.devices_path.clone(), state.config_path.clone())
    };

    // Check for duplicate device_id
    {
        let state = s.shared.lock().unwrap();
        if state.registry.iter().any(|d| d.id.0 == req.device_id) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error":   "duplicate_device",
                    "message": format!("device_id '{}' already exists", req.device_id),
                    "status":  400,
                })),
            ).into_response();
        }
    }

    // ── Write to devices.toml ─────────────────────────────────────────────────

    let capability = if req.writable {
        format!("\n[[devices.capabilities]]\nWritable = \"{}\"", req.attribute)
    } else {
        format!("\n[[devices.capabilities]]\nReadable = \"{}\"", req.attribute)
    };

    let safe_default_lines: String = req.safe_default.iter()
        .map(|(k, v)| format!("{} = \"{}\"", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    let device_toml = format!(
        "\n[[devices]]\nid = \"{}\"\nexternal_id = \"{}\"\nname = \"{}\"\nkind = \"{}\"\nconfidence_decay_seconds = {}{}\n\n[devices.safe_default]\n{}\n",
        req.device_id,
        req.device_id,
        req.name,
        req.kind,
        req.confidence_decay_seconds,
        capability,
        safe_default_lines,
    );

    if let Err(e) = append_to_file(&devices_path, &device_toml) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error":   "write_failed",
                "message": format!("failed to write devices.toml: {}", e),
                "status":  500,
            })),
        ).into_response();
    }

    // ── Write to config.toml HA devices section ───────────────────────────────

    let state_map_inline = map_to_inline_toml(&req.state_map);
    let service_map_inline = map_to_inline_toml(&req.service_map);

    let ha_device_toml = format!(
        "\n[[home_assistant.devices]]\nha_entity_id = \"{}\"\ndevice_id    = \"{}\"\nattribute    = \"{}\"\nstate_map    = {}\nservice_map  = {}\n",
        req.ha_entity_id,
        req.device_id,
        req.attribute,
        state_map_inline,
        service_map_inline,
    );

    if let Err(e) = append_to_file(&config_path, &ha_device_toml) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error":   "write_failed",
                "message": format!("failed to write config.toml: {}", e),
                "status":  500,
            })),
        ).into_response();
    }

    info!(
        device_id  = %req.device_id,
        ha_entity  = %req.ha_entity_id,
        "device added via API — restart daemon to activate"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "device_id": req.device_id,
            "status":    "created",
            "note":      "restart daemon to activate new device",
        })),
    ).into_response()
}

// ── File helpers ──────────────────────────────────────────────────────────────

fn append_to_file(path: &str, content: &str) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())
}

fn map_to_inline_toml(map: &HashMap<String, String>) -> String {
    if map.is_empty() {
        return "{}".to_string();
    }
    let pairs: Vec<String> = map.iter()
        .map(|(k, v)| format!("\"{}\" = \"{}\"", k, v))
        .collect();
    format!("{{ {} }}", pairs.join(", "))
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

// ── GET /stream — Server-Sent Events ─────────────────────────────────────────
//
// Persistent connection. Daemon pushes events as they happen.
// Flutter app subscribes once and receives instant updates.
// Heartbeat every 30 seconds — client reconnects if missed within 60s.

async fn sse_handler(
    State(s): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = s.shared.lock().unwrap().sse_tx.subscribe();

    let stream = BroadcastStream::new(rx)
        .filter_map(|result| async move {
            match result {
                Ok(msg) => {
                    let event = Event::default()
                        .event(msg.event)
                        .data(msg.data);
                    Some(Ok(event))
                }
                Err(_) => None, // lagged — skip, client will catch up on next poll
            }
        });

    Sse::new(stream).keep_alive(KeepAlive::default())
}