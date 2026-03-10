# AI Commander

Multi-interface AI session management system written in Rust.

Control AI coding sessions from TUI, REPL, or Telegram with support for multiple adapters including Claude Code, MPM, Aider, and plain shell sessions.

## Features

### Core
- **Multi-interface AI session management** - Control AI coding sessions from TUI, REPL, or Telegram
- **Adapter system** - Support for Claude Code, MPM, Aider, and plain shell sessions
- **Project management** - Create, connect, disconnect, and manage multiple projects
- **Tmux integration** - Sessions run in tmux for persistence and multiplexing

### TUI
- Interactive terminal UI with ratatui
- Session list view (F3)
- Inspect mode for live tmux view (F2)
- Text wrapping for long outputs
- Tab autocomplete for slash commands
- Clickable session links - click session names in `/list` output to connect

### GUI (Graphical User Interface) 🪟
- Desktop application with all TUI features and better discoverability
- Real-time session list with auto-refresh (2s interval)
- Chat interface with Claude and auto-scroll
- Bot daemon control (start/stop/status with 5s monitoring)
- Keyboard shortcuts (Enter to send, Shift+Enter for newlines)
- Lightweight binary (~10MB, Tauri 2.x + Svelte)
- Cross-platform support (macOS, Linux, Windows)
- **Documentation**: See [`docs/GUI.md`](docs/GUI.md) for comprehensive guide

### Telegram Bot
- Remote session control from Telegram
- Secure pairing with 6-character codes
- Auto-start bot daemon on `/telegram` command
- Auto-build binary if not found
- Auto-restart bot on app launch
- Response summarization for mobile (via OpenRouter)
- Inline keyboard buttons - tap session names in `/list` or `/sessions` to connect
- Forum Topics support - create dedicated topics for different sessions in group chats

## Installation

### Quick Start - Local Deployment (macOS)

For stable production-like local setup with auto-starting services:

```bash
cd /Users/masa/Projects/ai-commander

# Build and install (5 minutes)
cargo build --release -p ai-commander
cd crates/commander-gui/ui && npm run build && cd .. && cargo tauri build && cd ../..
./scripts/install-user.sh

# Setup Telegram bot service (optional)
./scripts/setup-telegram-service.sh
```

**See [docs/QUICKSTART.md](docs/QUICKSTART.md) for complete setup guide.**

### Homebrew (macOS)

```bash
brew tap bobmatnyc/tools
brew install ai-commander
```

### From Source

```bash
# Requires Rust toolchain
git clone https://github.com/bobmatnyc/ai-commander
cd ai-commander
cargo install --path crates/ai-commander
```

### Verify Installation

```bash
ai-commander --version
```

## Development

### Auto-Restart on Code Changes

AI Commander includes a development mode that automatically rebuilds and restarts the bot when you save code changes, while preserving active sessions:

```bash
./scripts/dev.sh          # Start dev mode with auto-restart
./scripts/dev.sh --debug  # Debug build (faster compilation)
./scripts/dev.sh -v       # Verbose bot logging
```

**How it works:**
- Watches `crates/` directory for file changes using `cargo-watch`
- Automatically rebuilds on save (~30 seconds)
- Gracefully restarts the bot with SIGTERM
- Saves active sessions before shutdown
- Restores valid sessions after restart (<24h old, tmux exists)
- Sends Telegram notification with restoration status

**Development workflow:**
1. Run `./scripts/dev.sh` once
2. Edit code in your editor
3. Save → Auto-rebuild → Auto-restart → Sessions restored
4. Check Telegram for rebuild notifications

No need to manually restart the bot during development!

### Development Dependencies

```bash
cargo install cargo-watch  # For auto-restart on code changes
```

### Running Tests

```bash
# Run all tests
cargo test

# Run specific crate tests
cargo test -p ai-commander
cargo test -p commander-telegram
cargo test -p commander-tmux

# Run tmux integration tests (requires tmux)
cargo test -p commander-tmux -- --ignored
```

## Quick Start

### TUI/REPL
1. Start the TUI: `ai-commander tui`
2. Create a project: `/connect /path/to/project -a claude-code -n myproject`
3. Send messages to interact with Claude Code
4. Use `/telegram` to enable mobile access
5. Use `/stop` to end session (auto-commits changes if in git repo)

### GUI
1. Install frontend dependencies:
   ```bash
   cd crates/commander-gui/ui
   npm install
   ```
2. Start the GUI:
   ```bash
   cd crates/commander-gui
   cargo tauri dev  # Development with hot-reload
   # OR
   cargo tauri build  # Production binary in target/release/bundle/
   ```
3. Click a session to connect or manage bot daemon from the UI

**Note**: GUI requires Node.js 18+ and npm 9+ for frontend development.

## Slash Commands

| Command | Description |
|---------|-------------|
| `/list` | List all projects |
| `/status` | Show project status |
| `/connect <path> -a <adapter> -n <name>` | Connect to a project |
| `/disconnect` | Disconnect from current project |
| `/send <message>` | Send message to session |
| `/sessions` | List active sessions |
| `/stop` | Commit changes and end session |
| `/telegram` | Generate pairing code for Telegram |
| `/inspect` | Toggle inspect mode (live tmux view) |
| `/clear` | Clear screen |
| `/help` | Show help |
| `/quit` | Exit |

## Telegram Integration

### Quick Start

1. Set `TELEGRAM_BOT_TOKEN` in `.env.local`
2. Run `/telegram` in TUI to generate a pairing code
3. In Telegram, send `/pair <code>` to your bot
4. Control sessions remotely from your phone

