# Stable Daemon Architecture Analysis and Design - ai-commander

**Date**: 2026-02-27
**Researcher**: Claude (with systematic debugging methodology)
**Goal**: Design a stable daemon architecture for ai-commander where core service runs continuously, Telegram bot connects to this service, and TUI/GUI can attach/detach as needed.

## Current Architecture Analysis

### 1. Current TUI, Telegram, and GUI Interfaces

**TUI/CLI Interface (`crates/ai-commander/`):**
- Main entry point via `ai-commander` binary
- Commands: `start`, `stop`, `list`, `status`, `send`, `repl`, `tui`
- Direct project management through REPL/TUI modes
- Current model: Each TUI session manages its own project instances

**Telegram Integration (`crates/commander-telegram/`):**
- Standalone binary `commander-telegram` with daemon support
- **Daemon functionality already exists**: `daemon.rs` provides:
  - `start()`, `stop()`, `restart()`, `is_running()`, `status()`
  - PID file management at `~/.ai-commander/state/telegram.pid`
  - Cross-platform process management (Unix: `kill`, Windows: `taskkill`)
  - Auto-build capability if binary missing
- **Background operation**: Runs as detached process with `Stdio::null()`
- **Service integration**: `setup-telegram-service.sh` creates launchd plist for macOS

**GUI Interface (`crates/commander-gui/`):**
- Tauri-based desktop application
- Background session polling via `events::start_session_polling()`
- Commands: session management, bot control, message sending
- Communicates with sessions through shared state mechanisms

### 2. Existing Daemon/Service Components

**Telegram Bot Daemon (`daemon.rs`):**
- Full lifecycle management (start, stop, restart, status)
- PID-based process tracking
- Health checking with process validation
- Binary location detection and auto-building
- Graceful shutdown (SIGTERM → 5s timeout → SIGKILL)

**Service Scripts:**
- `setup-telegram-service.sh`: Creates macOS launchd service
- `manage-services.sh`: Service management wrapper
- Cross-platform daemon management already implemented

### 3. Process Management and Stability Patterns

**Current Patterns:**
- **PID File Management**: `~/.ai-commander/state/telegram.pid`
- **Process Health Checking**: Cross-platform process validation
- **Auto-restart**: Service script handles keep-alive
- **Graceful Shutdown**: SIGTERM with fallback to SIGKILL
- **Environment Management**: Loads `.env.local` from config directory

**Stability Features:**
- HTTP client with 120s timeout, 30s connect timeout, connection pooling
- Retry logic for telegram API calls
- Error recovery and restart capabilities
- Logs to `~/.ai-commander/logs/telegram.log`

### 4. Memory Monitoring Capabilities

**Current Memory Components:**
- `commander-memory` crate for vector storage and retrieval
- Memory stores: local, Qdrant integration
- Context management and session memory
- Memory compaction for long-running sessions

**Missing**: System-level memory monitoring for process health

### 5. Inter-Process Communication Mechanisms

**Current IPC Patterns:**

**File-Based Communication:**
- **Pairing System**: `~/.ai-commander/state/pairings.json`
  - CLI generates codes, Telegram bot consumes
  - Expiry-based security (5 minutes)
  - Project name + session name mapping
- **State Files**: Shared state directory structure
- **Config Files**: Environment variables in `~/.ai-commander/config/`

**Shared State Architecture:**
- `commander-core::config` provides centralized state management
- State directory: `~/.ai-commander/` with subdirectories:
  - `state/`: Runtime state files, PID files
  - `config/`: Configuration and environment files
  - `logs/`: Application logs
  - `db/`: Database files (ChromaDB, etc.)
  - `cache/`: Temporary cache files

**Session Communication:**
- Event-based messaging through shared state
- Session polling mechanisms in GUI/TUI
- Project instance management via orchestrator

## Current Limitations and Issues

### 1. No Central Service Architecture
- TUI/CLI manage sessions directly
- Telegram bot operates independently
- No unified session management service
- Duplication of session logic across interfaces

### 2. Session State Fragmentation
- Each interface maintains its own view of sessions
- No single source of truth for active sessions
- State synchronization issues between interfaces

