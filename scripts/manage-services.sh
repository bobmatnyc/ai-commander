#!/bin/bash

PLIST_FILE="$HOME/Library/LaunchAgents/com.ai-commander.telegram.plist"
LOG_DIR="$HOME/.ai-commander/logs"

case "$1" in
  start)
    echo "Starting AI Commander services..."

    # Start Telegram bot
    if [ -f "$PLIST_FILE" ]; then
        launchctl load "$PLIST_FILE" 2>/dev/null || echo "Telegram bot already running"
    else
        echo "⚠️  Telegram service not set up. Run: ./scripts/setup-telegram-service.sh"
    fi

    # Start GUI
    if [ -d "$HOME/Applications/AI Commander.app" ]; then
        open -a "AI Commander" 2>/dev/null && echo "✓ GUI started" || echo "GUI already running"
    else
        echo "⚠️  GUI not installed. Run: ./scripts/install-local.sh"
    fi
    ;;

  stop)
    echo "Stopping AI Commander services..."

    # Stop Telegram bot
    if [ -f "$PLIST_FILE" ]; then
        launchctl unload "$PLIST_FILE" 2>/dev/null && echo "✓ Telegram bot stopped" || echo "Telegram bot not running"
    fi

    # Stop GUI
    pkill -f "AI Commander" 2>/dev/null && echo "✓ GUI stopped" || echo "GUI not running"
    ;;

  restart)
    echo "Restarting AI Commander services..."
    $0 stop
    sleep 2
    $0 start
    ;;

  status)
    echo "=== AI Commander Service Status ==="
    echo ""
    echo "--- Telegram Bot ---"
    if launchctl list | grep -q com.ai-commander.telegram; then
        echo "✓ Running"
        PID=$(launchctl list | grep com.ai-commander.telegram | awk '{print $1}')
        if [ "$PID" != "-" ]; then
            echo "  PID: $PID"
        fi
    else
        echo "✗ Not running"
    fi

    echo ""
    echo "--- GUI ---"
    if pgrep -f "AI Commander" > /dev/null; then
        echo "✓ Running"
        PID=$(pgrep -f "AI Commander")
        echo "  PID: $PID"
    else
        echo "✗ Not running"
    fi

    echo ""
    echo "--- Recent Logs ---"
    if [ -f "$LOG_DIR/telegram.log" ]; then
        echo "Last 5 lines of telegram.log:"
        tail -5 "$LOG_DIR/telegram.log" 2>/dev/null || echo "  (empty)"
    else
        echo "No logs found at $LOG_DIR/telegram.log"
    fi

    echo ""
    echo "--- System Resources ---"
    echo "Disk usage for home directory:"
    df -h ~ | awk 'NR==2 {print "  " $5 " used (" $3 " / " $2 ")"}'
    ;;

  logs)
    if [ -f "$LOG_DIR/telegram.log" ]; then
        echo "Streaming telegram.log (Ctrl+C to stop)..."
        tail -f "$LOG_DIR/telegram.log"
    else
        echo "Log file not found: $LOG_DIR/telegram.log"
        echo "Check if service is set up: ./scripts/setup-telegram-service.sh"
    fi
    ;;

  logs-error)
    if [ -f "$LOG_DIR/telegram-error.log" ]; then
        echo "Error log contents:"
        cat "$LOG_DIR/telegram-error.log"
    else
        echo "Error log file not found: $LOG_DIR/telegram-error.log"
    fi
    ;;

  *)
    echo "AI Commander Service Manager"
    echo ""
    echo "Usage: $0 {start|stop|restart|status|logs|logs-error}"
    echo ""
    echo "Commands:"
    echo "  start        - Start Telegram bot and GUI"
    echo "  stop         - Stop all services"
    echo "  restart      - Restart all services"
    echo "  status       - Show service status and recent logs"
    echo "  logs         - Stream Telegram bot logs (live)"
    echo "  logs-error   - Show Telegram bot error logs"
    echo ""
    echo "Examples:"
    echo "  $0 status"
    echo "  $0 logs"
    exit 1
    ;;
esac
