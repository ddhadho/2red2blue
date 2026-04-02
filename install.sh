#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# 2red2blue Daemon Installer
# Supports: Linux (native), macOS (limited), WSL2
# =============================================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

CONFIG_DIR="/etc/smarthome"
DATA_DIR="/var/lib/smarthome"
BINARY_NAME="daemon"
SERVICE_NAME="smarthome"

# --------------------------------------------------------------------------
# Helpers
# --------------------------------------------------------------------------
info()    { echo -e "${BLUE}[INFO]${NC}  $*"; }
success() { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }
ask()     { echo -e "${BOLD}$*${NC}"; }

detect_os() {
  case "$(uname -s)" in
    Linux*)
      if grep -qi microsoft /proc/version 2>/dev/null; then
        OS="wsl"
      else
        OS="linux"
      fi
      ;;
    Darwin*) OS="mac" ;;
    *)       error "Unsupported OS: $(uname -s). This daemon runs on Linux or WSL2." ;;
  esac
}

require_sudo() {
  if [[ "$EUID" -eq 0 ]]; then
    SUDO=""
  elif command -v sudo &>/dev/null; then
    SUDO="sudo"
    info "This script needs sudo for creating system directories."
    sudo -v || error "Could not obtain sudo privileges."
  else
    error "sudo is required but not installed."
  fi
}

# --------------------------------------------------------------------------
# Step 1 — Check / install Rust
# --------------------------------------------------------------------------
install_rust() {
  if command -v cargo &>/dev/null; then
    success "Rust is already installed ($(cargo --version))."
    return
  fi

  info "Rust not found. Installing via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  # Source cargo env for the rest of this script
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
  success "Rust installed ($(cargo --version))."
}

# --------------------------------------------------------------------------
# Step 2 — Locate repo
# --------------------------------------------------------------------------
setup_repo() {
  if ! git -C "$(pwd)" rev-parse --is-inside-work-tree &>/dev/null || [[ ! -f "$(pwd)/Cargo.toml" ]]; then
    error "This script must be run from inside the 2red2blue repo directory.\ncd into the repo first and try again."
  fi
  INSTALL_DIR="$(pwd)"
  success "Repo found at $INSTALL_DIR."
}

# --------------------------------------------------------------------------
# Step 3 — Create system directories
# --------------------------------------------------------------------------
create_dirs() {
  info "Creating system directories..."
  $SUDO mkdir -p "$DATA_DIR/wal" "$DATA_DIR/snapshots" "$CONFIG_DIR"
  $SUDO chown -R "$USER:$USER" "$DATA_DIR" "$CONFIG_DIR"
  success "Directories created."
}

# --------------------------------------------------------------------------
# Step 4 — Prompt for configuration
# --------------------------------------------------------------------------
collect_config() {
  echo ""
  echo -e "${BOLD}========================================${NC}"
  echo -e "${BOLD}   Home Assistant Configuration${NC}"
  echo -e "${BOLD}========================================${NC}"
  echo ""

  ask "Enter your Home Assistant host and port (e.g. 127.0.0.1:8123):"
  read -r HA_HOST
  [[ -z "$HA_HOST" ]] && error "HA host cannot be empty."

  ask "Enter your long-lived HA access token:"
  read -r -s HA_TOKEN
  echo ""
  [[ -z "$HA_TOKEN" ]] && error "HA token cannot be empty."

  echo ""
  info "You can edit entity IDs later in $CONFIG_DIR/devices.toml"
  echo ""
}

