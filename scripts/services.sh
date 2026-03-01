#!/usr/bin/env bash
# scripts/services.sh — AI Commander service manager
#
# After first run of `install`, the script copies itself to ~/.ai-commander/
# and all commands work from there, independent of any project directory.
#
# Usage (from project root, first time):
#   ./scripts/services.sh install   # build + install binaries + self
#
# Usage (anywhere, after install):
#   ~/.ai-commander/services.sh start
#   ~/.ai-commander/services.sh stop
#   ~/.ai-commander/services.sh restart
#   ~/.ai-commander/services.sh status
#   ~/.ai-commander/services.sh logs

set -euo pipefail

# ── paths (all under ~/.ai-commander) ─────────────────────────────────────────
HOME_DIR="$HOME/.ai-commander"
BIN_DIR="$HOME_DIR/bin"
LOG_DIR="$HOME_DIR/logs"
STATE_DIR="$HOME_DIR/state"
CONFIG_DIR="$HOME_DIR/config"
SELF_INSTALL="$HOME_DIR/services.sh"

DAEMON_BIN="$BIN_DIR/commander-daemon"
TELEGRAM_BIN="$BIN_DIR/commander-telegram"
DAEMON_PID="$STATE_DIR/daemon.pid"
TELEGRAM_PID="$STATE_DIR/telegram.pid"
DAEMON_LOG="$LOG_DIR/daemon.log"
TELEGRAM_LOG="$LOG_DIR/telegram.log"

# ── colours ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓${NC} $*"; }
info() { echo -e "${BLUE}→${NC} $*"; }
warn() { echo -e "${YELLOW}⚠${NC} $*"; }
fail() { echo -e "${RED}✗${NC} $*"; }

# ── env loading ────────────────────────────────────────────────────────────────
load_env() {
    local config_env="$CONFIG_DIR/.env"
    local project_env
    # Walk up from cwd looking for .env.local (useful when running from project)
    project_env="$(pwd)/.env.local"

    [[ -f "$config_env"  ]] && set -a && source "$config_env"  && set +a
    [[ -f "$project_env" ]] && set -a && source "$project_env" && set +a
}

# ── process helpers ────────────────────────────────────────────────────────────
is_running() {
    local pid_file="$1"
    [[ -f "$pid_file" ]] || return 1
    local pid; pid=$(cat "$pid_file")
    kill -0 "$pid" 2>/dev/null
}

stop_proc() {
    local pid_file="$1" name="$2"
    if is_running "$pid_file"; then
        local pid; pid=$(cat "$pid_file")
        info "Stopping $name (PID $pid)…"
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 1 10); do
            kill -0 "$pid" 2>/dev/null || { ok "$name stopped"; rm -f "$pid_file"; return; }
            sleep 0.5
        done
        kill -9 "$pid" 2>/dev/null || true
        rm -f "$pid_file"
        ok "$name force-killed"
    else
        warn "$name not running"
        rm -f "$pid_file" 2>/dev/null || true
    fi
}

start_proc() {
    local binary="$1" log_file="$2" pid_file="$3" name="$4"
    shift 4

    if is_running "$pid_file"; then
        warn "$name already running (PID $(cat "$pid_file"))"
        return
    fi

    if [[ ! -x "$binary" ]]; then
        fail "$binary not found — run: $SELF_INSTALL install"
        return 1
    fi

    mkdir -p "$(dirname "$log_file")" "$(dirname "$pid_file")"
    { echo ""; echo "── started $(date) ──────────────────────"; } >> "$log_file" 2>/dev/null || true

    nohup "$binary" "$@" >> "$log_file" 2>&1 &
    local pid=$!
    echo "$pid" > "$pid_file"

    sleep 2
    if kill -0 "$pid" 2>/dev/null; then
        ok "$name started (PID $pid)"
        info "  log → $log_file"
    else
        fail "$name exited immediately — check log:"
        tail -20 "$log_file"
        rm -f "$pid_file"
        return 1
    fi
}

# ── commands ───────────────────────────────────────────────────────────────────
cmd_install() {
    # Find the project root (script may live in scripts/ or in ~/.ai-commander/)
    local script_path; script_path="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local project_root

    if [[ -f "$script_path/Cargo.toml" ]]; then
        project_root="$script_path"           # script is at project root
    elif [[ -f "$script_path/../Cargo.toml" ]]; then
        project_root="$(cd "$script_path/.." && pwd)"   # script is in scripts/
    else
        fail "Cannot find project Cargo.toml — run this from the ai-commander repo"
        exit 1
    fi

    echo -e "\n${BOLD}Installing AI Commander services…${NC}\n"
    info "Project root: $project_root"
    info "Install dir:  $BIN_DIR"

    # Build release binaries
    info "Building commander-daemon and commander-telegram (release)…"
    cargo build --release \
        -p commander-daemon \
        -p commander-telegram \
        --manifest-path "$project_root/Cargo.toml" 2>&1 | grep -E "^(error|warning|Compiling|Finished)" || true

    # Install binaries
    mkdir -p "$BIN_DIR"
    cp "$project_root/target/release/commander-daemon"   "$DAEMON_BIN"
    cp "$project_root/target/release/commander-telegram" "$TELEGRAM_BIN"
    chmod +x "$DAEMON_BIN" "$TELEGRAM_BIN"
    ok "commander-daemon   → $DAEMON_BIN"
    ok "commander-telegram → $TELEGRAM_BIN"

    # Install this script
    mkdir -p "$HOME_DIR"
    cp "${BASH_SOURCE[0]}" "$SELF_INSTALL"
    chmod +x "$SELF_INSTALL"
    ok "services.sh        → $SELF_INSTALL"

    # Ensure config dir exists with a template .env if missing
    mkdir -p "$CONFIG_DIR" "$LOG_DIR" "$STATE_DIR"
    if [[ ! -f "$CONFIG_DIR/.env" ]]; then
        cat > "$CONFIG_DIR/.env" <<'EOF'
# AI Commander — service configuration
# Add your tokens here; this file is outside any project repo.

TELEGRAM_BOT_TOKEN=
OPENROUTER_API_KEY=
NGROK_AUTHTOKEN=
EOF
        warn "Created $CONFIG_DIR/.env — add your TELEGRAM_BOT_TOKEN"
    fi

    echo ""
    echo -e "${BOLD}Done. Run services from anywhere:${NC}"
    echo "  $SELF_INSTALL start"
    echo "  $SELF_INSTALL status"
    echo "  $SELF_INSTALL logs"
    echo ""
}

