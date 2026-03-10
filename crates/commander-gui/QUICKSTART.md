# AI Commander GUI - Quick Start Guide

## Prerequisites

- Rust (latest stable)
- Node.js 18+ and npm
- Tauri CLI: `cargo install tauri-cli`
- AI Commander bot configured (Telegram API credentials)

## Installation

### 1. Install Dependencies

```bash
# Install UI dependencies
cd crates/commander-gui/ui
npm install

# Go back to GUI crate root
cd ..
```

### 2. Development Mode

Run the full desktop app in development mode:

```bash
# From crates/commander-gui/
cargo tauri dev
```

This will:
1. Start Vite dev server (UI)
2. Build Rust backend
3. Launch desktop app with hot reload

**First launch** may take 2-3 minutes to compile Rust dependencies.

### 3. Production Build

Build the distributable application:

```bash
# From crates/commander-gui/
cargo tauri build
```

Output locations:
- **macOS**: `target/release/bundle/macos/AI Commander.app`
- **Linux**: `target/release/bundle/appimage/ai-commander_*.AppImage`
- **Windows**: `target/release/bundle/msi/AI Commander_*.msi`

## Usage

### Starting the Application

1. **Launch the GUI**: Open the built app or run `cargo tauri dev`
2. **Start the Bot**: Click "Start" button in the top-right
3. **Select Session**: Choose a Telegram session from the sidebar
4. **Send Messages**: Type in the input field and press Enter

### Key Features

#### Bot Control
- **Start**: Launches the Telegram bot daemon
- **Stop**: Gracefully stops the bot daemon
- **Status**: Shows running state and process ID

#### Session Management
- **List**: Auto-refreshes available sessions every 2 seconds
- **Connect**: Click any session to establish connection
- **Status**: Green indicator for connected, gray for disconnected

#### Chat Interface
- **Send**: Type message and press Enter
- **Receive**: Messages appear automatically
- **Scroll**: Auto-scrolls to bottom, manual scroll available
- **Timestamps**: Each message shows send/receive time

## Project Structure

```
crates/commander-gui/
├── src/                       # Rust backend
│   ├── main.rs               # Tauri app entry
│   ├── commands.rs           # IPC command handlers
│   └── daemon.rs             # Bot daemon management
├── ui/                        # Svelte frontend
│   ├── src/
│   │   ├── lib/
│   │   │   ├── components/   # UI components
│   │   │   └── stores/       # State management
│   │   ├── App.svelte        # Root component
│   │   └── main.ts           # Entry point
│   ├── index.html
│   ├── package.json
│   └── vite.config.ts
├── tauri.conf.json           # Tauri configuration
├── Cargo.toml                # Rust dependencies
└── build.rs                  # Build script
```

## Development Workflow

### Frontend Only (Fast Iteration)

```bash
cd crates/commander-gui/ui
npm run dev
```

Visit http://localhost:5173 - UI will load but IPC calls will fail (no backend).

### Full Stack (Integrated)

```bash
cd crates/commander-gui
cargo tauri dev
```

Both frontend and backend run together with hot reload.

### Backend Only (Rust Testing)

```bash
cd crates/commander-gui
cargo build
cargo test
```

## Troubleshooting

### "Failed to load sessions"

**Cause**: Bot not running or no sessions configured

**Solution**:
1. Ensure bot is started (click Start button)
2. Check that Telegram sessions exist in `.sessions/` directory
3. Verify environment variables (`TELEGRAM_*`) are set

### "Failed to connect to session"

**Cause**: Session file corrupted or not authenticated

**Solution**:
1. Delete the session file from `.sessions/`
2. Re-authenticate using the CLI: `cargo run -- auth`
3. Try connecting again

### "Failed to start bot"

**Cause**: Bot already running or missing credentials

**Solution**:
1. Check if bot is already running: `ps aux | grep commander`
2. Stop existing instances: `pkill -f commander-telegram`
3. Verify `.env` file has all required variables
4. Check logs: Bot status should show error details

### Build Errors

