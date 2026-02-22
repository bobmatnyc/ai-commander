# Commander GUI

Tauri-based desktop GUI for AI Commander.

## Phase 1 - Tauri Backend (Complete)

### Features Implemented

- **State Management**: `GuiState` with persistence, tmux orchestration, and bot status tracking
- **Tauri Commands** (IPC Layer):
  - `list_sessions`: List all tmux sessions with connection status
  - `connect_session`: Connect to a specific session
  - `disconnect_session`: Disconnect from current session
  - `send_message`: Send message to connected session
  - `start_bot`: Start Telegram bot daemon
  - `stop_bot`: Stop Telegram bot daemon
  - `get_bot_status`: Query bot running status
  - `generate_pairing_code`: Generate pairing code (placeholder)
- **Event Streaming**: Background polling for session output (500ms interval)

### Architecture

```
commander-gui/
├── src/
│   ├── main.rs          # Tauri entry point, app setup
│   ├── state.rs         # GuiState management
│   ├── commands.rs      # Tauri IPC commands
│   └── events.rs        # Event streaming (session output)
├── icons/
│   └── icon.png         # App icon (RGBA format)
├── Cargo.toml           # Dependencies
├── tauri.conf.json      # Tauri configuration
└── build.rs             # Build script

Dependencies:
- commander-models
- commander-persistence (StateStore)
- commander-tmux (TmuxOrchestrator)
- commander-telegram (daemon module)
- commander-core (config)
- commander-adapters
```

### Build

```bash
cargo build -p commander-gui
```

Binary: `target/debug/commander-gui`

### Testing

```bash
# Run the GUI (requires frontend - Phase 2)
cargo run -p commander-gui
```

## Phase 2 - Svelte Frontend (TODO)

Will implement:
- Session list UI
- Chat interface
- Bot control panel
- Pairing UI
- Settings panel

## Notes

- **Session Polling**: Captures last 50 lines every 500ms when connected
- **Daemon Integration**: Uses `commander-telegram::daemon` for bot lifecycle
- **State Persistence**: Leverages `StateStore` with configured state directory
- **Tmux Integration**: Direct access to `TmuxOrchestrator` for session management
- **Frontend Placeholder**: Currently configured for development on http://localhost:5173

## Configuration

`tauri.conf.json`:
- Window size: 1200x800 (min: 800x600)
- Product name: "AI Commander"
- Development URL: http://localhost:5173 (Svelte dev server)
- Build output: `ui/dist` (to be created in Phase 2)

## Development Status

- ✅ Phase 1: Tauri backend foundation
- ⏳ Phase 2: Svelte frontend UI components
- ⏳ Phase 3: Integration and testing