### Inline Keyboard Buttons

The `/list` and `/sessions` commands display inline keyboard buttons for one-tap session connection. Simply tap a session button to connect instead of typing the full `/connect` command.

### Forum Topics (Group Chat Mode)

Use Telegram Forum Topics to organize multiple sessions in a single group chat, with each session getting its own dedicated topic thread.

**Setup:**

1. Create a Telegram supergroup
2. Enable Forum Topics in group settings (Settings > Topics > Enable)
3. Add the bot as an admin
4. Run `/groupmode` to enable group mode
5. Run `/topic <session>` to create a topic for each session

**Commands:**

| Command | Description |
|---------|-------------|
| `/groupmode` | Enable group mode in the current supergroup |
| `/topic <session>` | Create a dedicated forum topic for a session |
| `/topics` | List all topics and their linked sessions |

**How it works:**

- Messages sent in each topic are automatically routed to that topic's linked session
- Responses from the session appear in the correct topic
- Great for managing multiple projects from one group chat
- Each topic acts as an isolated conversation with its session

## Architecture

```
┌─────────────────────────────────────────────────┐
│            User Interfaces                      │
│  ┌──────┐  ┌──────┐  ┌──────┐  ┌────────────┐  │
│  │ CLI  │  │ TUI  │  │ GUI  │  │ Telegram   │  │
│  │      │  │      │  │(Tauri│  │    Bot     │  │
│  └──┬───┘  └──┬───┘  │Svelte│  └─────┬──────┘  │
│     │        │        └───┬──┘        │         │
└─────┼────────┼────────────┼───────────┼─────────┘
      │        │            │           │
      v        v            v           v
┌─────────────────────────────────────────────────┐
│           Shared Core Crates                    │
│  ┌────────────────┐  ┌──────────────────────┐  │
│  │ commander-core │  │ commander-persistence│  │
│  │ commander-state│  │ commander-models     │  │
│  └────────────────┘  └──────────────────────┘  │
│  ┌────────────────┐  ┌──────────────────────┐  │
│  │ commander-tmux │  │ commander-telegram   │  │
│  └────────────────┘  └──────────────────────┘  │
│  ┌────────────────┐  ┌──────────────────────┐  │
│  │ commander-     │  │ commander-events     │  │
│  │   adapters     │  │ commander-work       │  │
│  │ commander-api  │  │ commander-runtime    │  │
│  └────────────────┘  └──────────────────────┘  │
└─────────────────────────────────────────────────┘
```

### Crates Overview

```
crates/
├── commander-core/      # Shared business logic (output filtering, summarization, config)
├── ai-commander/        # TUI and REPL interfaces (main binary)
├── commander-gui/       # GUI application (Tauri 2.x backend + Svelte frontend)
├── commander-telegram/  # Telegram bot with daemon management
├── commander-tmux/      # Tmux orchestration
├── commander-adapters/  # Runtime adapters (Claude Code, MPM, Shell, etc.)
├── commander-state/     # Project state management
├── commander-models/    # Core data types
├── commander-persistence/  # JSON file storage
├── commander-events/    # Event system
├── commander-work/      # Work queue
├── commander-runtime/   # Async runtime
└── commander-api/       # REST API
```

**Documentation**: See [`crates/commander-gui/README.md`](crates/commander-gui/README.md) for GUI details and [`docs/architecture/`](docs/architecture/) for architecture deep-dives.

## Configuration

### Storage

All application data is stored under `~/.ai-commander/`:

```
~/.ai-commander/
├── db/           # Databases (ChromaDB, etc.)
│   └── chroma/
├── logs/         # Application logs
├── config/       # User configuration
│   ├── config.toml
│   └── .env.local
├── cache/        # Temporary cache files
└── state/        # Runtime state files
    ├── pairings.json
    ├── projects.json
    ├── notifications.json
    ├── telegram.pid
    └── sessions/
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `COMMANDER_STATE_DIR` | Override base state directory (default: `~/.ai-commander/`) |
| `COMMANDER_DB_DIR` | Override database directory |
| `COMMANDER_LOG_DIR` | Override log directory |
| `COMMANDER_CONFIG_DIR` | Override config directory |
| `COMMANDER_CACHE_DIR` | Override cache directory |
| `TELEGRAM_BOT_TOKEN` | Telegram bot token for remote control |
| `OPENROUTER_API_KEY` | API key for response summarization |

Environment variables can be set in `~/.ai-commander/config/.env.local`.

### Migration from v0.2.x

If upgrading from v0.2.x, the application will automatically migrate data from `~/.commander/` to `~/.ai-commander/` on first run. The old directory is preserved with a marker file indicating migration occurred.

## REST API

```
GET    /api/health              Health check
GET    /api/projects            List projects
POST   /api/projects            Create project
GET    /api/projects/:id        Get project
DELETE /api/projects/:id        Delete project
POST   /api/projects/:id/start  Start instance
POST   /api/projects/:id/stop   Stop instance
POST   /api/projects/:id/send   Send message
GET    /api/events              List events
GET    /api/events/:id          Get event
POST   /api/events/:id/ack      Acknowledge
POST   /api/events/:id/resolve  Resolve
GET    /api/work                List work items
POST   /api/work                Create work item
GET    /api/work/:id            Get work item
POST   /api/work/:id/complete   Complete work
GET    /api/adapters            List adapters
```

## License

MIT License - see [LICENSE](LICENSE) for details.
