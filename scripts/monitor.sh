#!/usr/bin/env bash
# AI Commander Service Monitor
# Watches logs for known failure patterns and takes corrective action.
#
# Usage:
#   ./scripts/monitor.sh          # run in foreground (Ctrl+C to stop)
#   ./scripts/monitor.sh install  # install as launchd agent (auto-start on login)
#   ./scripts/monitor.sh remove   # uninstall launchd agent
#   ./scripts/monitor.sh status   # show monitor + service health

set -euo pipefail

# ── config ────────────────────────────────────────────────────────────────────

HOME_DIR="${AI_COMMANDER_HOME:-$HOME/.ai-commander}"
BIN_DIR="$HOME_DIR/bin"
LOG_DIR="$HOME_DIR/logs"
TELEGRAM_LOG="$LOG_DIR/telegram.log"
MONITOR_LOG="$LOG_DIR/monitor.log"
MONITOR_PID_FILE="$HOME_DIR/state/monitor.pid"

TELEGRAM_LABEL="ai.commander.telegram"
DAEMON_LABEL="ai.commander.daemon"
LAUNCH_AGENTS_DIR="$HOME/Library/LaunchAgents"
MONITOR_PLIST="$LAUNCH_AGENTS_DIR/ai.commander.monitor.plist"
MONITOR_LABEL="ai.commander.monitor"

POLL_INTERVAL=10          # seconds between checks
LOG_TAIL_LINES=100        # lines to inspect per poll
# Cooldowns: don't repeat same action within N seconds
COOLDOWN_KILL_DEBUG=60
COOLDOWN_RESTART=90
COOLDOWN_LOG_NOISE=300    # suppress repeated "already healthy" messages

# ── helpers ───────────────────────────────────────────────────────────────────

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

ts() { date -u '+%Y-%m-%dT%H:%M:%SZ'; }

log_monitor() {
    local level="$1"; shift
    local line="[$(ts)] [$level] $*"
    # Append to log file explicitly
    echo "$line" >> "$MONITOR_LOG"
    # Only echo to stdout if it's a terminal (not launchd, which redirects stdout to the log)
    [[ -t 1 ]] && echo "$line" || true
}

info()  { log_monitor "INFO " "$@"; }
warn()  { log_monitor "WARN " "$@"; }
action(){ log_monitor "ACTION" "$@"; }
ok()    { log_monitor "OK   " "$@"; }

# ── cooldown tracking ─────────────────────────────────────────────────────────

# Cooldown state stored as timestamp files in state dir
_cooldown_dir=""

_init_cooldowns() {
    _cooldown_dir="$HOME_DIR/state/monitor-cooldowns"
    mkdir -p "$_cooldown_dir"
}

cooldown_ok() {
    local key="$1" cooldown="$2"
    local file="$_cooldown_dir/$key"
    [[ ! -f "$file" ]] && return 0
    local now; now=$(date +%s)
    local last; last=$(cat "$file" 2>/dev/null || echo 0)
    (( now - last >= cooldown ))
}

mark_action() {
    date +%s > "$_cooldown_dir/$1"
}

# ── remediation actions ───────────────────────────────────────────────────────

kill_stale_debug_processes() {
    local pids
    pids=$(pgrep -f "target/debug/commander-telegram" 2>/dev/null || true)
    pids+=" $(pgrep -f "target/debug/commander-daemon" 2>/dev/null || true)"
    pids=$(echo "$pids" | tr ' ' '\n' | grep -E '^[0-9]+$' | sort -u | tr '\n' ' ')

    if [[ -n "${pids// }" ]]; then
        action "Killing stale debug process(es): $pids"
        # shellcheck disable=SC2086
        kill $pids 2>/dev/null || true
        mark_action "kill_debug"
        return 0
    fi
    return 1
}

restart_telegram() {
    local uid; uid=$(id -u)
    action "Kickstarting $TELEGRAM_LABEL"
    launchctl kickstart -k "gui/$uid/$TELEGRAM_LABEL" 2>&1 | \
        while IFS= read -r line; do action "launchctl: $line"; done || true
    mark_action "restart_telegram"
    sleep 3   # give it time to settle before next check
}

