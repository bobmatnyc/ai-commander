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
#   ~/.ai-commander/services.sh launchd-install
#   ~/.ai-commander/services.sh launchd-uninstall
#   ~/.ai-commander/services.sh rotate-logs

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

# ── launchd paths ──────────────────────────────────────────────────────────────
LAUNCH_AGENTS_DIR="$HOME/Library/LaunchAgents"
DAEMON_PLIST_DEST="$LAUNCH_AGENTS_DIR/ai.commander.daemon.plist"
TELEGRAM_PLIST_DEST="$LAUNCH_AGENTS_DIR/ai.commander.telegram.plist"
LOGROTATE_PLIST_DEST="$LAUNCH_AGENTS_DIR/ai.commander.logrotate.plist"
DAEMON_LABEL="ai.commander.daemon"
TELEGRAM_LABEL="ai.commander.telegram"
LOGROTATE_LABEL="ai.commander.logrotate"

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

# ── launchd helpers ────────────────────────────────────────────────────────────

# Returns 0 if the given launchd label is loaded (registered with the session).
launchd_is_loaded() {
    local label="$1"
    launchctl list "$label" &>/dev/null
}

# Bootstrap a single plist into the user's GUI session.
launchd_load() {
    local plist="$1" label="$2"
    local uid; uid=$(id -u)

    if launchd_is_loaded "$label"; then
        info "Unloading existing $label before reload…"
        launchctl bootout "gui/$uid/$label" 2>/dev/null || \
            launchctl bootout "gui/$uid" "$plist" 2>/dev/null || true
        sleep 0.5
    fi

    launchctl bootstrap "gui/$uid" "$plist"
    ok "Loaded $label"
}

# Remove a single label from the user's GUI session.
launchd_unload() {
    local label="$1"
    local uid; uid=$(id -u)

    if launchd_is_loaded "$label"; then
        launchctl bootout "gui/$uid/$label" 2>/dev/null && ok "Unloaded $label" || \
            warn "Could not unload $label (may already be gone)"
    else
        warn "$label not loaded"
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

    # Install launchd agents (if the launchd/ templates exist next to this script)
    local launchd_src="$script_path/launchd"
    if [[ -d "$launchd_src" ]]; then
        echo ""
        cmd_launchd_install_from "$launchd_src"
    else
        warn "No scripts/launchd/ directory found — skipping launchd setup"
        warn "Services will rely on manual start/stop only"
    fi

    echo ""
    echo -e "${BOLD}Done. Run services from anywhere:${NC}"
    echo "  $SELF_INSTALL start"
    echo "  $SELF_INSTALL status"
    echo "  $SELF_INSTALL logs"
    echo ""
}

# Internal helper: install launchd agents from a given source directory.
# Separated so it can be called from both cmd_install and cmd_launchd_install.
cmd_launchd_install_from() {
    local launchd_src="$1"

    echo -e "\n${BOLD}Setting up launchd boot persistence…${NC}\n"

    # Install wrapper scripts
    local wrapper_daemon="$launchd_src/start-daemon.sh"
    local wrapper_telegram="$launchd_src/start-telegram.sh"

    if [[ -f "$wrapper_daemon" ]]; then
        cp "$wrapper_daemon" "$BIN_DIR/start-daemon.sh"
        chmod +x "$BIN_DIR/start-daemon.sh"
        ok "start-daemon.sh    → $BIN_DIR/start-daemon.sh"
    else
        warn "start-daemon.sh not found in $launchd_src — skipping"
    fi

    if [[ -f "$wrapper_telegram" ]]; then
        cp "$wrapper_telegram" "$BIN_DIR/start-telegram.sh"
        chmod +x "$BIN_DIR/start-telegram.sh"
        ok "start-telegram.sh  → $BIN_DIR/start-telegram.sh"
    else
        warn "start-telegram.sh not found in $launchd_src — skipping"
    fi

    mkdir -p "$LAUNCH_AGENTS_DIR"

    # Copy and substitute HOME placeholders in plists
    local plists=(
        "ai.commander.daemon.plist:$DAEMON_PLIST_DEST"
        "ai.commander.telegram.plist:$TELEGRAM_PLIST_DEST"
        "ai.commander.logrotate.plist:$LOGROTATE_PLIST_DEST"
    )

    for entry in "${plists[@]}"; do
        local src_name="${entry%%:*}"
        local dest="${entry##*:}"
        local src="$launchd_src/$src_name"

        if [[ ! -f "$src" ]]; then
            warn "$src not found — skipping"
            continue
        fi

        # Substitute HOME_DIR and REAL_HOME placeholders with actual paths
        sed \
            -e "s|HOME_DIR|$HOME_DIR|g" \
            -e "s|REAL_HOME|$HOME|g" \
            "$src" > "$dest"

        ok "$src_name → $dest"
    done

    # Load all three agents
    echo ""
    info "Loading launchd agents…"
    [[ -f "$DAEMON_PLIST_DEST"    ]] && launchd_load "$DAEMON_PLIST_DEST"    "$DAEMON_LABEL"
    [[ -f "$TELEGRAM_PLIST_DEST"  ]] && launchd_load "$TELEGRAM_PLIST_DEST"  "$TELEGRAM_LABEL"
    [[ -f "$LOGROTATE_PLIST_DEST" ]] && launchd_load "$LOGROTATE_PLIST_DEST" "$LOGROTATE_LABEL"

    echo ""
    ok "launchd agents installed — services will auto-start on login and restart on crash"
}

