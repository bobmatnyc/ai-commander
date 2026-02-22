# AI Commander GUI

Desktop application providing all TUI features with improved usability and discoverability.

## Overview

The GUI uses Tauri 2.x (Rust backend) + Svelte (TypeScript frontend) to provide:
- **Lightweight binary** (~10MB vs 100MB+ Electron)
- **Native performance** (50-100MB RAM vs 500MB+ Electron)
- **Cross-platform installers** (macOS .dmg, Linux .AppImage/.deb, Windows .msi)
- **All core AI Commander features** available through intuitive UI

## Features

### Session Management
- **Real-time session list**: Auto-refreshes every 2 seconds
- **One-click connection**: Click any session to connect instantly
- **Visual feedback**: Connected session highlighted with active styling
- **Disconnect**: Disconnect from current session with one click

### Chat Interface
- **Bidirectional messaging**: Send messages to Claude and receive responses
- **Auto-scroll**: Automatically scrolls to new messages
- **Manual scroll control**: Scroll button appears when viewing history
- **Timestamps**: All messages timestamped with `HH:MM:SS` format
- **Keyboard shortcuts**:
  - **Enter**: Send message
  - **Shift+Enter**: Insert newline (multiline messages)

### Bot Daemon Management
- **Status monitoring**: Auto-refreshes every 5 seconds
- **Start/Stop controls**: Launch and terminate bot daemon from UI
- **PID display**: Shows process ID when bot is running
- **Status indicators**: Clear visual feedback (Running/Stopped)

### UI/UX
- **Responsive layout**: Flexbox-based layout adapts to window resizing
- **Tailwind CSS styling**: Modern, consistent design system
- **Hover states**: Interactive elements have visual feedback
- **Error handling**: Clear error messages via alerts

## Installation

### Prerequisites

**For Development**:
- Rust 1.70+ (for Tauri backend)
- Node.js 18+ (for Svelte frontend)
- npm 9+

**For Production Use**:
- Just the binary (no runtime dependencies)

### Development Setup

```bash
# 1. Install frontend dependencies
cd crates/commander-gui/ui
npm install

# 2. Return to GUI crate root
cd ..

# 3. Start development server with hot-reload
cargo tauri dev
```

