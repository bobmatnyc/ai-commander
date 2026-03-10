#!/bin/bash

# Health check script for AI Commander
# Run this periodically (e.g., via cron) to monitor system health

LOG_DIR="$HOME/.ai-commander/logs"
HEALTH_LOG="$LOG_DIR/health.log"
PLIST_FILE="$HOME/Library/LaunchAgents/com.ai-commander.telegram.plist"

mkdir -p "$LOG_DIR"

timestamp() {
    date "+%Y-%m-%d %H:%M:%S"
}

log() {
    echo "[$(timestamp)] $1" | tee -a "$HEALTH_LOG"
}

# Check if Telegram bot is running
check_bot() {
    if ! launchctl list | grep -q com.ai-commander.telegram; then
        log "⚠️  Bot is down, attempting restart..."
        if [ -f "$PLIST_FILE" ]; then
            launchctl load "$PLIST_FILE" 2>&1 | tee -a "$HEALTH_LOG"
            sleep 2
            if launchctl list | grep -q com.ai-commander.telegram; then
                log "✓ Bot restarted successfully"
            else
                log "❌ Bot restart failed"
                return 1
            fi
        else
            log "❌ Plist file not found: $PLIST_FILE"
            return 1
        fi
    else
        log "✓ Bot is running"
    fi
    return 0
}

# Check if tmux is available
check_tmux() {
    if ! command -v tmux &> /dev/null; then
        log "⚠️  tmux not found in PATH"
        return 1
    else
        log "✓ tmux is available"
    fi
    return 0
}

# Check disk space
check_disk() {
    DISK_USAGE=$(df -h ~ | awk 'NR==2 {print $5}' | sed 's/%//')
    if [ "$DISK_USAGE" -gt 90 ]; then
        log "⚠️  Disk usage critical: ${DISK_USAGE}%"
        return 1
    elif [ "$DISK_USAGE" -gt 80 ]; then
        log "⚠️  Disk usage high: ${DISK_USAGE}%"
        return 1
    else
        log "✓ Disk usage OK: ${DISK_USAGE}%"
    fi
    return 0
}

# Check log file sizes
check_logs() {
    if [ -f "$LOG_DIR/telegram.log" ]; then
        LOG_SIZE=$(du -h "$LOG_DIR/telegram.log" | cut -f1)
        log "Log size: $LOG_SIZE"

        # Rotate if larger than 50MB
        LOG_SIZE_MB=$(du -m "$LOG_DIR/telegram.log" | cut -f1)
        if [ "$LOG_SIZE_MB" -gt 50 ]; then
            log "Rotating large log file..."
            mv "$LOG_DIR/telegram.log" "$LOG_DIR/telegram.log.old"
            touch "$LOG_DIR/telegram.log"
        fi
    fi
    return 0
}

# Main health check
main() {
    log "=== Health Check Started ==="

    ALL_OK=true

    check_bot || ALL_OK=false
    check_tmux || ALL_OK=false
    check_disk || ALL_OK=false
    check_logs || ALL_OK=false

    if [ "$ALL_OK" = true ]; then
        log "✓ All health checks passed"
    else
        log "⚠️  Some health checks failed"
    fi

    log "=== Health Check Completed ==="
    log ""
}

main