cmd_launchd_install() {
    local script_path; script_path="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local launchd_src

    # Locate the launchd templates — check same dir, scripts/ sub-dir, or project scripts/
    if [[ -d "$script_path/launchd" ]]; then
        launchd_src="$script_path/launchd"
    elif [[ -d "$script_path/../scripts/launchd" ]]; then
        launchd_src="$(cd "$script_path/../scripts/launchd" && pwd)"
    else
        fail "Cannot find scripts/launchd/ directory"
        exit 1
    fi

    mkdir -p "$BIN_DIR" "$LAUNCH_AGENTS_DIR" "$LOG_DIR" "$STATE_DIR"
    cmd_launchd_install_from "$launchd_src"
}

cmd_launchd_uninstall() {
    echo -e "\n${BOLD}Removing launchd agents…${NC}\n"

    launchd_unload "$DAEMON_LABEL"
    launchd_unload "$TELEGRAM_LABEL"
    launchd_unload "$LOGROTATE_LABEL"

    for plist in "$DAEMON_PLIST_DEST" "$TELEGRAM_PLIST_DEST" "$LOGROTATE_PLIST_DEST"; do
        if [[ -f "$plist" ]]; then
            rm -f "$plist"
            ok "Removed $plist"
        fi
    done

    echo ""
    ok "launchd agents removed — services will no longer start automatically"
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

    # Prefer launchd if agents are loaded
    if launchd_is_loaded "$DAEMON_LABEL"; then
        info "launchd is managing services — using launchctl kickstart"
        local uid; uid=$(id -u)
        launchctl kickstart -k "gui/$uid/$DAEMON_LABEL"   && ok "commander-daemon kickstarted" || \
            warn "commander-daemon kickstart returned non-zero (may already be running)"
        launchctl kickstart -k "gui/$uid/$TELEGRAM_LABEL" && ok "commander-telegram kickstarted" || \
            warn "commander-telegram kickstart returned non-zero (may already be running)"
        echo ""
        return
    fi

    # Fallback: nohup-based start
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

    # Prefer launchd if agents are loaded
    if launchd_is_loaded "$DAEMON_LABEL"; then
        info "launchd is managing services — using launchctl kill"
        local uid; uid=$(id -u)
        launchctl kill SIGTERM "gui/$uid/$TELEGRAM_LABEL" 2>/dev/null && \
            ok "Sent SIGTERM to commander-telegram" || \
            warn "commander-telegram: launchctl kill returned error (may not be running)"
        launchctl kill SIGTERM "gui/$uid/$DAEMON_LABEL" 2>/dev/null && \
            ok "Sent SIGTERM to commander-daemon" || \
            warn "commander-daemon: launchctl kill returned error (may not be running)"
        echo ""
        return
    fi

    # Fallback: PID-based stop
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

    # Show launchd management status
    if launchd_is_loaded "$DAEMON_LABEL"; then
        echo -e "  ${GREEN}launchd${NC}: agents loaded (boot-persistent, auto-restart enabled)"
    else
        echo -e "  ${YELLOW}launchd${NC}: agents not loaded (manual start/stop only)"
    fi
    echo ""

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

cmd_rotate_logs() {
    local max_bytes=$((10 * 1024 * 1024))  # 10 MB
    local rotated=0

    rotate_one() {
        local log="$1"
        if [[ ! -f "$log" ]]; then
            return
        fi
        local size; size=$(stat -f%z "$log" 2>/dev/null || stat -c%s "$log" 2>/dev/null || echo 0)
        if (( size > max_bytes )); then
            info "Rotating $log ($(( size / 1024 / 1024 )) MB)…"
            rm -f "${log}.1"
            mv "$log" "${log}.1"
            touch "$log"
            ok "Rotated → ${log}.1"
            rotated=$(( rotated + 1 ))
        fi
    }

    mkdir -p "$LOG_DIR"
    rotate_one "$DAEMON_LOG"
    rotate_one "$TELEGRAM_LOG"

    if (( rotated == 0 )); then
        info "No logs exceed 10 MB — nothing to rotate"
    else
        ok "$rotated log(s) rotated"
    fi
}

# ── dispatch ───────────────────────────────────────────────────────────────────
case "${1:-}" in
    install)          cmd_install          ;;
    start)            cmd_start            ;;
    stop)             cmd_stop             ;;
    restart)          cmd_restart          ;;
    status)           cmd_status           ;;
    logs)             cmd_logs             ;;
    launchd-install)  cmd_launchd_install  ;;
    launchd-uninstall) cmd_launchd_uninstall ;;
    rotate-logs)      cmd_rotate_logs      ;;
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
        echo ""
        echo -e "  ${BOLD}launchd (boot persistence + crash recovery):${NC}"
        echo "    ~/.ai-commander/services.sh launchd-install    # register agents"
        echo "    ~/.ai-commander/services.sh launchd-uninstall  # remove agents"
        echo ""
        echo -e "  ${BOLD}Maintenance:${NC}"
        echo "    ~/.ai-commander/services.sh rotate-logs  # rotate logs > 10 MB"
        exit 1
        ;;
esac
