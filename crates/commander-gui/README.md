# Commander GUI

Tauri-based desktop GUI for AI Commander.

**Status**: MVP Complete (Phases 1-2) - QA Approved for Manual Validation

## Features

### Phase 1 - Tauri Backend ✅

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

### Phase 2 - Svelte Frontend ✅

- **Session List Component**: Real-time session list with 2s auto-refresh and click-to-connect
- **Chat View Component**: Message display with auto-scroll, timestamps, and scroll-to-bottom button
- **Input Area Component**: Message composition with Enter to send, Shift+Enter for newlines
- **Bot Status Component**: Bot control panel with 5s status monitoring and start/stop buttons
- **Svelte Stores**: Reactive state management for sessions, messages, bot status
- **TypeScript**: Type-safe frontend code with full type definitions
- **Tailwind CSS**: Modern, consistent styling with hover states and transitions

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

## Installation & Usage

### Prerequisites

- Rust 1.70+
- Node.js 18+
- npm 9+

### Development

```bash
# Install frontend dependencies
cd ui
npm install
cd ..

# Start development server with hot-reload
cargo tauri dev
```

### Production Build

```bash
cargo tauri build --release
```

Output: `target/release/bundle/` (platform-specific installers)

### Running

```bash
# Development
cargo tauri dev

# Production binary
./target/release/bundle/macos/AI Commander.app  # macOS
./target/release/ai-commander                   # Linux
```

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

## QA Status

**Code Analysis**: 35/40 tests verified structurally
**Manual Validation**: Required (see [`/QA_TESTING_REPORT.md`](/QA_TESTING_REPORT.md))
**Confidence**: 95%
**Critical Issues**: 0
**Non-Critical Issues**: 2 (pairing code placeholder, no create/destroy session UI)

## Development Status

- ✅ Phase 1: Tauri backend foundation
- ✅ Phase 2: Svelte frontend UI components
- ✅ Phase 3: Integration and QA validation
- 🔄 Phase 4: Manual testing and bug fixes

## Next Steps

1. **Manual Testing**: Follow checklist in [`/QA_TESTING_REPORT.md`](/QA_TESTING_REPORT.md)
2. **Pairing Code Modal**: Implement UI for pairing code display
3. **Session Creation**: Add UI for creating new sessions
4. **Session Destruction**: Add UI for stopping/destroying sessions

## Documentation

- **Comprehensive Guide**: [`/docs/GUI.md`](/docs/GUI.md)
- **Architecture Details**: See `/docs/architecture/`
- **Main Project README**: [`/README.md`](/README.md)
