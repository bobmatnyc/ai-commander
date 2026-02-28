# Commander Daemon

Central daemon service for ai-commander session management. This daemon provides a stable, persistent service that manages all AI sessions, handles IPC communication with clients, and enables pairing code generation without requiring TUI launch.

## Overview

The `commander-daemon` is Phase 1 of the stable daemon architecture, implementing:

- **Session Management**: Centralized session lifecycle (create, list, terminate)
- **IPC Communication**: Unix domain socket server with JSON-RPC protocol
- **Pairing System**: Generate pairing codes without TUI dependency
- **Memory Monitoring**: Built-in resource tracking and cleanup
- **Health Checking**: Process monitoring and status reporting

## Architecture

```text
┌─────────────────────────────────────────────────────────┐
│                  commander-daemon                       │
│  ┌─────────────────────────────────────────────────┐    │
│  │              Core Service                       │    │
│  │  • Session Management                           │    │
│  │  • Project Orchestration                       │    │
│  │  • Memory Monitoring                           │    │
│  │  • State Persistence                           │    │
│  │  • Health Checking                             │    │
│  └─────────────────────────────────────────────────┘    │
│  ┌─────────────────────────────────────────────────┐    │
│  │              IPC Layer                         │    │
│  │  • Unix Domain Sockets / Named Pipes          │    │
│  │  • JSON-RPC Protocol                          │    │
│  │  • Authentication                             │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
              │                │                │
    ┌─────────┘      ┌─────────┘      ┌─────────┘
    │                │                │
┌───▼────┐     ┌─────▼──────┐    ┌────▼─────┐
│   TUI  │     │ Telegram   │    │   GUI    │
│ Client │     │    Bot     │    │  Client  │
└────────┘     └────────────┘    └──────────┘
```

## Usage

### Via Main CLI

The daemon commands are integrated into the main `ai-commander` CLI:

```bash
# Check daemon status
ai-commander daemon status

# Start daemon in foreground (for development)
ai-commander daemon start --foreground

# Start daemon in background
ai-commander daemon start

# Stop daemon
ai-commander daemon stop

# Restart daemon
ai-commander daemon restart

# Generate pairing code (works without daemon running)
ai-commander pair

# Generate pairing code for specific session
ai-commander pair --session <session-id>
```

### Standalone Daemon Binary

The daemon can also be run as a standalone service:

```bash
# Run standalone daemon
commander-daemon start --foreground

# Check status via standalone binary
commander-daemon status

# Generate pairing code
commander-daemon pair
```

## Configuration

The daemon uses the existing `~/.ai-commander/` directory structure:

```
~/.ai-commander/
├── config/          # Configuration files
│   └── .env.local    # Environment variables
├── logs/            # Application logs
│   └── daemon.log    # Daemon-specific logs
├── state/           # Runtime state
│   ├── daemon.sock   # Unix domain socket
│   ├── daemon.pid    # Process ID file
│   ├── pairings.json # Pairing codes
│   └── sessions/     # Session state
├── db/              # Databases
└── cache/           # Temporary files
```

## IPC Protocol

The daemon communicates via JSON-RPC 2.0 over Unix domain sockets:

### Session Management
```json
// Create session
{"jsonrpc": "2.0", "method": "session.create", "params": {"project_path": "/path", "adapter": "claude-code"}, "id": 1}

// List sessions
{"jsonrpc": "2.0", "method": "session.list", "params": {}, "id": 2}

// Send message
{"jsonrpc": "2.0", "method": "session.send", "params": {"session_id": "uuid", "message": "Hello"}, "id": 3}
```

### Pairing
```json
// Generate pairing code
{"jsonrpc": "2.0", "method": "pairing.generate", "params": {"session_id": "uuid"}, "id": 4}

// Validate pairing code
{"jsonrpc": "2.0", "method": "pairing.validate", "params": {"code": "ABC123"}, "id": 5}
```

### Health & Monitoring
```json
// Get health status
{"jsonrpc": "2.0", "method": "status.health", "params": {}, "id": 6}

// Get memory status
{"jsonrpc": "2.0", "method": "status.memory", "params": {}, "id": 7}
```

## Memory Monitoring

The daemon includes built-in memory monitoring with configurable limits:

- **Per-session tracking**: Monitor memory usage for each session
- **Global limits**: System-wide memory thresholds
- **Automatic cleanup**: Configurable cleanup policies
- **Health alerts**: Warning and critical thresholds

Default configuration:
- Max memory: 1GB per session, 2GB global
- Warning threshold: 80%
- Cleanup threshold: 90%
- Monitoring interval: 30 seconds

## Pairing System

Pairing codes allow secure client connections without manual configuration:

- **5-minute expiry**: Codes are valid for 5 minutes after generation
- **Single use**: Each code can only be used once
- **Project association**: Codes can be linked to specific sessions/projects
- **Client tracking**: Records which client used each code

Generated codes are 6-character alphanumeric strings (e.g., "A1B2C3").

## Development Status

This is **Phase 1** of the daemon architecture implementation, providing:

✅ **Completed**:
- Core daemon service lifecycle
- Session management via orchestrator integration
- IPC server with JSON-RPC protocol
- Pairing code generation and validation
- CLI integration (`ai-commander daemon`, `ai-commander pair`)
- Basic memory monitoring framework
- Health status reporting

**Future Phases**:
- **Phase 2**: Telegram bot integration with daemon
- **Phase 3**: TUI/GUI client integration
- **Phase 4**: Production hardening and service installation

## Building and Testing

```bash
# Build the daemon
cargo build -p commander-daemon

# Run tests
cargo test -p commander-daemon

# Check compilation
cargo check -p commander-daemon

# Test daemon status
cargo run --bin ai-commander -- daemon status

# Test pairing generation
cargo run --bin ai-commander -- pair
```

The implementation successfully compiles and provides working CLI commands for daemon management and pairing code generation.

## Dependencies

The daemon builds on existing ai-commander infrastructure:

- `commander-core`: Configuration and state management
- `commander-orchestrator`: Agent orchestration
- `commander-memory`: Memory storage systems
- Standard async runtime: `tokio`, `futures`
- Serialization: `serde`, `serde_json`
- IPC: Unix domain sockets
- Signal handling: `signal-hook`, `signal-hook-tokio`

This foundational implementation provides the core infrastructure for the stable daemon architecture described in the research analysis.