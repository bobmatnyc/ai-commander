# Watchdog / Monitor Design for ai-commander

**Date:** 2026-03-26
**Status:** Research + Implementation Plan

---

## 1. Current State Audit

### 1.1 What is running right now

All four launchd agents are loaded and active:

| Label | PID | LastExitStatus | Notes |
|---|---|---|---|
| ai.commander.daemon | 59334 | 0 | Running, socket healthy |
| ai.commander.telegram | 59337 | 15 | Running (15 = last kill was SIGTERM — normal) |
| ai.commander.monitor | 44877 | 15 | Running |
| ai.commander.logrotate | (no PID shown) | — | Timer-based, not always resident |

The daemon socket at `~/.ai-commander/state/daemon.sock` is live and responding to JSON-RPC:

```
status.health response: {"status":"healthy","uptime_seconds":27006,"active_sessions":1,"version":"0.3.1"}
```

The monitor process (PID 44877) is alive and managed by launchd with `KeepAlive: true`.

### 1.2 monitor.sh — what it does

`scripts/monitor.sh` runs as `ai.commander.monitor` under launchd (KeepAlive), polls every 10 seconds, and checks:

1. **Stale debug binaries** — kills `target/debug/commander-*` processes that compete with the installed service.
2. **TerminatedByOtherGetUpdates flood** — detects Telegram long-poll conflict (>= 3 occurrences in last 30 log lines) and restarts telegram.
3. **Selector rate-limit spam** — detects Telegram rate-limit loop (>= 5 occurrences) and restarts telegram.
4. **Process liveness** — checks `pgrep -f "$BIN_DIR/commander-telegram"`. If the process is gone, kicks telegram.
5. **Poll errors** — detects "Error polling output" flood (>= 10) and restarts telegram.

Cooldowns are tracked with timestamp files in `~/.ai-commander/state/monitor-cooldowns/`.

Current monitor.log shows solid "All checks passed" entries every ~5 minutes (the `COOLDOWN_LOG_NOISE=300` gate). This means the monitor is running and not hitting errors, but it also means **it cannot tell you when it last did anything useful**.

### 1.3 What monitor.sh misses (the critical gaps)

**Gap 1: No IPC socket probe.**
The monitor only checks `pgrep` — process existence. A process can be fully running (pgrep passes) while the daemon's Unix socket is deadlocked, stuck in an async task, or has its `RwLock` writer held indefinitely. The daemon could be alive but completely unresponsive to all IPC. This is likely the core cause of "becoming unresponsive."

**Gap 2: No daemon monitoring at all.**
The monitor checks `commander-telegram` process existence but has zero checks on `commander-daemon`. There is no check that the daemon process is running, and no check that its socket accepts connections.

**Gap 3: No response-time / timeout check.**
Even if the socket connects, the monitor never verifies that a request-response round-trip completes within a reasonable time. A deadlocked daemon will accept the connection then never respond.

**Gap 4: Log-pattern detection is reactive, not proactive.**
The monitor detects failures only after they manifest as recognizable log patterns. Silent failures (deadlock, memory exhaustion causing OOM kill, launchd throttle) produce no log patterns.

**Gap 5: launchd throttle blindspot.**
launchd applies an exponential backoff when a service exits too quickly ("ThrottleInterval"). If the daemon is crash-looping, launchd may stop trying to restart it and the monitor will still see the process as missing but its `kickstart` calls will be silently no-ops until the throttle clears.

**Gap 6: No backoff awareness for the daemon.**
`restart_daemon()` exists in monitor.sh but is never called from `check_once()`. Only `restart_telegram` is actually wired up.

**Gap 7: Telegram pid file is stale.**
`~/.ai-commander/state/telegram.pid` has a timestamp of `Mar 13` — weeks old. The actual telegram process (PID 59337) is tracked by launchd, not by the pid file. `services.sh cmd_status` would show telegram as "stopped" even though it's running.

**Gap 8: Memory monitoring is a stub.**
`crates/commander-daemon/src/monitoring.rs` has a full `MemoryMonitor` but `get_process_memory()` is gated behind a `psutil` feature flag that is not enabled. It always returns zeros. No OOM early-warning exists.

### 1.4 daemon.sock health check — does anything probe it?

Nothing external probes the socket. The `services.sh daemon_running()` function calls `"$DAEMON_BIN" status` which itself connects over IPC — but this is only called from `cmd_status` and `cmd_start`, not from the monitor loop. No continuous socket health check exists.

The IPC server in `crates/commander-daemon/src/ipc/server.rs` implements `RpcMethod::StatusHealth` which returns a full health JSON. This is the correct probe target — it is already implemented and tested above.

---

## 2. Recommended Watchdog Design

### 2.1 Architecture decision: shell script vs Rust binary vs launchd KeepAlive