restart_daemon() {
    local uid; uid=$(id -u)
    action "Kickstarting $DAEMON_LABEL"
    launchctl kickstart -k "gui/$uid/$DAEMON_LABEL" 2>&1 | \
        while IFS= read -r line; do action "launchctl: $line"; done || true
    mark_action "restart_daemon"
    sleep 3   # give it time to settle before next check
}

# ── daemon socket probe ───────────────────────────────────────────────────────

probe_daemon_socket() {
    local sock="$HOME_DIR/state/daemon.sock"
    [[ -S "$sock" ]] || { warn "Daemon socket missing: $sock"; return 1; }
    local response
    response=$(printf '{"jsonrpc":"2.0","method":"status.health","params":null,"id":99}\n' \
        | nc -U -w 3 "$sock" 2>/dev/null) || { warn "Daemon socket not accepting connections"; return 1; }
    echo "$response" | grep -q '"result"' || { warn "Daemon socket response invalid: $response"; return 1; }
    return 0
}

# ── launchd throttle detection ───────────────────────────────────────────────

check_launchd_throttle() {
    local label="$1"
    local result
    result=$(launchctl list "$label" 2>/dev/null) || return 0  # not loaded, handled elsewhere
    # If PID is missing (-) and LastExitStatus is non-zero, launchd has throttled it
    local pid exit_status
    pid=$(echo "$result" | grep '"PID"' | awk '{print $3}' | tr -d ',')
    exit_status=$(echo "$result" | grep '"LastExitStatus"' | awk '{print $3}' | tr -d ',')
    if [[ -z "$pid" ]] && [[ -n "$exit_status" ]] && [[ "$exit_status" != "0" ]]; then
        warn "launchd has throttled $label (LastExitStatus=$exit_status) — forcing kickstart"
        launchctl kickstart -k "gui/$(id -u)/$label" 2>/dev/null || true
    fi
}

# ── restart backoff ───────────────────────────────────────────────────────────

# Returns 0 (allow restart) or 1 (backoff, skip restart).
# Side-effect: increments the count file and emits warnings.
restart_backoff_ok() {
    local service="$1"
    local count_file="$_cooldown_dir/.${service}_restart_count"
    local window=600  # 10 minutes
    local now; now=$(date +%s)

    local window_start=0 count=0
    if [[ -f "$count_file" ]]; then
        read -r window_start count < "$count_file" 2>/dev/null || true
    fi

    # Reset count if last window started more than 10 minutes ago
    if (( now - window_start >= window )); then
        window_start=$now
        count=0
    fi

    count=$(( count + 1 ))
    printf '%s %s\n' "$window_start" "$count" > "$count_file"

    if (( count >= 8 )); then
        warn "Restart storm for $service ($count restarts in last 10 min) — manual intervention required"
        return 1
    elif (( count >= 5 )); then
        warn "Frequent restarts for $service ($count in 10 min) — holding off 15 min"
        mark_action "restart_${service}_hold"
        # Override the normal cooldown to 15 min
        printf '%s\n' "$now" > "$_cooldown_dir/restart_${service}"
        # We already incremented; caller must check cooldown separately — signal skip
        return 1
    elif (( count >= 3 )); then
        warn "Repeated restarts for $service ($count in 10 min) — holding off 5 min"
        return 1
    fi
    return 0
}

# Wraps cooldown_ok + restart_backoff_ok for a service restart decision.
# Usage: should_restart <service_key> <cooldown_seconds>
#   service_key is used both for the cooldown file and the backoff count file.
should_restart() {
    local key="$1" cooldown="$2"
    cooldown_ok "$key" "$cooldown" || return 1
    restart_backoff_ok "$key" || return 1
    return 0
}

# ── pattern checkers ──────────────────────────────────────────────────────────

# Count occurrences of a pattern in the last N lines of a file.
count_pattern() {
    local file="$1" pattern="$2" lines="$3"
    tail -n "$lines" "$file" 2>/dev/null | grep -c "$pattern" || true
}

# ── main check loop ───────────────────────────────────────────────────────────

