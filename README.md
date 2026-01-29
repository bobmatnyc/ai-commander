# Commander

Multi-project AI orchestration system written in Rust.

> **Status**: ✅ All 8 phases complete! (293 tests passing)

## Quick Start

```bash
# Build
cargo build

# Run CLI
cargo run -p commander-cli -- --help

# Enter interactive REPL
cargo run -p commander-cli

# List available adapters
cargo run -p commander-cli -- adapters

# Run all tests
cargo test
```

## Overview

Commander manages multiple AI coding tool instances (Claude Code, Aider, etc.) across projects, providing:

- **Project Management**: Track multiple projects with isolated state ✅
- **Work Queue**: Priority-based task execution with dependencies ✅
- **Event System**: Notifications, decisions, and approvals inbox ✅
- **CLI & REPL**: Interactive command-line interface ✅
- **REST API**: Programmatic control via axum ✅

## Project Structure

```
.
├── Cargo.toml                    # Workspace root
└── crates/
    ├── commander-models/         # ✅ Phase 1: Core data types
    ├── commander-persistence/    # ✅ Phase 2: JSON file storage
    ├── commander-adapters/       # ✅ Phase 3: Runtime adapters
    ├── commander-cli/            # ✅ Phase 4: CLI and REPL
    ├── commander-events/         # ✅ Phase 5: Event system
    ├── commander-work/           # ✅ Phase 5: Work queue
    ├── commander-tmux/           # ✅ Phase 6: Tmux orchestration
    ├── commander-runtime/        # ✅ Phase 7: Async runtime
    └── commander-api/            # ✅ Phase 8: REST API
```

## CLI Commands

```bash
commander start <path>       # Start a project instance
commander stop <project>     # Stop a project
commander list               # List all projects
commander status [project]   # Show project status
commander adapters           # List runtime adapters
commander repl               # Interactive REPL mode
```

## REPL Commands

```
/list, /ls       List all projects
/status [proj]   Show project status
/connect <proj>  Connect to a project
/disconnect      Disconnect from current project
/help            Show help
/quit            Exit
```

## REST API Endpoints

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

## Development Phases

| Phase | Crate | Status | Rust Concepts |
|-------|-------|--------|---------------|
| 1 | commander-models | ✅ Done | struct, enum, derive, Option, Vec, serde, newtype |
| 2 | commander-persistence | ✅ Done | Result<T,E>, ?, thiserror, atomic file I/O |
| 3 | commander-adapters | ✅ Done | trait, Box<dyn>, Arc<dyn>, Send+Sync, regex |
| 4 | commander-cli | ✅ Done | clap, rustyline, tracing, match |
| 5 | commander-events/work | ✅ Done | mpsc, Arc<Mutex>, Arc<RwLock>, BinaryHeap |
| 6 | commander-tmux | ✅ Done | std::process::Command, output parsing |
| 7 | commander-runtime | ✅ Done | tokio, async/await, select!, broadcast, watch |
| 8 | commander-api | ✅ Done | axum, tower-http, REST API, JSON |

## Testing

```bash
# Run all tests (293 tests)
cargo test

# Run specific crate tests
cargo test -p commander-models
cargo test -p commander-persistence
cargo test -p commander-adapters
cargo test -p commander-cli
cargo test -p commander-events
cargo test -p commander-work
cargo test -p commander-tmux
cargo test -p commander-runtime
cargo test -p commander-api

# Run tmux integration tests (requires tmux)
cargo test -p commander-tmux -- --ignored
```

## License

MIT License - see [LICENSE](LICENSE) for details.