**Recommendation: Enhance the existing shell script (monitor.sh) with socket probing.**

Rationale:
- A shell script calling `nc -U` or `socat` for Unix socket probing is reliable, zero-dependency, and already fits the existing launchd agent pattern.
- Writing a new Rust watchdog binary is the right long-term answer but adds a compile step to the hot path; the script can be deployed in minutes.
- launchd `KeepAlive` alone only handles process-exit recovery, not socket-level health. It cannot restart a process that is still running but deadlocked.
- The monitor is already launchd-managed with KeepAlive, so it survives its own crashes.

**For a longer-term Rust watchdog binary:** The `RpcMethod::StatusHealth` endpoint is already implemented. A Rust watchdog could connect to the socket with a 3-second timeout, send the health probe, and parse the response. This would be more portable than shell + `nc`. File this as a future improvement.

### 2.2 Watchdog check sequence (per poll)

```
Every 15 seconds:
  1. [SOCKET PROBE] Connect to daemon.sock, send status.health, expect response in <3s
     - If connect fails or times out: daemon is unresponsive → restart daemon (with backoff)
     - If response contains "status":"healthy": proceed
  2. [DAEMON PROCESS] pgrep commander-daemon
     - If missing: kickstart daemon (launchd may have failed)
  3. [TELEGRAM PROCESS] pgrep commander-telegram (existing check)
  4. [EXISTING LOG PATTERN CHECKS] TerminatedByOtherGetUpdates, rate-limit, poll errors
  5. [LAUNCHD THROTTLE CHECK] `launchctl list` exit status for each label
     - If service shows no PID and LastExitStatus != 0: log throttle warning + wait longer
  6. [BACKOFF] If daemon restarted N times in last 10 minutes: pause restarts, alert only
```

### 2.3 Backoff strategy

Use a restart-count file per service (extending the existing cooldown mechanism):

- Restart count stored in `~/.ai-commander/state/monitor-cooldowns/restart_count_daemon`
- Count resets if the service has been stable for 10 minutes
- Restart allowed immediately for first 2 restarts
- After 3 restarts in 10 minutes: 5-minute wait before next restart attempt
- After 5 restarts in 10 minutes: 15-minute wait, log "possible crash loop detected"
- After 8 restarts: stop attempting, log "restart storm — manual intervention required"

This prevents the monitor from amplifying a crash loop.

---

## 3. Implementation Plan

### 3.1 Quick wins (30 minutes, shell changes only)

**File:** `scripts/monitor.sh` (also update installed copy at `~/.ai-commander/bin/monitor.sh`)

**Change 1: Add daemon socket probe function.**

```bash
probe_daemon_socket() {
    local sock="$HOME_DIR/state/daemon.sock"
    local timeout=3
    [[ -S "$sock" ]] || return 1  # socket file must exist
    # Send a JSON-RPC health request; expect a response within timeout seconds
    local response
    response=$(echo '{"jsonrpc":"2.0","method":"status.health","params":null,"id":99}' \
        | nc -U -w "$timeout" "$sock" 2>/dev/null) || return 1
    # Verify it looks like a valid JSON-RPC response
    echo "$response" | grep -q '"result"' || return 1
    return 0
}
```

**Change 2: Add daemon-specific check to `check_once()`.**

```bash
# -- Daemon socket health check --
if ! probe_daemon_socket; then
    warn "daemon.sock is unresponsive or missing"
    healthy=false
    if cooldown_ok "restart_daemon" "$COOLDOWN_RESTART"; then
        restart_daemon
    fi
else
    # Daemon socket OK — also verify the process exists
    local daemon_pid
    daemon_pid=$(pgrep -f "$BIN_DIR/commander-daemon" 2>/dev/null | head -1 || true)
    if [[ -z "$daemon_pid" ]]; then
        warn "commander-daemon process not found despite socket existing (stale socket?)"
        healthy=false
    fi
fi
```

**Change 3: Wire up `restart_daemon` inside the check loop (it currently exists but is never called).**

Already shown above.

**Change 4: Increase `LOG_TAIL_LINES` from 30 to 100.**
30 lines covers only ~30 seconds of telegram output at normal verbosity. Failures that unfold over a minute are missed.

**Change 5: Add launchd throttle detection.**

```bash
check_launchd_throttle() {
    local label="$1" name="$2"
    local info; info=$(launchctl list "$label" 2>/dev/null) || return
    # No PID means service isn't running
    echo "$info" | grep -q '"PID"' && return
    # Non-zero exit status suggests crash
    local exit_status; exit_status=$(echo "$info" | grep 'LastExitStatus' | grep -o '[0-9]*' | head -1)
    if [[ "${exit_status:-0}" -ne 0 && "${exit_status:-0}" -ne 15 ]]; then
        warn "$name crashed (LastExitStatus=$exit_status) — launchd may be throttling"
    fi
}
```

