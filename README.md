# Commander

Multi-project AI orchestration system written in Rust.

> **Status**: Active development - Phase 4 complete (CLI works!)

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
```

## Overview

Commander manages multiple AI coding tool instances (Claude Code, Aider, etc.) across projects, providing:

- **Project Management**: Track multiple projects with isolated state
- **Work Queue**: Priority-based task execution with dependencies
- **Event System**: Notifications, decisions, and approvals inbox
- **CLI & REPL**: Interactive command-line interface ✅
- **REST API**: Programmatic control (coming in Phase 8)

## Project Structure

```
.
├── Cargo.toml                    # Workspace root
└── crates/
    ├── commander-models/         # ✅ Phase 1: Core data types
    ├── commander-persistence/    # ✅ Phase 2: JSON file storage
    ├── commander-adapters/       # ✅ Phase 3: Runtime adapters
    ├── commander-cli/            # ✅ Phase 4: CLI and REPL
    ├── commander-events/         # Phase 5: Event system
    ├── commander-work/           # Phase 5: Work queue
    ├── commander-tmux/           # Phase 6: Tmux orchestration
    ├── commander-runtime/        # Phase 7: Async runtime
    └── commander-api/            # Phase 8: REST API
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

## Development Phases

| Phase | Crate | Status | Rust Concepts |
|-------|-------|--------|---------------|
| 1 | commander-models | ✅ Done | struct, enum, derive, Option, Vec, serde |
| 2 | commander-persistence | ✅ Done | Result<T,E>, ?, thiserror, file I/O |
| 3 | commander-adapters | ✅ Done | trait, Box<dyn>, Send+Sync, regex |
| 4 | commander-cli | ✅ Done | clap, rustyline, tracing, match |
| 5 | commander-events/work | 🔜 Next | mpsc, Arc<Mutex>, channels |
| 6 | commander-tmux | Planned | std::process::Command |
| 7 | commander-runtime | Planned | tokio, async/await |
| 8 | commander-api | Planned | axum, REST API |

## Testing

```bash
# Run all tests (140 tests)
cargo test

# Run specific crate tests
cargo test -p commander-models
cargo test -p commander-persistence
cargo test -p commander-adapters
cargo test -p commander-cli
```

## License

MIT License - see [LICENSE](LICENSE) for details.