### 3. Process Isolation Issues
- TUI termination can kill managed sessions
- No session persistence across TUI restarts
- Interface-dependent session lifetimes

### 4. Memory Monitoring Gaps
- No system-level memory monitoring
- Process health limited to existence checking
- No automatic resource cleanup

### 5. Pairing Code Generation Limitation
- Currently requires TUI to generate pairing codes
- Goal: Generate pairing codes without launching TUI

## Proposed Stable Daemon Architecture

### Core Design Principles

1. **Single Source of Truth**: Central daemon manages all sessions
2. **Interface Independence**: TUI/GUI/Telegram are clients, not managers
3. **Session Persistence**: Sessions outlive interface connections
4. **Unified Communication**: All interfaces use same protocol to daemon
5. **Health Monitoring**: Built-in memory and process monitoring

### Architecture Components

```
┌─────────────────────────────────────────────────────────┐
│                  ai-commander-daemon                    │
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

### 1. Core Daemon Service (`ai-commander-daemon`)

**New Crate Structure:**
```
crates/commander-daemon/
├── src/
│   ├── main.rs              # Daemon entry point
│   ├── service.rs           # Core service implementation
│   ├── sessions.rs          # Session management
│   ├── monitoring.rs        # Memory & health monitoring
│   ├── ipc/
│   │   ├── mod.rs
│   │   ├── unix.rs          # Unix domain sockets
│   │   ├── windows.rs       # Named pipes
│   │   └── protocol.rs      # JSON-RPC protocol
│   └── pairing.rs           # Pairing code generation
├── Cargo.toml
└── README.md
```

**Service Responsibilities:**
- **Session Lifecycle**: Create, manage, monitor, terminate sessions
- **Project Management**: Start/stop project instances
- **Memory Monitoring**: Track memory usage, implement cleanup
- **State Persistence**: Maintain session state across restarts
- **Health Monitoring**: Process health, resource usage, recovery
- **Pairing Management**: Generate/validate pairing codes
- **Client Authentication**: Secure IPC connections

### 2. IPC Communication Layer

**Protocol Design:**
- **Transport**: Unix Domain Sockets (Unix) / Named Pipes (Windows)
- **Format**: JSON-RPC 2.0 for structured communication
- **Authentication**: Token-based with session management
- **Location**: `~/.ai-commander/state/daemon.sock`

**Message Types:**
```json
// Session Management
{
  "method": "session.create",
  "params": {
    "project_path": "/path/to/project",
    "adapter": "claude-code",
    "name": "optional-name"
  }
}

{
  "method": "session.list",
  "params": {}
}

{
  "method": "session.send",
  "params": {
    "session_id": "uuid",
    "message": "user message"
  }
}

// Pairing
{
  "method": "pairing.generate",
  "params": {
    "session_id": "uuid"
  }
}

// Monitoring
{
  "method": "status.health",
  "params": {}
}
```

### 3. Enhanced Telegram Bot Integration

**Modified Architecture:**
- Telegram bot becomes IPC client to daemon
- Remove independent session management
- All session operations via daemon IPC
- Pairing codes generated through daemon API

**Implementation Changes:**
```rust
// Current: Direct session management
let session = create_session(project_path)?;

// Proposed: Daemon IPC
let session_id = daemon_client
    .call("session.create", params)
    .await?;
```

### 4. Client Interface Modifications

**TUI/CLI Changes:**
- Convert to daemon client model
- Session attach/detach operations
- Local caching for responsiveness
- Graceful degradation if daemon unavailable

**GUI Changes:**
- Replace direct session polling with daemon IPC
- Real-time updates via daemon event streams
- Simplified session management UI

### 5. Memory Monitoring System

**Components:**
```rust
pub struct MemoryMonitor {
    session_limits: HashMap<SessionId, MemoryConfig>,
    global_limits: MemoryConfig,
    cleanup_policies: Vec<CleanupPolicy>,
}

pub struct MemoryConfig {
    max_memory_mb: u64,
    warning_threshold: f32,  // 0.8 = 80%
    cleanup_threshold: f32,  // 0.9 = 90%
}

