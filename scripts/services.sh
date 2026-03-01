#!/usr/bin/env bash
# scripts/services.sh - Start/stop/status for commander-daemon + commander-telegram
#
# Usage:
#   ./scripts/services.sh start    # start daemon + bot
#   ./scripts/services.sh stop     # stop both
#   ./scripts/services.sh restart  # restart both
#   ./scripts/services.sh status   # show status + recent logs
#   ./scripts/services.sh logs     # tail live logs

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/target/debug"

STATE_DIR="$HOME/.ai-commander/state"
LOG_DIR="$HOME/.ai-commander/logs"
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

# ── helpers ────────────────────────────────────────────────────────────────────
load_env() {
    # Config dir .env takes priority, then project .env.local
    local config_env="$HOME/.ai-commander/config/.env"
    local project_env="$PROJECT_ROOT/.env.local"
    [[ -f "$config_env"  ]] && set -a && source "$config_env"  && set +a
    [[ -f "$project_env" ]] && set -a && source "$project_env" && set +a
}

is_running() {          # is_running <pid_file>
    local pid_file="$1"
    [[ -f "$pid_file" ]] || return 1
    local pid; pid=$(cat "$pid_file")
    kill -0 "$pid" 2>/dev/null
}

stop_proc() {           # stop_proc <pid_file> <name>
    local pid_file="$1" name="$2"
    if is_running "$pid_file"; then
        local pid; pid=$(cat "$pid_file")
        info "Stopping $name (PID $pid)..."
        kill "$pid" 2>/dev/null || true
        # wait up to 5 s
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

start_proc() {          # start_proc <binary> <log_file> <pid_file> <name> [args...]
    local binary="$1" log_file="$2" pid_file="$3" name="$4"
    shift 4

    if is_running "$pid_file"; then
        local pid; pid=$(cat "$pid_file")
        warn "$name already running (PID $pid)"
        return
    fi

    [[ -x "$binary" ]] || { fail "$binary not found — run 'cargo build' first"; return 1; }

    mkdir -p "$(dirname "$log_file")" "$(dirname "$pid_file")"

    # Append a separator to the log so we can see restart boundaries
    { echo ""; echo "── started $(date) ──────────────────────"; } >> "$log_file" 2>/dev/null || true

    nohup "$binary" "$@" >> "$log_file" 2>&1 &
    local pid=$!
    echo "$pid" > "$pid_file"

    # Give it 2 s to confirm it's still alive
    sleep 2
    if kill -0 "$pid" 2>/dev/null; then
        ok "$name started (PID $pid)"
        ok "  log → $log_file"
    else
        fail "$name exited immediately — check log:"
        tail -20 "$log_file"
        rm -f "$pid_file"
        return 1
    fi
}

# ── commands ───────────────────────────────────────────────────────────────────
cmd_start() {
    load_env

    echo -e "\n${BOLD}Starting AI Commander services…${NC}\n"

    # 1. commander-daemon
    start_proc \
        "$BUILD_DIR/commander-daemon" \
        "$DAEMON_LOG" \
        "$DAEMON_PID" \
        "commander-daemon"

    # Small pause so socket is ready before bot probes it
    sleep 1

    # 2. commander-telegram
    start_proc \
        "$BUILD_DIR/commander-telegram" \
        "$TELEGRAM_LOG" \
        "$TELEGRAM_PID" \
        "commander-telegram" \
        -v

    echo ""
}

cmd_stop() {
    echo -e "\n${BOLD}Stopping AI Commander services…${NC}\n"
    stop_proc "$TELEGRAM_PID" "commander-telegram"
    stop_proc "$DAEMON_PID"   "commander-daemon"
    echo ""
}

cmd_restart() {
    cmd_stop
    sleep 1
    cmd_start
}

cmd_status() {
    load_env
    echo -e "\n${BOLD}AI Commander — Service Status${NC}\n"

    for pair in "commander-daemon:$DAEMON_PID" "commander-telegram:$TELEGRAM_PID"; do
        local name="${pair%%:*}" pid_file="${pair##*:}"
        if is_running "$pid_file"; then
            local pid; pid=$(cat "$pid_file")
            ok "$name  (PID $pid)"
        else
            fail "$name  (not running)"
        fi
    done

    echo ""
    if [[ -f "$DAEMON_LOG" ]]; then
        echo -e "${BLUE}daemon  (last 5 lines):${NC}"
        tail -5 "$DAEMON_LOG" | sed 's/^/  /'
        echo ""
    fi
    if [[ -f "$TELEGRAM_LOG" ]]; then
        echo -e "${BLUE}telegram (last 5 lines):${NC}"
        tail -5 "$TELEGRAM_LOG" | sed 's/^/  /'
        echo ""
    fi

    if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
        warn "TELEGRAM_BOT_TOKEN not set — add it to ~/.ai-commander/config/.env or .env.local"
    fi
}

cmd_logs() {
    echo -e "${BLUE}Tailing daemon + telegram logs (Ctrl-C to stop)…${NC}\n"
    tail -f "$DAEMON_LOG" "$TELEGRAM_LOG" 2>/dev/null \
        || { fail "No log files found yet — run './scripts/services.sh start' first"; exit 1; }
}

# ── dispatch ───────────────────────────────────────────────────────────────────
case "${1:-}" in
    start)   cmd_start   ;;
    stop)    cmd_stop    ;;
    restart) cmd_restart ;;
    status)  cmd_status  ;;
    logs)    cmd_logs    ;;
    *)
        echo -e "${BOLD}Usage:${NC} $0 {start|stop|restart|status|logs}"
        echo ""
        echo "  start    — build & run commander-daemon + commander-telegram"
        echo "  stop     — gracefully stop both"
        echo "  restart  — stop then start"
        echo "  status   — show PIDs + last 5 log lines each"
        echo "  logs     — tail both logs live"
        exit 1
        ;;
esac
