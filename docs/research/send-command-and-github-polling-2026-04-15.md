# Research: /send command gap and GitHub polling

Date: 2026-04-15

---

## 1. /send command gap

### Verdict: No gap in TUI or REPL — `/send` IS registered and handled

Both the REPL and TUI implement `/send`. The "Unknown command: /send" message can only come from one specific path.

### Where slash commands are registered / dispatched

**REPL**

- Enum: `crates/ai-commander/src/repl.rs`, line 569 — `pub enum ReplCommand`
- Variant: line 581 — `Send(String)`
- Parser: lines 653–655 — `"send" => arg.map(ReplCommand::Send).unwrap_or(ReplCommand::Unknown(...))`
- Handler: lines 1154–1240+ — `ReplCommand::Send(message) => { ... tmux.send_line(...) ... }`
- Unknown fallback: lines 1404–1409 — `ReplCommand::Unknown(cmd) => println!("Unknown command: /{}. ...", cmd)`

The only way "Unknown command: /send" fires in the REPL is if the user types `/send` with no argument — the parser converts it to `ReplCommand::Unknown("send requires a message".to_string())`.

**TUI**

- Dispatcher: `crates/ai-commander/src/tui/commands.rs`, lines 203–211
- Match arm: `"send" => { if let Some(message) = arg { self.send_message(message) } else { Usage message } }`
- Unknown fallback: line 222–224 — `_ => self.messages.push(Message::system(format!("Unknown command: /{}", command)))`
- Input entry point: `crates/ai-commander/src/tui/input.rs`, line 63–64 — strips `/` prefix and calls `handle_command(cmd)`

### How existing commands like /connect, /status, /stop work

All commands follow the same pattern in `tui/commands.rs`:

- `"connect" | "c"` (line 71) — parses `ConnectArgs`, calls `self.connect()` or `self.connect_new()`
- `"status" | "s"` (line 197) — calls `self.show_status(arg)`
- `"stop"` (line 155) — resolves target session, calls `self.stop_session(&name)`

### Intended behavior of /send

`/send <message>` sends a literal string directly to the active tmux session via `tmux send-keys`. It bypasses all command parsing and filesystem-command handling — the string goes directly to whatever process is running in the pane.

In the TUI, `send_message()` is in `crates/ai-commander/src/tui/messaging.rs` starting at line 16. It:
1. Resolves the connected project → session name
2. Calls `tmux.send_line(&session, None, &message)` — this is a raw `tmux send-keys` call
3. Polls for output changes (250 ms intervals, 60 s max, 3 s idle timeout)

In the REPL, the same logic is inline in `repl.rs` starting at line 1161.

### Root cause of "Unknown command: /send"

Two possible causes:
1. User typed `/send` with no argument — REPL converts to `ReplCommand::Unknown("send requires a message")` and prints the error.
2. User is not connected (`self.project.is_none()` / `self.connected_project.is_none()`) — `send_message()` returns `Err("Not connected to any project")`.

There is NO missing registration. The command exists. The fix (if needed) is better error messaging distinguishing "no argument" from "not connected".

---

## 2. GitHub polling

### Poll loop location

`crates/commander-api/src/handlers/web.rs`

- Spawner function: `spawn_github_stats_poller()`, line 895
- Called from: `crates/commander-api/src/router.rs`, line 114 — `handlers::web::spawn_github_stats_poller(state.github_stats.clone())`
- Poll function: `poll_github_stats()`, line 910
- Shared state type: `Arc<RwLock<HashMap<String, GitHubStats>>>` stored in `AppState.github_stats` (`crates/commander-api/src/state.rs`, line 75)

### Actual poll interval

**Exactly 1 hour (3600 seconds).**

```rust
// router.rs line 113 comment:
// Start the GitHub stats poller (hourly).

// web.rs lines 903–904:
// Poll every hour
tokio::time::sleep(Duration::from_secs(3600)).await;
```

Note: The poll fires once at startup (no initial sleep), then every 3600 seconds. There is a 2-second delay between each repo within a poll cycle (lines 984 and 997).

### Which repos get polled

Every git repository found under the configured scan directories. It is NOT limited to the current project — it scans all directories.

The algorithm (lines 937–1007):
1. Iterates subdirectories under each scan root
2. For each directory, runs `git remote get-url origin`
3. Calls `parse_github_repo()` to extract `owner/repo` from the remote URL (handles SSH `git@github.com:owner/repo.git` and HTTPS `https://github.com/owner/repo.git`)
4. Queries GitHub Search API for open issues and open PRs separately
5. Stores results keyed by directory name in the shared `HashMap<String, GitHubStats>`

### How the repo list is determined

`load_scan_directories()` in `web.rs` at line 583:

1. Reads `~/.ai-commander/config.json` and looks for a `scan_directories` array
2. Falls back to `DEFAULT_SCAN_DIRECTORIES` (line 564):

```rust
const DEFAULT_SCAN_DIRECTORIES: &[&str] = &[
    "Projects",
    "src",
    "projects",
    "code",
    "work",
    // (likely more entries)
];
```

These are relative to `$HOME`. So by default it scans `~/Projects/`, `~/src/`, `~/projects/`, `~/code/`, `~/work/`.

The API endpoint `GET /api/github-stats` (router.rs line 72) returns the entire cached map, and the web UI badges look up stats by project directory name.

---

## Key file paths + line numbers summary

| Area | File | Lines |
|------|------|-------|
| `/send` enum variant | `crates/ai-commander/src/repl.rs` | 580–581 |
| `/send` REPL parser | `crates/ai-commander/src/repl.rs` | 653–655 |
| `/send` REPL handler | `crates/ai-commander/src/repl.rs` | 1154–1240 |
| `/send` TUI dispatcher | `crates/ai-commander/src/tui/commands.rs` | 203–211 |
| `send_message()` TUI impl | `crates/ai-commander/src/tui/messaging.rs` | 16–end |
| TUI input entry (slash strip) | `crates/ai-commander/src/tui/input.rs` | 63–64 |
| Unknown command fallback (TUI) | `crates/ai-commander/src/tui/commands.rs` | 222–224 |
| Unknown command fallback (REPL) | `crates/ai-commander/src/repl.rs` | 1404–1409 |
| GitHub poller spawner | `crates/commander-api/src/handlers/web.rs` | 895–907 |
| GitHub poll function | `crates/commander-api/src/handlers/web.rs` | 910–1017 |
| Poll interval (3600s) | `crates/commander-api/src/handlers/web.rs` | 904 |
| Router spawn call | `crates/commander-api/src/router.rs` | 114 |
| `AppState.github_stats` field | `crates/commander-api/src/state.rs` | 75 |
| `load_scan_directories()` | `crates/commander-api/src/handlers/web.rs` | 583–603 |
| `DEFAULT_SCAN_DIRECTORIES` | `crates/commander-api/src/handlers/web.rs` | 564–569 |
| `GET /api/github-stats` route | `crates/commander-api/src/router.rs` | 72 |
