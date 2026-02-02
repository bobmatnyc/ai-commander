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

### Telegram Bot
- Remote session control from Telegram
- Secure pairing with 6-character codes
- Auto-start bot daemon on `/telegram` command
- Auto-build binary if not found
- Auto-restart bot on app launch
- Response summarization for mobile (via OpenRouter)

## Installation

```bash
# Clone and build
git clone https://github.com/bobmatnyc/ai-commander
cd ai-commander
cargo build --release

# Run TUI
./target/release/ai-commander tui

# Run REPL
./target/release/ai-commander repl
```

## Quick Start

1. Start the TUI: `ai-commander tui`
2. Create a project: `/connect /path/to/project -a claude-code -n myproject`
3. Send messages to interact with Claude Code
4. Use `/telegram` to enable mobile access
5. Use `/stop` to end session (auto-commits changes if in git repo)

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

1. Set `TELEGRAM_BOT_TOKEN` in `.env.local`
2. Run `/telegram` in TUI to generate a pairing code
3. In Telegram, send `/pair <code>` to your bot
4. Control sessions remotely from your phone

## Architecture

```
crates/
├── commander-core/      # Shared business logic (output filtering, summarization, config)
├── ai-commander/        # TUI and REPL interfaces (main binary)
├── commander-telegram/  # Telegram bot
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

## Configuration

| Variable | Description |
|----------|-------------|
| `COMMANDER_STATE_DIR` | Override state directory (default: `~/.commander/`) |
| `TELEGRAM_BOT_TOKEN` | Telegram bot token for remote control |
| `OPENROUTER_API_KEY` | API key for response summarization |

Environment variables can be set in `.env.local` in the project root.

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

## Development

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

## License

MIT License - see [LICENSE](LICENSE) for details.
