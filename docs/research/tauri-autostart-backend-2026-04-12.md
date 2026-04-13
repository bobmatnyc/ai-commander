# Tauri App Backend Auto-Start Research

**Date:** 2026-04-12

## Files Examined

- `crates/commander-gui/src/main.rs`
- `crates/commander-gui/src/lib.rs` (does not exist; modules declared inline in main.rs)
- `crates/commander-gui/src/state.rs`
- `crates/commander-gui/src/commands.rs`
- `crates/commander-gui/Cargo.toml`
- `crates/commander-api/src/lib.rs`
- `crates/commander-api/src/router.rs`
- `crates/commander-daemon/src/service.rs`

---

## Key Findings

### 1. Current Tauri Setup (main.rs)

The `setup()` hook already exists and spawns one background task:

```rust
tauri::Builder::default()
    .setup(|app| {
        let state = state::GuiState::new()?;
        tauri::async_runtime::spawn(async move {
            events::start_session_polling(app_handle, state_clone).await;
        });
        app.manage(state);
        Ok(())
    })
```

This is the correct place to add more `tauri::async_runtime::spawn` calls for backend servers.

### 2. GuiState (state.rs)

`GuiState` holds:
- `store: Arc<StateStore>` — persistence
- `tmux: Option<Arc<TmuxOrchestrator>>` — tmux control
- `current_session: Arc<RwLock<Option<String>>>`
- `bot_status: Arc<RwLock<DaemonStatus>>` — Telegram bot only

**No API server handle or DaemonService handle is stored.** These would need to be added.

### 3. commander-api (lib.rs / router.rs)

The API exposes:
- `serve(config: ApiConfig, state: AppState) -> Result<(), std::io::Error>` — async, blocks on `axum::serve`
- `AppState::new(...)` — requires `ApiConfig`, optional runtime, `EventManager`, `WorkQueue`, `AdapterRegistry`
- `commander-api` is **not** in `commander-gui/Cargo.toml` — must be added as a dependency

### 4. commander-daemon (service.rs)

`DaemonService` has two startup modes:
- `DaemonService::run(self)` — foreground, blocks until SIGTERM; starts IPC server, cleanup task, and idle monitor (tmux-based MPM session watcher)
- `DaemonService::daemonize(self)` — spawns `commander-daemon` binary as a background `nohup` subprocess

**Key point:** `DaemonService::new().await` is fully async and self-contained. The `run()` method can be wrapped in a `tokio::spawn` to run it as a task inside the Tauri process, avoiding the need for an external binary.

`commander-daemon` is also **not** in `commander-gui/Cargo.toml`.

### 5. Existing commands.rs pattern

The `start_bot` / `stop_bot` commands use `commander_telegram::daemon::start()` which spawns an external subprocess. The API and DaemonService could follow this same pattern (subprocess) or be embedded directly.

---

## What Needs to Change

### Option A: Embed servers in-process (recommended for GUI)

**Cargo.toml additions:**
```toml
commander-api = { path = "../commander-api" }
commander-daemon = { path = "../commander-daemon" }
```

**state.rs:** Add handles for graceful shutdown:
```rust
pub struct GuiState {
    // existing fields ...
    pub api_shutdown: Option<tokio::sync::broadcast::Sender<()>>,
    pub daemon_shutdown: Option<tokio::sync::broadcast::Sender<()>>,
}
```

**main.rs setup() hook:** Add two more spawned tasks:
```rust
// Start commander-daemon service (IPC + idle monitor + cleanup)
tauri::async_runtime::spawn(async move {
    let svc = DaemonService::new().await.expect("daemon init");
    svc.run().await.ok();
});

// Start commander-api HTTP server
tauri::async_runtime::spawn(async move {
    let state = AppState::new(ApiConfig::default(), None, event_mgr, work_q, adapters);
    serve(ApiConfig::default(), state).await.ok();
});
```

### Option B: Spawn external binaries (matches existing bot pattern)

Call `DaemonService::daemonize()` and a similar spawn for `commander-api` from within the setup hook or a Tauri command. Requires the binaries to be present.

---

## Summary

| Question | Answer |
|---|---|
| Does Tauri app start any background services? | Yes — one: `events::start_session_polling` |
| Can commander-api be started programmatically? | Yes — `serve(config, state)` is a simple async function |
| Does DaemonService have a `run()` to spawn? | Yes — `DaemonService::new().await?.run()` blocks and handles its own lifecycle |
| Relationship between Tauri state and backends? | Currently none; `GuiState` would need new fields for shutdown handles |
| Dependencies missing from commander-gui? | `commander-api` and `commander-daemon` not in Cargo.toml |