pub enum CleanupPolicy {
    CompactMemory,
    TerminateOldestSession,
    PurgeCache,
    NotifyUser,
}
```

**Monitoring Features:**
- Per-session memory tracking
- System-wide memory limits
- Automatic cleanup policies
- Memory usage alerts
- Resource leak detection

### 6. Health Checking and Recovery

**Health Metrics:**
- Process memory usage
- Session response times
- IPC connection health
- File handle usage
- Database connection status

**Recovery Strategies:**
- Automatic session restart on failure
- Memory cleanup on threshold breach
- Connection recovery mechanisms
- Graceful degradation modes

## Implementation Plan

### Phase 1: Core Daemon Infrastructure (Week 1-2)

1. **Create `commander-daemon` crate**
   - Basic daemon structure
   - Service lifecycle management
   - Configuration loading

2. **Implement IPC Layer**
   - Unix domain socket server
   - JSON-RPC protocol handling
   - Basic authentication

3. **Session Management Core**
   - Session creation/termination
   - State persistence
   - Basic monitoring

### Phase 2: Client Integration (Week 3-4)

4. **Telegram Bot Migration**
   - Convert to IPC client
   - Remove direct session management
   - Implement daemon communication

5. **TUI/CLI Conversion**
   - Client library for daemon communication
   - Session attach/detach operations
   - Backward compatibility mode

6. **Pairing System Integration**
   - Daemon-based code generation
   - Remove TUI dependency

### Phase 3: Advanced Features (Week 5-6)

7. **Memory Monitoring**
   - Resource tracking implementation
   - Cleanup policies
   - Alert system

8. **Health Monitoring**
   - Process health checking
   - Recovery mechanisms
   - Metrics collection

9. **GUI Integration**
   - Convert to daemon client
   - Real-time updates
   - Enhanced session management

### Phase 4: Production Hardening (Week 7-8)

10. **Service Integration**
    - systemd service files
    - launchd plist updates
    - Windows service support

11. **Testing & Validation**
    - Integration tests
    - Load testing
    - Memory leak detection

12. **Migration & Documentation**
    - Smooth upgrade path
    - Performance benchmarks
    - User documentation

## Benefits of Proposed Architecture

### 1. Stability Improvements
- **Session Persistence**: Sessions survive interface restarts
- **Process Isolation**: Interface crashes don't affect sessions
- **Centralized Recovery**: Single point for health monitoring
- **Resource Management**: Unified memory monitoring and cleanup

### 2. Interface Independence
- **TUI Freedom**: Attach/detach without session disruption
- **GUI Scalability**: Multiple GUI instances possible
- **Telegram Reliability**: Bot failures don't affect sessions
- **Future Interfaces**: Easy addition of new interfaces

### 3. Operational Excellence
- **Unified Logging**: All operations through single service
- **Consistent State**: Single source of truth
- **Health Monitoring**: Proactive issue detection
- **Service Management**: Standard daemon management tools

### 4. Developer Experience
- **Simplified Debugging**: Centralized session logic
- **Clear Boundaries**: Well-defined interface contracts
- **Testability**: Mock daemon for testing clients
- **Maintainability**: Reduced code duplication

## Migration Strategy

### Backward Compatibility
- **Gradual Migration**: Support both modes during transition
- **Fallback Mode**: Direct session management if daemon unavailable
- **Configuration Flag**: Enable daemon mode via config
- **Smooth Upgrade**: Automatic daemon installation

### Risk Mitigation
- **Feature Flags**: Enable/disable daemon features
- **Rollback Plan**: Quick revert to current architecture
- **Testing Strategy**: Comprehensive integration tests
- **Monitoring**: Real-time performance tracking during migration

## Conclusion

The proposed daemon architecture addresses all current limitations while building on existing strengths of the ai-commander system. The Telegram bot daemon infrastructure provides a solid foundation, and the file-based IPC patterns demonstrate viable communication mechanisms.

Key advantages:
1. **Leverages Existing**: Builds on proven telegram daemon patterns
2. **Solves Core Issues**: Session persistence, interface independence, resource monitoring
3. **Enables Future Growth**: Foundation for additional interfaces and features
4. **Operational Excellence**: Production-ready monitoring and management

The phased implementation approach minimizes risk while delivering incremental value. The architecture supports the specific goal of pairing code generation without TUI launch while providing a foundation for long-term system stability and scalability.