**UI build fails**:
```bash
cd crates/commander-gui/ui
rm -rf node_modules package-lock.json
npm install
npm run build
```

**Rust build fails**:
```bash
cd crates/commander-gui
cargo clean
cargo build
```

### Hot Reload Not Working

**Frontend changes not reflecting**:
- Vite HMR issue: Restart `cargo tauri dev`
- Check browser console for errors

**Backend changes not reflecting**:
- Tauri recompiles on Rust changes automatically
- If stuck, kill process and restart

## Configuration

### Tauri Configuration

Edit `crates/commander-gui/tauri.conf.json`:

```json
{
  "productName": "AI Commander",
  "identifier": "com.aicommander.app",
  "build": {
    "beforeBuildCommand": "cd ui && npm run build",
    "beforeDevCommand": "cd ui && npm run dev",
    "devUrl": "http://localhost:5173",
    "frontendDist": "./ui/dist"
  }
}
```

### UI Configuration

Edit `crates/commander-gui/ui/vite.config.ts`:

```typescript
export default defineConfig({
  server: {
    port: 5173,        // Dev server port
    strictPort: true,  // Fail if port in use
  },
});
```

## Environment Variables

Required for bot functionality (set in project root `.env`):

```bash
TELEGRAM_API_ID=your_api_id
TELEGRAM_API_HASH=your_api_hash
TELEGRAM_BOT_TOKEN=your_bot_token
```

## Testing

### Manual Testing Checklist

- [ ] Start bot via GUI
- [ ] Stop bot via GUI
- [ ] Bot status updates correctly
- [ ] Sessions list populates
- [ ] Connect to session works
- [ ] Send message appears in chat
- [ ] Received messages appear
- [ ] Auto-scroll works
- [ ] Manual scroll shows return button
- [ ] Enter key sends messages
- [ ] Empty messages blocked
- [ ] Error messages display

### Automated Testing

```bash
# UI unit tests (not yet implemented)
cd crates/commander-gui/ui
npm test

# Rust unit tests
cd crates/commander-gui
cargo test
```

## Performance

### Development Mode
- Initial compile: 2-3 minutes
- Hot reload: 1-2 seconds
- Vite HMR: <100ms

### Production Build
- Full build: 3-5 minutes
- Bundle size: ~25KB JS (gzipped)
- Memory usage: ~50MB
- CPU usage: <5% idle

## Logging

### Backend Logs

Rust logs via `tracing`:

```bash
# Enable debug logs
RUST_LOG=debug cargo tauri dev

# Enable trace logs
RUST_LOG=trace cargo tauri dev
```

### Frontend Logs

Browser console (F12 DevTools):
- Component events
- IPC call results
- Store updates

## Common Tasks

### Add New IPC Command

1. **Backend** (`src/commands.rs`):
```rust
#[tauri::command]
pub async fn my_command(param: String) -> Result<String, String> {
    Ok(format!("Got: {}", param))
}
```

2. **Register** (`src/main.rs`):
```rust
.invoke_handler(tauri::generate_handler![
    commands::my_command,
    // ... other commands
])
```

3. **Frontend** (any component):
```typescript
import { invoke } from '@tauri-apps/api/core';

const result = await invoke('my_command', { param: 'value' });
```

### Add New Component

1. Create `ui/src/lib/components/MyComponent.svelte`
2. Import in parent component:
```svelte
<script lang="ts">
  import MyComponent from './lib/components/MyComponent.svelte';
</script>

<MyComponent />
```

### Add New Store

1. Add to `ui/src/lib/stores/app.ts`:
```typescript
export const myStore = writable<MyType>(initialValue);
```

2. Use in components:
```svelte
<script lang="ts">
  import { myStore } from '../stores/app';
</script>

{$myStore}
```

## Resources

- **Tauri Docs**: https://tauri.app/
- **Svelte Docs**: https://svelte.dev/
- **Vite Docs**: https://vitejs.dev/
- **Tailwind CSS**: https://tailwindcss.com/

## Support

Issues: https://github.com/yourusername/ai-commander/issues

## License

MIT License - See LICENSE file for details