Call as:
```bash
check_launchd_throttle "$DAEMON_LABEL"   "commander-daemon"
check_launchd_throttle "$TELEGRAM_LABEL" "commander-telegram"
```

### 3.2 Medium-term improvements (hours of work)

**Backoff counter enhancement.**
Extend the cooldown files to store a restart count + window start time. The current system only stores the last-action timestamp; it cannot enforce "max N restarts per window."

Proposed state file format for `restart_count_daemon`:
```
<epoch_of_window_start> <restart_count>
```

**Heartbeat from the daemon itself.**
Add a background tokio task to `commander-daemon/src/service.rs` that writes a timestamp to `~/.ai-commander/state/daemon.heartbeat` every 30 seconds. The watchdog can then check `mtime` freshness as a secondary liveness indicator that doesn't require a full socket round-trip.

**Watchdog log rotation.**
`monitor.log` is not subject to the logrotate plist (which only rotates daemon.log and telegram.log). Add monitor.log to the logrotate plist or add rotation inside monitor.sh itself.

**Fix the stale telegram.pid file.**
`~/.ai-commander/state/telegram.pid` contains `44877` (the old monitor PID, or a very old telegram PID). Since launchd is now the authoritative process manager for telegram, the pid file approach in `services.sh` is obsolete. Either:
- Remove the pid file check in `cmd_status` and rely on `launchctl list` instead.
- Have `start-telegram.sh` write the correct PID.

### 3.3 Longer-term (Rust watchdog binary)

Create `crates/commander-watchdog/` as a minimal Rust binary that:
- Connects to `daemon.sock` with `tokio::net::UnixStream` and a 3-second connection timeout
- Sends `{"jsonrpc":"2.0","method":"status.health","params":null,"id":1}\n`
- Reads one line response with a 3-second read timeout
- Parses `"status":"healthy"` — exits 0 if healthy, exits 1 if not
- Can be called from shell script or from launchd directly

This gives a clean, testable probe that shell scripts can call as `commander-watchdog probe-daemon` and take action on the exit code. It is also more portable to Windows if the GUI ever needs it.

---

## 4. Summary Table

| Gap | Severity | Fix | Effort |
|---|---|---|---|
| No daemon socket probe | Critical | Add `probe_daemon_socket()` to monitor.sh | 30 min |
| restart_daemon never called | Critical | Wire into check_once() | 5 min |
| LOG_TAIL_LINES too small | Medium | Change 30 → 100 | 1 min |
| No backoff for daemon restarts | Medium | Extend cooldown to count restarts | 2 hours |
| No launchd throttle detection | Medium | Add `check_launchd_throttle()` | 30 min |
| Stale telegram.pid | Low | Fix cmd_status to use launchctl | 30 min |
| Memory monitoring is a stub | Low | Enable psutil feature or use sysinfo | 1 day |
| No daemon heartbeat file | Low | Add tokio task to write timestamp | 2 hours |
| monitor.log not rotated | Low | Add to logrotate plist | 15 min |

---

## 5. File Paths

| File | Role |
|---|---|
| `/Users/masa/Projects/ai-commander/scripts/monitor.sh` | Source monitor (edit here, then redeploy) |
| `/Users/masa/.ai-commander/bin/monitor.sh` | Installed copy (what launchd runs) |
| `/Users/masa/Library/LaunchAgents/ai.commander.monitor.plist` | Launchd agent for monitor |
| `/Users/masa/Library/LaunchAgents/ai.commander.daemon.plist` | Launchd agent for daemon |
| `/Users/masa/Library/LaunchAgents/ai.commander.telegram.plist` | Launchd agent for telegram |
| `/Users/masa/.ai-commander/state/daemon.sock` | IPC socket to probe |
| `/Users/masa/.ai-commander/state/monitor-cooldowns/` | Cooldown timestamp files |
| `/Users/masa/.ai-commander/logs/monitor.log` | Monitor output |
| `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/ipc/server.rs` | IPC server (StatusHealth handler) |
| `/Users/masa/Projects/ai-commander/crates/commander-daemon/src/monitoring.rs` | MemoryMonitor stub |

---

## 6. Deployment Steps for Quick Wins

After editing `scripts/monitor.sh`:

```bash
# Install updated monitor script
cp scripts/monitor.sh ~/.ai-commander/bin/monitor.sh
chmod +x ~/.ai-commander/bin/monitor.sh

# Reload the monitor launchd agent
uid=$(id -u)
launchctl kill SIGTERM "gui/$uid/ai.commander.monitor"
# launchd KeepAlive will restart it automatically within a few seconds

# Verify it started
launchctl list ai.commander.monitor
tail -f ~/.ai-commander/logs/monitor.log
```