check_once() {
    local healthy=true

    # ── 1. Stale debug processes ─────────────────────────────────────────────
    if pgrep -qf "target/debug/commander-telegram" 2>/dev/null || \
       pgrep -qf "target/debug/commander-daemon" 2>/dev/null; then
        warn "Stale debug binary is running (competing with installed service)"
        healthy=false
        if cooldown_ok "kill_debug" "$COOLDOWN_KILL_DEBUG"; then
            kill_stale_debug_processes
            sleep 2
            # After killing competitor, restart to pick up the connection
            if should_restart "restart_telegram" "$COOLDOWN_RESTART"; then
                restart_telegram
            fi
        fi
    fi

    # ── 2. TerminatedByOtherGetUpdates flood ─────────────────────────────────
    local terminated_count
    terminated_count=$(count_pattern "$TELEGRAM_LOG" "TerminatedByOtherGetUpdates" "$LOG_TAIL_LINES")
    if (( terminated_count >= 3 )); then
        warn "TerminatedByOtherGetUpdates x${terminated_count} in last ${LOG_TAIL_LINES} lines"
        healthy=false
        if cooldown_ok "kill_debug" "$COOLDOWN_KILL_DEBUG"; then
            kill_stale_debug_processes
            sleep 2
        fi
        if should_restart "restart_telegram" "$COOLDOWN_RESTART"; then
            restart_telegram
        fi
    fi

    # ── 3. Selector rate-limit spam loop ─────────────────────────────────────
    local rate_limit_count
    rate_limit_count=$(count_pattern "$TELEGRAM_LOG" "Failed to send selector.*Retry after" "$LOG_TAIL_LINES")
    if (( rate_limit_count >= 5 )); then
        warn "Selector rate-limit spam x${rate_limit_count} — restarting bot"
        healthy=false
        if should_restart "restart_telegram" "$COOLDOWN_RESTART"; then
            restart_telegram
        fi
    fi

    # ── 4. Process liveness check (reliable: is the right binary running?) ────────────
    local bot_pid
    bot_pid=$(pgrep -f "$BIN_DIR/commander-telegram" 2>/dev/null | head -1 || true)
    if [[ -z "$bot_pid" ]]; then
        warn "commander-telegram process not found — launchd may have failed to restart"
        healthy=false
        if should_restart "restart_telegram" "$COOLDOWN_RESTART"; then
            restart_telegram
        fi
    fi

    local poll_errors
    poll_errors=$(count_pattern "$TELEGRAM_LOG" "Error polling output" "$LOG_TAIL_LINES")
    if (( poll_errors >= 10 )); then
        warn "Excessive poll errors x${poll_errors} — restarting bot"
        healthy=false
        if should_restart "restart_telegram" "$COOLDOWN_RESTART"; then
            restart_telegram
        fi
    fi

    # ── 5. Daemon process + socket health ────────────────────────────────────
    local daemon_pid
    daemon_pid=$(pgrep -f "$BIN_DIR/commander-daemon" 2>/dev/null | head -1 || true)
    if [[ -n "$daemon_pid" ]]; then
        # Process exists — verify the socket is actually responsive
        if ! probe_daemon_socket; then
            warn "commander-daemon (PID $daemon_pid) socket is unresponsive"
            healthy=false
            if should_restart "restart_daemon" "$COOLDOWN_RESTART"; then
                restart_daemon
            fi
        fi
    else
        warn "commander-daemon process not found — launchd may have failed to restart"
        healthy=false
        if should_restart "restart_daemon" "$COOLDOWN_RESTART"; then
            restart_daemon
        fi
    fi

    # ── 6. launchd throttle detection ────────────────────────────────────────
    check_launchd_throttle "$DAEMON_LABEL"
    check_launchd_throttle "$TELEGRAM_LABEL"

    # ── 7. Report healthy ────────────────────────────────────────────────────
    if $healthy && cooldown_ok "log_healthy" "$COOLDOWN_LOG_NOISE"; then
        ok "All checks passed"
        mark_action "log_healthy"
    fi
}

# ── commands ──────────────────────────────────────────────────────────────────

cmd_run() {
    _init_cooldowns
    mkdir -p "$LOG_DIR" "$(dirname "$MONITOR_PID_FILE")"
    echo $$ > "$MONITOR_PID_FILE"
    info "Monitor started (PID=$$, poll every ${POLL_INTERVAL}s)"

    # Immediate first check
    check_once

    while true; do
        sleep "$POLL_INTERVAL"
        check_once
    done
}

