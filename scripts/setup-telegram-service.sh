#!/bin/bash
set -e

echo "=== Setting up Telegram Bot as System Service ==="
echo ""

# Check if CLI is installed
if ! command -v ai-commander &> /dev/null; then
    echo "Error: ai-commander not found in PATH"
    echo "Run ./scripts/install-local.sh first"
    exit 1
fi

# Create log directory
LOG_DIR="$HOME/.ai-commander/logs"
mkdir -p "$LOG_DIR"
echo "Created log directory: $LOG_DIR"

# Create launchd plist
PLIST_FILE="$HOME/Library/LaunchAgents/com.ai-commander.telegram.plist"

cat > "$PLIST_FILE" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.ai-commander.telegram</string>

    <key>ProgramArguments</key>
    <array>
        <string>/Users/masa/.local/bin/ai-commander</string>
        <string>telegram</string>
        <string>start</string>
    </array>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>

    <key>StandardOutPath</key>
    <string>$LOG_DIR/telegram.log</string>

    <key>StandardErrorPath</key>
    <string>$LOG_DIR/telegram-error.log</string>

    <key>WorkingDirectory</key>
    <string>$HOME</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>$HOME</string>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
    </dict>

    <key>ThrottleInterval</key>
    <integer>10</integer>
</dict>
</plist>
EOF

echo "Created launchd plist: $PLIST_FILE"

# Unload if already loaded
launchctl unload "$PLIST_FILE" 2>/dev/null || true

# Load the service
echo "Loading service..."
launchctl load "$PLIST_FILE"

# Give it a moment to start
sleep 2

# Check status
echo ""
echo "Service status:"
if launchctl list | grep -q com.ai-commander.telegram; then
    echo "✓ Telegram bot service is running"
    echo ""
    echo "View logs with:"
    echo "  tail -f $LOG_DIR/telegram.log"
    echo ""
    echo "Manage service with:"
    echo "  ./scripts/manage-services.sh {start|stop|restart|status|logs}"
else
    echo "⚠️  Service may not be running. Check logs:"
    echo "  cat $LOG_DIR/telegram-error.log"
fi

echo ""
echo "The Telegram bot will now start automatically on boot."