**What happens**:
1. Tauri builds the Rust backend
2. Vite starts the frontend dev server (http://localhost:5173)
3. Tauri window opens with hot-reload enabled
4. Code changes trigger automatic reload

### Production Build

```bash
cd crates/commander-gui
cargo tauri build --release
```

**Output locations**:
- **macOS**: `target/release/bundle/macos/AI Commander.app`
- **Linux**: `target/release/bundle/appimage/ai-commander_*.AppImage`
- **Windows**: `target/release/bundle/msi/AI Commander_*.msi`

**Binary size**: ~10MB (Tauri 2.x with optimized dependencies)

## Architecture

### Technology Stack

**Backend (Rust)**:
- **Tauri 2.x**: Native window management and IPC
- **tokio**: Async runtime for background tasks
- **serde**: JSON serialization for IPC

**Frontend (TypeScript/Svelte)**:
- **Svelte 4.2**: Reactive UI framework
- **TypeScript 5.3**: Type-safe frontend code
- **Tailwind CSS 3.4**: Utility-first CSS framework
- **Vite 5.x**: Fast build tool and dev server

### Project Structure

```
commander-gui/
├── src/                    # Rust backend
│   ├── main.rs            # Tauri entry point, app setup
│   ├── state.rs           # GuiState management
│   ├── commands.rs        # Tauri IPC commands
│   └── events.rs          # Event streaming (session output)
├── ui/                    # Svelte frontend
│   ├── src/
│   │   ├── App.svelte           # Root component
│   │   ├── components/
│   │   │   ├── SessionList.svelte    # Session list UI
│   │   │   ├── ChatView.svelte       # Message display
│   │   │   ├── InputArea.svelte      # Message input
│   │   │   └── BotStatus.svelte      # Bot control panel
│   │   ├── stores/
│   │   │   └── index.ts              # Svelte stores (state)
│   │   └── main.ts                   # Frontend entry point
│   ├── public/
│   ├── package.json
│   ├── tsconfig.json
│   └── vite.config.ts
├── icons/
│   └── icon.png           # App icon (RGBA format)
├── Cargo.toml             # Rust dependencies
├── tauri.conf.json        # Tauri configuration
└── build.rs               # Build script
```

### Communication Architecture

```
┌─────────────────────────────────────────┐
│         Svelte Frontend (UI)            │
│  ┌──────────────────────────────────┐   │
│  │  Components (SessionList,        │   │
│  │  ChatView, InputArea, BotStatus) │   │
│  └──────────┬───────────────────────┘   │
│             │                            │
│             │ Svelte Stores              │
│             │ (Reactive State)           │
│             │                            │
│             v                            │
│  ┌──────────────────────────────────┐   │
│  │  Tauri IPC Commands (invoke)     │   │
│  │  - list_sessions                 │   │
│  │  - connect_session               │   │
│  │  - send_message                  │   │
│  │  - start_bot, stop_bot           │   │
│  └──────────┬───────────────────────┘   │
└─────────────┼──────────────────────────┘
              │
         Tauri IPC Bridge
              │
┌─────────────┼──────────────────────────┐
│             v                           │
│  ┌──────────────────────────────────┐  │
│  │  Rust Backend (commands.rs)      │  │
│  │  - Handle IPC commands           │  │
│  │  - Access GuiState               │  │
│  └──────────┬───────────────────────┘  │
│             │                           │
│             v                           │
│  ┌──────────────────────────────────┐  │
│  │  GuiState (state.rs)             │  │
│  │  - TmuxOrchestrator              │  │
│  │  - StateStore                    │  │
│  │  - BotDaemon management          │  │
│  └──────────┬───────────────────────┘  │
│             │                           │
│             v                           │
│  ┌──────────────────────────────────┐  │
│  │  Shared Crates                   │  │
│  │  - commander-tmux                │  │
│  │  - commander-telegram (daemon)   │  │
│  │  - commander-persistence         │  │
│  │  - commander-models              │  │
│  └──────────────────────────────────┘  │
│             Rust Backend                │
└─────────────────────────────────────────┘

     Events (Backend → Frontend)
     ────────────────────────────▶
     - session-output (500ms poll)
```

### State Management

**Frontend (Svelte Stores)**:
```typescript
// src/stores/index.ts
export const sessions = writable<Session[]>([]);
export const currentSession = writable<string | null>(null);
export const messages = writable<Message[]>([]);
export const botRunning = writable<boolean>(false);
export const botPid = writable<number | null>(null);
```

**Backend (GuiState)**:
```rust
// src/state.rs
pub struct GuiState {
    tmux_orchestrator: Arc<Mutex<TmuxOrchestrator>>,
    state_store: Arc<Mutex<StateStore>>,
    current_session: Arc<Mutex<Option<String>>>,
}
```

### IPC Commands

All IPC commands are defined in `src/commands.rs`:

| Command | Parameters | Returns | Description |
|---------|-----------|---------|-------------|
| `list_sessions` | None | `Vec<Session>` | List all tmux sessions |
| `connect_session` | `session_name: String` | `Result<(), String>` | Connect to session |
| `disconnect_session` | None | `Result<(), String>` | Disconnect from current session |
| `send_message` | `message: String` | `Result<(), String>` | Send message to connected session |
| `start_bot` | None | `Result<(), String>` | Start Telegram bot daemon |
| `stop_bot` | None | `Result<(), String>` | Stop Telegram bot daemon |
| `get_bot_status` | None | `BotStatusResponse` | Query bot status and PID |
| `generate_pairing_code` | None | `Result<String, String>` | Generate pairing code (TODO) |

### Event Streaming

**Backend Polling** (`src/events.rs`):
```rust
// Poll session output every 500ms when connected
loop {
    if let Some(session_name) = get_current_session() {
        let output = capture_last_50_lines(session_name);
        window.emit("session-output", output)?;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
}
```

**Frontend Listening** (`ChatView.svelte`):
```typescript
import { listen } from '@tauri-apps/api/event';

listen('session-output', (event) => {
  const output = event.payload as string;
  messages.update(m => [...m, {
    content: output,
    timestamp: new Date(),
    direction: 'received'
  }]);
});
```

## Development Guide

### Prerequisites

```bash
# Check versions
rustc --version    # 1.70+
node --version     # 18+
npm --version      # 9+

# Install Tauri CLI (optional, cargo tauri is recommended)
npm install -g @tauri-apps/cli
```

### Development Workflow

#### Option 1: Full Tauri Dev (Recommended)

```bash
cd crates/commander-gui
cargo tauri dev
```

**Advantages**:
- Full Tauri environment with IPC
- Hot-reload for both frontend and backend
- All commands and events work

#### Option 2: Frontend Only

```bash
cd crates/commander-gui/ui
npm run dev
```

**Advantages**:
- Faster frontend-only iteration
- Vite dev server on http://localhost:5173

**Limitations**:
- Tauri commands will fail (mocked responses needed)
- No backend state or events

### Building for Production

```bash
cd crates/commander-gui

# Debug build (faster compilation)
cargo tauri build --debug

# Release build (optimized binary)
cargo tauri build --release

# Platform-specific
cargo tauri build --target aarch64-apple-darwin  # Apple Silicon
cargo tauri build --target x86_64-pc-windows-msvc  # Windows x64
```

### Testing

**Frontend Unit Tests**:
```bash
cd crates/commander-gui/ui
npm run test
```

**Backend Tests**:
```bash
cd crates/commander-gui
cargo test
```

**Integration Testing**:
- See [`QA_TESTING_REPORT.md`](/QA_TESTING_REPORT.md) for comprehensive manual testing checklist
- 40 test cases covering session management, messaging, bot control, and UI/UX

### Debugging

**Frontend Debugging**:
1. Open Developer Tools in Tauri window: Right-click → Inspect
2. Console logs appear in DevTools console
3. Use `console.log()` for debugging

**Backend Debugging**:
1. Backend logs appear in terminal where `cargo tauri dev` was run
2. Use `println!()` or `eprintln!()` for debugging
3. Use `dbg!()` macro for variable inspection

**IPC Debugging**:
```typescript
// Frontend: Log all IPC calls
import { invoke } from '@tauri-apps/api/tauri';

invoke('list_sessions')
  .then(sessions => console.log('Sessions:', sessions))
  .catch(err => console.error('Error:', err));
```

```rust
// Backend: Log command execution
#[tauri::command]
async fn list_sessions(state: State<'_, GuiState>) -> Result<Vec<Session>, String> {
    eprintln!("[DEBUG] list_sessions called");
    // ... implementation
}
```

## Troubleshooting

### Build Issues

**Issue**: `npm install` fails with dependency conflicts

**Solution**:
```bash
cd crates/commander-gui/ui
rm -rf node_modules package-lock.json
npm install
```

---

**Issue**: Tauri build fails with "could not find `Cargo.toml`"

**Solution**: Ensure you're in `crates/commander-gui/` directory:
```bash
cd crates/commander-gui
cargo tauri build
```

---

**Issue**: Frontend build fails with TypeScript errors

**Solution**: Check TypeScript version and tsconfig:
```bash
cd ui
npm list typescript  # Should be 5.3+
npx tsc --noEmit      # Check for errors
```

### Runtime Issues

**Issue**: Sessions don't appear in list

**Possible causes**:
1. No tmux sessions exist → Create sessions via TUI first
2. Tmux not installed → Install tmux: `brew install tmux` (macOS)
3. StateStore path incorrect → Check `~/.ai-commander/state/`

**Debug**:
```bash
# Check if tmux sessions exist
tmux ls

# Check StateStore
ls -la ~/.ai-commander/state/sessions/
```

---

**Issue**: Bot start/stop commands fail

**Possible causes**:
1. `TELEGRAM_BOT_TOKEN` not set → Add to `~/.ai-commander/config/.env.local`
2. Bot binary not built → Build with `cargo build -p commander-telegram --release`
3. Permission issues → Check file permissions on `~/.ai-commander/state/telegram.pid`

**Debug**:
```bash
# Check bot binary
ls -la target/release/commander-telegram

# Check environment
grep TELEGRAM_BOT_TOKEN ~/.ai-commander/config/.env.local

# Check PID file
cat ~/.ai-commander/state/telegram.pid
```

---

**Issue**: Messages not appearing in chat view

**Possible causes**:
1. Not connected to session → Connect first via SessionList
2. Event listener not registered → Check DevTools console for errors
3. Session output polling failed → Check backend logs

**Debug**: Open DevTools and check console for `session-output` events

---

**Issue**: Window doesn't open or crashes on launch

**Solution**:
```bash
# Clear Tauri cache
rm -rf ~/.cache/ai-commander/  # Linux
rm -rf ~/Library/Caches/ai-commander/  # macOS

# Rebuild
cargo tauri build --debug
```

### Platform-Specific Issues

**macOS**:
- **Issue**: "App is damaged and can't be opened"
- **Solution**: Unsigned app; disable Gatekeeper: `sudo spctl --master-disable`

**Linux**:
- **Issue**: AppImage doesn't run
- **Solution**: Make executable: `chmod +x ai-commander_*.AppImage`

**Windows**:
- **Issue**: Installer blocked by SmartScreen
- **Solution**: Click "More info" → "Run anyway"

## Configuration

### Tauri Configuration (`tauri.conf.json`)

```json
{
  "productName": "AI Commander",
  "version": "0.3.0",
  "identifier": "com.aicommander.app",
  "build": {
    "devPath": "http://localhost:5173",
    "distDir": "../ui/dist"
  },
  "tauri": {
    "windows": [
      {
        "title": "AI Commander",
        "width": 1200,
        "height": 800,
        "minWidth": 800,
        "minHeight": 600,
        "resizable": true,
        "fullscreen": false
      }
    ]
  }
}
```

### Frontend Configuration (`vite.config.ts`)

```typescript
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true
  }
});
```

## Contributing

### Adding New Features

1. **Backend (Rust)**:
   - Add Tauri command in `src/commands.rs`
   - Update `GuiState` if needed in `src/state.rs`
   - Expose command in `src/main.rs` via `.invoke_handler()`

2. **Frontend (Svelte)**:
   - Create component in `ui/src/components/`
   - Add store in `ui/src/stores/index.ts` for state
   - Invoke command via `import { invoke } from '@tauri-apps/api/tauri'`

3. **Testing**:
   - Add unit tests for new functions
   - Update [`QA_TESTING_REPORT.md`](/QA_TESTING_REPORT.md) with manual test cases
   - Test on all target platforms

### Code Style

**Rust**:
```bash
cargo fmt --all
cargo clippy --all-targets --all-features
```

**TypeScript/Svelte**:
```bash
cd ui
npm run lint
npm run format
```

## Roadmap

### Completed (MVP - Phases 1-2)
- ✅ Tauri backend with IPC commands
- ✅ Svelte frontend with session management
- ✅ Chat interface with auto-scroll
- ✅ Bot daemon control
- ✅ Real-time event streaming

### Planned Enhancements
- 🔄 Pairing code modal UI (currently hardcoded "12345678")
- 🔄 Create new session UI (currently requires TUI)
- 🔄 Stop/destroy session UI
- 🔄 Settings panel (theme, preferences)
- 🔄 Session logs viewer
- 🔄 Keyboard shortcut customization
- 🔄 Dark/light theme toggle
- 🔄 Notification system for bot events

### Future Considerations
- Multi-window support (one window per session)
- Native system tray integration
- Auto-updater for releases
- Plugin system for custom adapters
- Syntax highlighting in chat view

## License

MIT License - see [LICENSE](/LICENSE) for details.

## Related Documentation

- [`crates/commander-gui/README.md`](/crates/commander-gui/README.md) - Crate-specific details
- [`QA_TESTING_REPORT.md`](/QA_TESTING_REPORT.md) - QA testing and validation
- [`docs/architecture/telegram-bot-daemon-architecture.md`](/docs/architecture/telegram-bot-daemon-architecture.md) - Daemon architecture
- [Main README.md](/README.md) - Project overview
