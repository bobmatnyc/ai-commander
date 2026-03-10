# Convenience Scripts

Quick shortcuts for common ai-commander development tasks.

## Core Scripts

### `./ai` - Main wrapper
Universal wrapper for all ai-commander commands with shortcuts:

```bash
./ai help              # Show help with shortcuts
./ai status            # Daemon status (shortcut for daemon status)
./ai start             # Start daemon (shortcut for daemon start)
./ai stop              # Stop daemon (shortcut for daemon stop)
./ai restart           # Restart daemon (shortcut for daemon restart)
./ai pair              # Generate pairing code
./ai tui               # Start TUI
./ai list              # List projects

# Pass through any other commands:
./ai send "message"    # Send message to session
./ai repl              # Start REPL mode
```

### `./daemon` - Daemon management
Direct daemon commands:

```bash
./daemon start         # Start background daemon
./daemon stop          # Stop daemon
./daemon status        # Check daemon health & sessions
./daemon restart       # Restart daemon
```

### `./pair` - Quick pairing
Generate Telegram pairing codes:

```bash
./pair                 # Generate new pairing code
./pair --session xyz   # Generate code for specific session
```

### `./build` - Build management
Smart build wrapper:

```bash
./build                # Release build (ai-commander + commander-daemon)
./build --debug        # Debug build
./build --all          # Build all crates including GUI & Telegram
```

## Features

**Smart Binary Detection**: Scripts automatically find the best available binary (release preferred, debug fallback).

**Colored Output**: Clear visual feedback with colored status messages.

**Error Handling**: Helpful error messages if binaries aren't built.

**Pass-through**: All non-shortcut commands passed directly to ai-commander.

## Quick Start

```bash
# Build everything
./build

# Start stable daemon
./daemon start

# Generate pairing code
./pair
# Output: Pairing code: 5BC272 (valid for 5 minutes)

# Check status
./daemon status
# Output: JSON with daemon health, sessions, config

# Stop when done
./daemon stop
```

## Development Workflow

```bash
# Build and test cycle
./build --debug        # Faster debug builds during development
./daemon restart       # Restart with new code
./ai status           # Verify it's working

# Production build
./build               # Release builds for production use
./daemon start        # Start stable service
```

These scripts make the development workflow much smoother by eliminating the need to type long paths and remember exact command syntax.