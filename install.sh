#!/usr/bin/env bash
set -euo pipefail

# ── Kaya Smart Home Daemon — Install Script ───────────────────────────────────
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/ddhadho/2red2blue/main/install.sh | bash
#   or
#   ./install.sh
#
# Supports: Linux (Debian/Ubuntu/Arch/OpenWrt)
# Requires: bash, curl, git

REPO_URL="https://github.com/ddhadho/2red2blue.git"
BINARY_NAME="daemon"
INSTALL_BIN="/usr/local/bin/kaya-daemon"
CONFIG_DIR="/etc/smarthome"
DATA_DIR="/var/lib/smarthome"
SERVICE_FILE="/etc/systemd/system/kaya.service"
KAYA_USER="kaya"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

info()    { echo -e "${GREEN}[kaya]${NC} $1"; }
warn()    { echo -e "${YELLOW}[warn]${NC} $1"; }
error()   { echo -e "${RED}[error]${NC} $1"; exit 1; }
prompt()  { echo -e "${BLUE}[?]${NC} $1"; }

# ── Banner ────────────────────────────────────────────────────────────────────

echo ""
echo -e "${GREEN}  ██╗  ██╗ █████╗ ██╗   ██╗ █████╗ ${NC}"
echo -e "${GREEN}  ██║ ██╔╝██╔══██╗╚██╗ ██╔╝██╔══██╗${NC}"
echo -e "${GREEN}  █████╔╝ ███████║ ╚████╔╝ ███████║${NC}"
echo -e "${GREEN}  ██╔═██╗ ██╔══██║  ╚██╔╝  ██╔══██║${NC}"
echo -e "${GREEN}  ██║  ██╗██║  ██║   ██║   ██║  ██║${NC}"
echo -e "${GREEN}  ╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝${NC}"
echo ""
echo -e "  Smart Home Daemon — Install Script"
echo -e "  https://github.com/ddhadho/2red2blue"
echo ""

# ── Root check ────────────────────────────────────────────────────────────────

if [[ $EUID -ne 0 ]]; then
    error "This script must be run as root. Try: sudo ./install.sh"
fi

# ── Detect OS ─────────────────────────────────────────────────────────────────

if [[ -f /etc/os-release ]]; then
    . /etc/os-release
    OS=$ID
else
    OS="unknown"
fi

info "Detected OS: ${OS}"

# ── Check dependencies ────────────────────────────────────────────────────────

check_cmd() {
    if ! command -v "$1" &> /dev/null; then
        return 1
    fi
    return 0
}

# Install curl if missing
if ! check_cmd curl; then
    warn "curl not found — installing"
    case $OS in
        ubuntu|debian) apt-get install -y curl ;;
        arch)          pacman -Sy --noconfirm curl ;;
        *)             error "Please install curl manually and re-run" ;;
    esac
fi

# Install git if missing
if ! check_cmd git; then
    warn "git not found — installing"
    case $OS in
        ubuntu|debian) apt-get install -y git ;;
        arch)          pacman -Sy --noconfirm git ;;
        *)             error "Please install git manually and re-run" ;;
    esac
fi

# ── Install Rust ──────────────────────────────────────────────────────────────

if ! check_cmd cargo; then
    info "Rust not found — installing via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet
    source "$HOME/.cargo/env"
    info "Rust installed"
else
    info "Rust found: $(cargo --version)"
fi

# Ensure cargo is on PATH
if ! check_cmd cargo; then
    source "$HOME/.cargo/env" 2>/dev/null || \
    source "/root/.cargo/env" 2>/dev/null || \
    error "Could not find cargo after install. Open a new terminal and re-run."
fi

# ── Collect configuration ─────────────────────────────────────────────────────

echo ""
echo "─────────────────────────────────────────"
echo "  Home Assistant Configuration"
echo "─────────────────────────────────────────"
echo ""

prompt "Home Assistant URL (e.g. http://localhost:8123):"
read -r HA_URL
HA_URL="${HA_URL%/}"  # strip trailing slash

prompt "Home Assistant long-lived access token:"
read -r -s HA_TOKEN
echo ""

prompt "Install as systemd service? (starts on boot) [y/N]:"
read -r INSTALL_SERVICE
INSTALL_SERVICE="${INSTALL_SERVICE,,}"

echo ""

# ── Clone or update repo ──────────────────────────────────────────────────────

WORK_DIR="/tmp/kaya-install"

if [[ -d "$WORK_DIR" ]]; then
    info "Updating existing source"
    cd "$WORK_DIR"
    git pull --quiet