# --------------------------------------------------------------------------
# Step 5 — Write config files
# --------------------------------------------------------------------------
write_configs() {
  info "Writing configuration files..."

  # config.toml
  $SUDO tee "$CONFIG_DIR/config.toml" > /dev/null <<EOF
[daemon]
name = "smarthome-daemon"
log_level = "info"
log_output = "stdout"

[storage]
wal_path           = "/var/lib/smarthome/wal"
snapshot_path      = "/var/lib/smarthome/snapshots"
rules_path         = "/etc/smarthome/rules.toml"
devices_path       = "/etc/smarthome/devices.toml"
desired_state_path = "/var/lib/smarthome/desired_state.json"
max_wal_size_mb = 512
snapshot_interval_events = 1000

[adapter]
kind = "homeassistant"
url = "ws://${HA_HOST}/api/websocket"
reconnect_interval_seconds = 5
event_dedup_window_ms = 50

[reconciler]
boot_window_secs = 30
continuous_poll_interval_seconds = 60
confidence_degraded_threshold = 0.5
confidence_unknown_threshold = 0.2

[dispatcher]
timeout_ms = 5000
max_retries = 3
retry_delay_ms = 500

[ui]
enabled = true
port = 7000
bind = "0.0.0.0"

[platform]
kind = "linux"
watchdog_enabled = true
watchdog_interval_seconds = 30
max_memory_mb = 256

[home_assistant]
url = "http://${HA_HOST}"
token = "${HA_TOKEN}"

[[home_assistant.devices]]
ha_entity_id = "input_boolean.main_gate"
device_id    = "main_gate"
attribute    = "state"
state_map    = { "on" = "unlocked", "off" = "locked" }
service_map  = { "unlocked" = "input_boolean/turn_on", "locked" = "input_boolean/turn_off" }

[[home_assistant.devices]]
ha_entity_id = "input_select.mains_power"
device_id    = "mains_power"
attribute    = "source"
state_map    = { "kplc" = "kplc", "outage" = "outage" }
service_map  = {}

[[home_assistant.devices]]
ha_entity_id = "input_boolean.borehole_pump"
device_id    = "borehole_pump"
attribute    = "state"
state_map    = { "on" = "on", "off" = "off" }
service_map  = { "on" = "input_boolean/turn_on", "off" = "input_boolean/turn_off" }
EOF

  # devices.toml — embedded template
  $SUDO tee "$CONFIG_DIR/devices.toml" > /dev/null <<'EOF'
[[devices]]
id = "main_gate"
external_id = "main_gate"
name = "Main Gate"
kind = "Gate"
confidence_decay_seconds = 3000
[[devices.capabilities]]
Writable = "state"
[devices.safe_default]
state = "locked"

[[devices]]
id = "borehole_pump"
external_id = "borehole_pump"
name = "Borehole Pump"
kind = "BoreholePump"
confidence_decay_seconds = 1200
[[devices.capabilities]]
Writable = "state"
[devices.safe_default]
state = "off"

[[devices]]
id = "mains_power"
external_id = "mains_power"
name = "Mains Power"
kind = "PowerMonitor"
confidence_decay_seconds = 1000
[[devices.capabilities]]
Readable = "source"
[devices.safe_default]
source = "outage"
EOF

  # rules.toml — embedded template
  $SUDO tee "$CONFIG_DIR/rules.toml" > /dev/null <<'EOF'
[[rules]]
id = "rule_001"
name = "Gate unknown state - lock it"
enabled = true
priority = 200
conflict_group = "security"
[rules.trigger]
kind = "DeviceStateChanged"
device_id = "main_gate"
attribute = "state"
[[rules.conditions]]
subject_device_id = "main_gate"
subject_attribute = "state"
operator = "IsUnknown"
[[rules.actions]]
device_id = "main_gate"
attribute = "state"
value_text = "locked"

# ─────────────────────────────────────────────────────────────
[[rules]]
id = "rule_002"
name = "Power restored - start pump"
enabled = true
priority = 255
conflict_group = "power_recovery"
[rules.trigger]
kind = "DeviceStateChanged"
device_id = "mains_power"
attribute = "source"
[[rules.conditions]]
subject_device_id = "mains_power"
subject_attribute = "source"
operator = "Equals"
value_text = "kplc"
[[rules.conditions]]
subject_device_id = "mains_power"
subject_attribute = "source"
operator = "WasPreviously"
value_text = "outage"
[[rules.actions]]
device_id = "main_gate"
attribute = "state"
value_text = "locked"
[[rules.actions]]
device_id = "borehole_pump"
attribute = "state"
value_text = "on"
delay_seconds = 30

# ─────────────────────────────────────────────────────────────
[[rules]]
id = "rule_003"
name = "Power outage - stop pump"
enabled = true
priority = 255
conflict_group = "pump"
[rules.trigger]
kind = "DeviceStateChanged"
device_id = "mains_power"
attribute = "source"
[[rules.conditions]]
subject_device_id = "mains_power"
subject_attribute = "source"
operator = "Equals"
value_text = "outage"
[[rules.actions]]
device_id = "borehole_pump"
attribute = "state"
value_text = "off"
EOF

  success "Config files written to $CONFIG_DIR."
}

