#!/usr/bin/env bash
# start-telegram.sh — launchd wrapper for commander-telegram
#
# Sourced by launchd; runs the telegram bot in foreground mode so launchd
# can track the process and restart it on crash or at boot.

set -euo pipefail

HOME_DIR="$HOME/.ai-commander"
CONFIG_ENV="$HOME_DIR/config/.env"

# Load environment variables
if [[ -f "$CONFIG_ENV" ]]; then
    set -a
    # shellcheck source=/dev/null
    source "$CONFIG_ENV"
    set +a
fi

exec "$HOME_DIR/bin/commander-telegram" -v
