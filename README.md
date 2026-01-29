# Commander

Multi-project AI orchestration system written in Rust.

> **Status**: Active development - Rust rewrite in progress

## Overview

Commander manages multiple AI coding tool instances (Claude Code, Aider, etc.) across projects, providing:

- **Project Management**: Track multiple projects with isolated state
- **Work Queue**: Priority-based task execution with dependencies
- **Event System**: Notifications, decisions, and approvals inbox
- **CLI & REPL**: Interactive command-line interface
- **REST API**: Programmatic control (coming in Phase 8)

## Building

```bash
cd commander-rs

# Build all crates
cargo build

# Run tests
cargo test

# Run with optimizations
cargo build --release
```

## Project Structure

```
commander-rs/
├── Cargo.toml                    # Workspace root
└── crates/
    ├── commander-models/         # ✅ Phase 1: Core data types
    ├── commander-persistence/    # Phase 2: JSON file storage
    ├── commander-adapters/       # Phase 3: Runtime adapters
    ├── commander-cli/            # Phase 4: CLI and REPL
    ├── commander-events/         # Phase 5: Event system
    ├── commander-work/           # Phase 5: Work queue
    ├── commander-tmux/           # Phase 6: Tmux orchestration
    ├── commander-runtime/        # Phase 7: Async runtime
    ├── commander-api/            # Phase 8: REST API
    └── commander/                # Phase 8: Main binary
```

## Development Phases

| Phase | Crate | Status | Focus |
|-------|-------|--------|-------|
| 1 | commander-models | ✅ Done | struct, enum, serde, Option, Vec |
| 2 | commander-persistence | 🔜 Next | Result, ?, thiserror, file I/O |
| 3 | commander-adapters | Planned | trait, Box<dyn>, generics |
| 4 | commander-cli | Planned | clap, rustyline REPL |
| 5 | commander-events/work | Planned | mpsc, Arc<Mutex>, channels |
| 6 | commander-tmux | Planned | std::process::Command |
| 7 | commander-runtime | Planned | tokio, async/await |
| 8 | commander-api | Planned | axum, REST API |

## License

MIT License - see [LICENSE](LICENSE) for details.