daemon_running() {
    "$DAEMON_BIN" status 2>/dev/null | grep -q '"running": true'
}

cmd_start() {
    load_env

    if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
        fail "TELEGRAM_BOT_TOKEN not set — add it to $CONFIG_DIR/.env"
        exit 1
    fi

    echo -e "\n${BOLD}Starting AI Commander services…${NC}\n"

    # commander-daemon: self-daemonizing, manages its own PID file
    if [[ ! -x "$DAEMON_BIN" ]]; then
        fail "$DAEMON_BIN not found — run: $SELF_INSTALL install"; exit 1
    fi
    if daemon_running; then
        warn "commander-daemon already running"
    else
        info "Starting commander-daemon…"
        "$DAEMON_BIN" start >> "$DAEMON_LOG" 2>&1 && ok "commander-daemon started" \
            || { fail "commander-daemon failed to start — check $DAEMON_LOG"; exit 1; }
    fi

    sleep 1   # let socket appear before bot probes it

    # commander-telegram: foreground process, we track the PID
    start_proc "$TELEGRAM_BIN" "$TELEGRAM_LOG" "$TELEGRAM_PID" "commander-telegram" -v

    echo ""
}

cmd_stop() {
    echo -e "\n${BOLD}Stopping AI Commander services…${NC}\n"
    stop_proc "$TELEGRAM_PID" "commander-telegram"

    # commander-daemon: use its own stop command
    if [[ -x "$DAEMON_BIN" ]] && daemon_running; then
        info "Stopping commander-daemon…"
        "$DAEMON_BIN" stop >> "$DAEMON_LOG" 2>&1 && ok "commander-daemon stopped" \
            || warn "commander-daemon stop returned error (may already be stopped)"
    else
        warn "commander-daemon not running"
    fi
    echo ""
}

cmd_restart() {
    cmd_stop
    sleep 1
    cmd_start
}

cmd_status() {
    load_env
    echo -e "\n${BOLD}AI Commander — Status${NC}\n"

    # commander-daemon: query its own status
    if [[ ! -x "$DAEMON_BIN" ]]; then
        echo -e "  ${RED}○${NC} ${BOLD}commander-daemon${NC}  (not installed — run: $SELF_INSTALL install)"
    elif daemon_running; then
        local pid; pid=$("$DAEMON_BIN" status 2>/dev/null | grep '"pid"' | grep -o '[0-9]*' | head -1)
        echo -e "  ${GREEN}●${NC} ${BOLD}commander-daemon${NC}  (PID ${pid:-?})"
    else
        echo -e "  ${RED}○${NC} ${BOLD}commander-daemon${NC}  (stopped)"
    fi

    # commander-telegram: PID file tracking
    if [[ ! -x "$TELEGRAM_BIN" ]]; then
        echo -e "  ${RED}○${NC} ${BOLD}commander-telegram${NC}  (not installed — run: $SELF_INSTALL install)"
    elif is_running "$TELEGRAM_PID"; then
        echo -e "  ${GREEN}●${NC} ${BOLD}commander-telegram${NC}  (PID $(cat "$TELEGRAM_PID"))"
    else
        echo -e "  ${RED}○${NC} ${BOLD}commander-telegram${NC}  (stopped)"
    fi

    echo ""
    for pair in "daemon:$DAEMON_LOG" "telegram:$TELEGRAM_LOG"; do
        local label="${pair%%:*}" log="${pair##*:}"
        if [[ -f "$log" ]]; then
            echo -e "${BLUE}$label log (last 5):${NC}"
            tail -5 "$log" | sed 's/^/  /'
            echo ""
        fi
    done

    if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
        warn "TELEGRAM_BOT_TOKEN not set — edit $CONFIG_DIR/.env"
    fi
}

cmd_logs() {
    echo -e "${BLUE}Tailing logs (Ctrl-C to stop)…${NC}\n"
    tail -f "$DAEMON_LOG" "$TELEGRAM_LOG" 2>/dev/null \
        || { fail "No log files yet — run: $SELF_INSTALL start"; exit 1; }
}

# ── dispatch ───────────────────────────────────────────────────────────────────
case "${1:-}" in
    install) cmd_install ;;
    start)   cmd_start   ;;
    stop)    cmd_stop    ;;
    restart) cmd_restart ;;
    status)  cmd_status  ;;
    logs)    cmd_logs    ;;
    *)
        echo -e "${BOLD}AI Commander service manager${NC}"
        echo ""
        echo -e "  ${BOLD}First time:${NC}"
        echo "    ./scripts/services.sh install   # build + install to ~/.ai-commander/"
        echo ""
        echo -e "  ${BOLD}After install (run from anywhere):${NC}"
        echo "    ~/.ai-commander/services.sh start"
        echo "    ~/.ai-commander/services.sh stop"
        echo "    ~/.ai-commander/services.sh restart"
        echo "    ~/.ai-commander/services.sh status"
        echo "    ~/.ai-commander/services.sh logs"
        exit 1
        ;;
esac