else
    info "Cloning repository"
    git clone --quiet "$REPO_URL" "$WORK_DIR"
    cd "$WORK_DIR"
fi

# ── Build ─────────────────────────────────────────────────────────────────────

info "Building daemon (this takes a few minutes on first run)"
cargo build --release --bin daemon 2>&1 | tail -5

info "Build complete"

# ── Create directories ────────────────────────────────────────────────────────

info "Creating directories"
mkdir -p "$CONFIG_DIR"
mkdir -p "$DATA_DIR/wal"
mkdir -p "$DATA_DIR/snapshots"

# ── Write config files ────────────────────────────────────────────────────────

info "Writing config files"

# Only write config.toml if it doesn't exist — don't overwrite existing config
if [[ ! -f "$CONFIG_DIR/config.toml" ]]; then
    cat > "$CONFIG_DIR/config.toml" << EOF
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
reconnect_interval_seconds = 5
event_dedup_window_ms = 500

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
url   = "${HA_URL}"
token = "${HA_TOKEN}"

EOF
    info "config.toml written"
else
    warn "config.toml already exists — skipping (update HA token manually if needed)"
fi

# Write empty rules.toml if missing
if [[ ! -f "$CONFIG_DIR/rules.toml" ]]; then
    cat > "$CONFIG_DIR/rules.toml" << 'EOF'
# Kaya automation rules
# See docs/technical-design/rule-dsl/ for the full DSL reference
EOF
    info "rules.toml written (empty — add your rules here)"
fi

# Write empty devices.toml if missing
if [[ ! -f "$CONFIG_DIR/devices.toml" ]]; then
    cat > "$CONFIG_DIR/devices.toml" << 'EOF'
# Kaya device registry
# Map your Home Assistant entity_ids to internal device IDs
#
# Example:
# [[devices]]
# id                      = "main_gate"
# external_id             = "input_boolean.main_gate"
# name                    = "Main Gate"
# kind                    = "Gate"
# confidence_decay_seconds = 300
#
# [[devices.safe_default]]
# attribute = "state"
# value     = "locked"
EOF
    info "devices.toml written (empty — add your devices here)"
fi

# ── Install binary ────────────────────────────────────────────────────────────

info "Installing binary to $INSTALL_BIN"
cp "$WORK_DIR/target/release/$BINARY_NAME" "$INSTALL_BIN"
chmod +x "$INSTALL_BIN"

# ── Create system user ────────────────────────────────────────────────────────

if [[ "$INSTALL_SERVICE" == "y" ]]; then
    if ! id "$KAYA_USER" &>/dev/null; then
        info "Creating system user: $KAYA_USER"
        useradd --system --no-create-home --shell /usr/sbin/nologin "$KAYA_USER"
    fi

    chown -R "$KAYA_USER:$KAYA_USER" "$DATA_DIR"
    chown -R "$KAYA_USER:$KAYA_USER" "$CONFIG_DIR"

    # ── Write systemd service ─────────────────────────────────────────────────

    info "Installing systemd service"
    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=Kaya Smart Home Daemon
After=network.target
Wants=network.target

[Service]
Type=simple
User=$KAYA_USER
ExecStart=$INSTALL_BIN $CONFIG_DIR/config.toml
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable kaya
    systemctl start kaya

    echo ""
    info "Kaya daemon installed and started"
    echo ""
    echo "  Useful commands:"
    echo "    sudo systemctl status kaya     — check status"
    echo "    sudo journalctl -u kaya -f     — follow logs"
    echo "    sudo systemctl restart kaya    — restart"
    echo "    sudo systemctl stop kaya       — stop"
    echo ""
else
    chown -R "$USER:$USER" "$DATA_DIR" 2>/dev/null || true
    chown -R "$USER:$USER" "$CONFIG_DIR" 2>/dev/null || true

    echo ""
    info "Kaya daemon installed"
    echo ""
    echo "  Run manually:"
    echo "    $INSTALL_BIN $CONFIG_DIR/config.toml"
    echo ""
fi

# ── Next steps ────────────────────────────────────────────────────────────────

echo "─────────────────────────────────────────"
echo "  Next steps"
echo "─────────────────────────────────────────"
echo ""
echo "  1. Add your devices to $CONFIG_DIR/devices.toml"
echo "  2. Add your rules to  $CONFIG_DIR/rules.toml"
echo "  3. Open the dashboard: http://localhost:7000"
echo "  4. Open the home view: http://localhost:7000/home"
echo ""
echo "  Docs: https://github.com/ddhadho/2red2blue/tree/main/docs"
echo ""