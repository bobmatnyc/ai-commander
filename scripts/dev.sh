#!/usr/bin/env bash
# scripts/dev.sh - Development script with auto-reload for commander-telegram
#
# Watches for file changes and automatically rebuilds and restarts the Telegram bot.
# Requires cargo-watch to be installed.
#
# Usage:
#   ./scripts/dev.sh          # Start dev mode
#   ./scripts/dev.sh --help   # Show help

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BINARY_NAME="commander-telegram"
TARGET_PATH="$PROJECT_ROOT/target/release/$BINARY_NAME"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

info() { echo -e "${BLUE}[info]${NC} $1"; }
success() { echo -e "${GREEN}[ok]${NC} $1"; }
warn() { echo -e "${YELLOW}[warn]${NC} $1"; }
error() { echo -e "${RED}[error]${NC} $1"; }

# Show help
show_help() {
    echo ""
    echo "Usage: $0 [options]"
    echo ""
    echo "Development script with auto-reload for commander-telegram."
    echo "Watches for file changes and automatically rebuilds/restarts the bot."
    echo ""
    echo "Options:"
    echo "  -h, --help     Show this help message"
    echo "  -v, --verbose  Enable verbose logging for the bot"
    echo "  --debug        Enable debug build (faster compilation, slower runtime)"
    echo ""
    echo "Environment variables:"
    echo "  TELEGRAM_BOT_TOKEN    Required: Your Telegram bot token"
    echo ""
    echo "The bot handles SIGTERM gracefully, so session state persists across restarts."
    echo ""
}

# Check dependencies
check_dependencies() {
    if ! command -v cargo-watch &> /dev/null; then
        warn "cargo-watch not found. Installing..."
        cargo install cargo-watch
        success "cargo-watch installed"
    fi
}

# Kill existing bot process
kill_existing() {
    if pgrep -f "$BINARY_NAME" > /dev/null 2>&1; then
        info "Stopping existing bot process..."
        pkill -TERM -f "$BINARY_NAME" || true
        # Give it a moment to shut down gracefully
        sleep 1
        # Force kill if still running
        pkill -KILL -f "$BINARY_NAME" 2>/dev/null || true
    fi
}

# Cleanup on exit
cleanup() {
    echo ""
    info "Shutting down dev mode..."
    kill_existing
    success "Cleanup complete"
}

# Main
main() {
    local verbose=""
    local build_profile="release"
    local build_flags="--release"

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_help
                exit 0
                ;;
            -v|--verbose)
                verbose="-v"
                shift
                ;;
            --debug)
                build_profile="debug"
                build_flags=""
                shift
                ;;
            *)
                error "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done

    # Update target path for debug builds
    if [[ "$build_profile" == "debug" ]]; then
        TARGET_PATH="$PROJECT_ROOT/target/debug/$BINARY_NAME"
    fi

    cd "$PROJECT_ROOT"

    echo ""
    echo -e "${CYAN}======================================${NC}"
    echo -e "${CYAN}  Commander Telegram - Dev Mode${NC}"
    echo -e "${CYAN}======================================${NC}"
    echo ""

    # Check dependencies
    check_dependencies

    # Check for bot token
    if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
        # Try loading from .env files
        if [[ -f "$HOME/.config/ai-commander/.env.local" ]]; then
            source "$HOME/.config/ai-commander/.env.local" 2>/dev/null || true
        fi
        if [[ -f "$PROJECT_ROOT/.env.local" ]]; then
            source "$PROJECT_ROOT/.env.local" 2>/dev/null || true
        fi
        if [[ -f "$PROJECT_ROOT/.env" ]]; then
            source "$PROJECT_ROOT/.env" 2>/dev/null || true
        fi
    fi

    if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
        error "TELEGRAM_BOT_TOKEN not set"
        echo "  Set it in your environment or add to .env.local"
        exit 1
    fi

    info "Bot token found"
    info "Build profile: $build_profile"
    info "Watching for changes in crates/"
    echo ""

    # Set up cleanup trap
    trap cleanup EXIT INT TERM

    # Kill any existing bot process
    kill_existing

    # Start watching and rebuilding
    # -w: watch paths
    # -x: execute cargo command
    # -s: shell command to run after successful build
    # -c: clear screen before each run
    # -q: quiet cargo output (only show errors)
    # --why: show what triggered rebuild
    #
    # Note: Builds BOTH ai-commander (TUI) and commander-telegram binaries.
    # Only the Telegram bot is auto-restarted here; user runs TUI separately
    # and must manually restart it to pick up changes.
    cargo watch \
        -w "$PROJECT_ROOT/crates" \
        -x "build $build_flags -p ai-commander -p commander-telegram" \
        -s "
            echo ''
            echo -e '${GREEN}[ok]${NC} Build successful - restarting bot...'
            pkill -TERM -f '$BINARY_NAME' 2>/dev/null || true
            sleep 0.5
            '$TARGET_PATH' $verbose &
            echo -e '${BLUE}[info]${NC} Bot started (PID: \$!)'
            echo ''
        " \
        --why
}

main "$@"