# --------------------------------------------------------------------------
# Step 6 — Build
# --------------------------------------------------------------------------
build_daemon() {
  info "Building daemon (this may take a few minutes on first run)..."
  cargo build 2>&1
  success "Build complete."
}

# --------------------------------------------------------------------------
# Step 7 — Optionally install as systemd service (Linux/WSL2 with systemd)
# --------------------------------------------------------------------------
install_service() {
  # Only offer on Linux with systemd
  if [[ "$OS" == "mac" ]]; then
    return
  fi
  if ! command -v systemctl &>/dev/null; then
    warn "systemd not found — skipping service install."
    return
  fi

  echo ""
  ask "Install daemon as a systemd service that starts on boot? [y/N]:"
  read -r INSTALL_SERVICE
  if [[ "$INSTALL_SERVICE" =~ ^[Yy]$ ]]; then
    BINARY_PATH="$INSTALL_DIR/target/debug/$BINARY_NAME"
    $SUDO tee "/etc/systemd/system/${SERVICE_NAME}.service" > /dev/null <<EOF
[Unit]
Description=2red2blue Smart Home Daemon
After=network.target

[Service]
Type=simple
ExecStart=${BINARY_PATH} ${CONFIG_DIR}/config.toml
Restart=on-failure
RestartSec=5
User=${USER}

[Install]
WantedBy=multi-user.target
EOF
    $SUDO systemctl daemon-reload
    $SUDO systemctl enable "$SERVICE_NAME"
    $SUDO systemctl start "$SERVICE_NAME"
    success "Service installed and started."
    info "Manage with: systemctl status|stop|restart $SERVICE_NAME"
  else
    info "Skipping service install."
  fi
}

# --------------------------------------------------------------------------
# Done — print summary
# --------------------------------------------------------------------------
print_summary() {
  echo ""
  echo -e "${GREEN}${BOLD}========================================${NC}"
  echo -e "${GREEN}${BOLD}   Installation complete!${NC}"
  echo -e "${GREEN}${BOLD}========================================${NC}"
  echo ""
  echo -e "  Config files:     ${BOLD}$CONFIG_DIR/${NC}"
  echo -e "  Data directory:   ${BOLD}$DATA_DIR/${NC}"
  echo -e "  Binary:           ${BOLD}$INSTALL_DIR/target/debug/$BINARY_NAME${NC}"
  echo ""
  echo -e "  To run manually:"
  echo -e "    ${BOLD}$INSTALL_DIR/target/debug/$BINARY_NAME $CONFIG_DIR/config.toml${NC}"
  echo ""
  echo -e "  Dashboards (once running):"
  echo -e "    Technical:  ${BOLD}http://localhost:7000${NC}"
  echo -e "    Homeowner:  ${BOLD}http://localhost:7000/home${NC}"
  echo ""
  echo -e "  Edit entity IDs anytime:"
  echo -e "    ${BOLD}nano $CONFIG_DIR/devices.toml${NC}"
  echo ""
}

# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------
main() {
  echo ""
  echo -e "${BOLD}  2red2blue Daemon Installer${NC}"
  echo ""

  detect_os
  require_sudo
  install_rust
  setup_repo
  create_dirs
  collect_config
  write_configs
  build_daemon
  install_service
  print_summary
}

main "$@"