cmd_status() {
    echo -e "\n${BOLD}AI Commander Monitor Status${NC}\n"

    # Monitor process
    if [[ -f "$MONITOR_PID_FILE" ]] && kill -0 "$(cat "$MONITOR_PID_FILE")" 2>/dev/null; then
        echo -e "  ${GREEN}●${NC} ${BOLD}monitor${NC}  (PID $(cat "$MONITOR_PID_FILE"))"
    elif launchctl list 2>/dev/null | grep -q "$MONITOR_LABEL"; then
        echo -e "  ${GREEN}●${NC} ${BOLD}monitor${NC}  (managed by launchd)"
    else
        echo -e "  ${RED}○${NC} ${BOLD}monitor${NC}  (not running)"
    fi

    # Stale debug processes
    local debug_pids
    debug_pids=$(pgrep -f "target/debug/commander-telegram" 2>/dev/null || true)
    if [[ -n "$debug_pids" ]]; then
        echo -e "  ${RED}!${NC} ${BOLD}stale debug process${NC}  (PID $debug_pids)"
    fi

    # Last few monitor actions
    if [[ -f "$MONITOR_LOG" ]]; then
        echo -e "\n${BOLD}Recent monitor activity:${NC}"
        grep -E "\[ACTION\]|\[WARN \]|\[OK   \]" "$MONITOR_LOG" 2>/dev/null | tail -10 | \
            sed 's/\[ACTION\]/⚡/; s/\[WARN \]/⚠️ /; s/\[OK   \]/✓ /'
    fi
    echo ""
}

cmd_install() {
    local uid; uid=$(id -u)
    local real_home="$HOME"
    local monitor_bin="$BIN_DIR/monitor.sh"

    # Install this script to ~/.ai-commander/bin/
    mkdir -p "$BIN_DIR"
    cp "${BASH_SOURCE[0]}" "$monitor_bin"
    chmod +x "$monitor_bin"
    echo -e "${GREEN}✓${NC} monitor.sh → $monitor_bin"

    # Write launchd plist
    mkdir -p "$LAUNCH_AGENTS_DIR"
    cat > "$MONITOR_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$MONITOR_LABEL</string>

    <key>ProgramArguments</key>
    <array>
        <string>$monitor_bin</string>
        <string>run</string>
    </array>

    <key>KeepAlive</key>
    <true/>

    <key>RunAtLoad</key>
    <true/>

    <key>WorkingDirectory</key>
    <string>$HOME_DIR</string>

    <key>StandardOutPath</key>
    <string>$MONITOR_LOG</string>

    <key>StandardErrorPath</key>
    <string>$MONITOR_LOG</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>$real_home</string>
        <key>PATH</key>
        <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>
</dict>
</plist>
EOF
    echo -e "${GREEN}✓${NC} plist → $MONITOR_PLIST"

    # Load it
    launchctl bootstrap "gui/$uid" "$MONITOR_PLIST" 2>/dev/null || \
        launchctl load "$MONITOR_PLIST" 2>/dev/null || true
    launchctl kickstart -k "gui/$uid/$MONITOR_LABEL" 2>/dev/null || true
    echo -e "${GREEN}✓${NC} Monitor agent loaded and started"
    echo ""
    echo -e "  ${BOLD}Monitor log:${NC} $MONITOR_LOG"
    echo -e "  ${BOLD}Remove:${NC}      $monitor_bin remove"
    echo ""
}

cmd_remove() {
    local uid; uid=$(id -u)
    launchctl kill SIGTERM "gui/$uid/$MONITOR_LABEL" 2>/dev/null || true
    launchctl bootout "gui/$uid" "$MONITOR_PLIST" 2>/dev/null || \
        launchctl unload "$MONITOR_PLIST" 2>/dev/null || true
    rm -f "$MONITOR_PLIST"
    echo -e "${GREEN}✓${NC} Monitor agent removed"
}

# ── dispatch ──────────────────────────────────────────────────────────────────

case "${1:-run}" in
    run)     cmd_run     ;;
    status)  cmd_status  ;;
    install) cmd_install ;;
    remove)  cmd_remove  ;;
    *)
        echo "Usage: $0 {run|status|install|remove}"
        exit 1
        ;;
esac
