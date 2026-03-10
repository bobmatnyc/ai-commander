# AI Commander - Local Deployment Guide

Complete guide for stable production-like deployment on macOS.

## Overview

This guide sets up AI Commander for reliable local operation with:
- Production-optimized binaries (no dev mode required)
- Automatic Telegram bot startup on boot
- Easy service management
- Health monitoring
- Comprehensive logging

## Prerequisites

- macOS (Darwin 25.2.0 or later)
- Rust toolchain (for building)
- Node.js and npm (for GUI frontend)
- Telegram bot token (if using Telegram features)
- tmux (for session management)

## Quick Start

### 1. Build Production Binaries

```bash
cd /Users/masa/Projects/ai-commander

# Build CLI
cargo build --release -p ai-commander

# Build GUI frontend
cd crates/commander-gui/ui
npm install
npm run build
cd ..

# Build GUI Tauri app
cargo tauri build
```

**Build times:**
- CLI: ~10 seconds (incremental)
- GUI: ~40 seconds (incremental)

**Output locations:**
- CLI: `target/release/ai-commander` (~14MB)
- GUI: `target/release/commander-gui` (~8.4MB)

### 2. Install Binaries

```bash
cd /Users/masa/Projects/ai-commander
./scripts/install-local.sh
```

**What it does:**
- Installs CLI to `/usr/local/bin/ai-commander`
- Creates `AI Commander.app` in `~/Applications/`
- Sets proper permissions and removes quarantine attributes

**Verify installation:**
```bash
ai-commander --version
open -a "AI Commander"
```

### 3. Setup Telegram Bot Service

```bash
./scripts/setup-telegram-service.sh
```

**What it does:**
- Creates launchd service for auto-start on boot
- Configures logging to `~/.ai-commander/logs/`
- Starts the Telegram bot immediately
- Enables automatic restart on crash

**Check service status:**
```bash
./scripts/manage-services.sh status
```

## Daily Usage

### Start All Services

```bash
./scripts/manage-services.sh start
```

Starts:
- Telegram bot (if not already running)
- GUI application

### Check Service Status

```bash
./scripts/manage-services.sh status
```

Shows:
- Telegram bot status and PID
- GUI status and PID
- Recent log entries
- Disk usage

### View Logs

```bash
# Stream live logs
./scripts/manage-services.sh logs

# View error logs
./scripts/manage-services.sh logs-error
```

### Stop Services

```bash
./scripts/manage-services.sh stop
```

### Restart Services

```bash
./scripts/manage-services.sh restart
```

## Service Management Details

### Telegram Bot Service

The Telegram bot runs as a launchd user agent, which means:
- **Auto-starts on login**: Service loads when you log in
- **Auto-restart on crash**: Service restarts if it exits unexpectedly
- **Throttled restarts**: 10-second delay between restart attempts
- **Persistent logs**: All output captured to log files

**Service file location:**
```
~/Library/LaunchAgents/com.ai-commander.telegram.plist
```

**Manual service control:**
```bash
# Stop service
launchctl unload ~/Library/LaunchAgents/com.ai-commander.telegram.plist

# Start service
launchctl load ~/Library/LaunchAgents/com.ai-commander.telegram.plist

# Check if running
launchctl list | grep ai-commander
```

### Log Files

**Locations:**
- Standard output: `~/.ai-commander/logs/telegram.log`
- Error output: `~/.ai-commander/logs/telegram-error.log`
- Health checks: `~/.ai-commander/logs/health.log`

**Log rotation:**
Logs larger than 50MB are automatically rotated by `health-check.sh`.

**Manual log cleanup:**
```bash
# Clear old logs
rm ~/.ai-commander/logs/*.log.old

# Clear all logs (service must be stopped first)
./scripts/manage-services.sh stop
rm ~/.ai-commander/logs/*.log
./scripts/manage-services.sh start
```

## Health Monitoring

### Automated Health Checks

The `health-check.sh` script monitors:
- Telegram bot running status
- tmux availability
- Disk space usage
- Log file sizes

**Run manually:**
```bash
./scripts/health-check.sh
```

**Setup automated checks (optional):**

Add to crontab to run every hour:
```bash
crontab -e
```

Add this line:
```
0 * * * * /Users/masa/Projects/ai-commander/scripts/health-check.sh
```

**Health check actions:**
- Restarts bot if down
- Warns on high disk usage (>80%)
- Rotates large log files (>50MB)
- Logs all checks to `~/.ai-commander/logs/health.log`

## Troubleshooting

### CLI Not Found After Installation

**Problem:** `command not found: ai-commander`

**Solution:**
```bash
# Check if installed
ls -l /usr/local/bin/ai-commander

# If not, run installer again
./scripts/install-local.sh

# Add to PATH if needed
echo 'export PATH="/usr/local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

### GUI Won't Open

**Problem:** Double-clicking app does nothing or shows error

**Solution:**
```bash
# Remove quarantine attribute
xattr -cr ~/Applications/"AI Commander.app"

# Try opening from terminal
open -a "AI Commander"

# Check for errors
open -a "AI Commander" 2>&1
```

### Telegram Bot Not Starting

**Problem:** Service loads but bot doesn't respond

**Check logs:**
```bash
./scripts/manage-services.sh logs-error
```

**Common issues:**
1. **Missing TELEGRAM_BOT_TOKEN**: Set in environment or `.env` file
2. **Network connectivity**: Check internet connection
3. **Invalid token**: Verify token with BotFather

**Solution:**
```bash
# Set token (if not in .env)
export TELEGRAM_BOT_TOKEN="your_token_here"

# Restart service
./scripts/manage-services.sh restart

# Verify logs
./scripts/manage-services.sh logs
```

### Service Crashes Immediately

**Problem:** Bot starts but exits within seconds

**Check error log:**
```bash
cat ~/.ai-commander/logs/telegram-error.log
```

**Common causes:**
- Missing dependencies (tmux, etc.)
- Configuration errors
- Permission issues

**Debug manually:**
```bash
# Stop service
./scripts/manage-services.sh stop

# Run manually to see errors
ai-commander telegram start
```

### High Disk Usage Warning

**Problem:** Health check reports high disk usage

**Solution:**
```bash
# Check log sizes
du -h ~/.ai-commander/logs/

# Clear old logs
rm ~/.ai-commander/logs/*.log.old

# Check tmux session storage
du -h ~/.ai-commander/
```

### Permissions Issues

**Problem:** "Permission denied" errors

**Solution:**
```bash
# Fix CLI permissions
sudo chmod +x /usr/local/bin/ai-commander

# Fix app bundle permissions
chmod +x ~/Applications/"AI Commander.app"/Contents/MacOS/"AI Commander"

# Reinstall if needed
./scripts/install-local.sh
```

## Advanced Configuration

### Environment Variables

The Telegram bot service uses these environment variables:

**Set in plist file** (`~/Library/LaunchAgents/com.ai-commander.telegram.plist`):
- `HOME`: User home directory
- `PATH`: Standard PATH with `/usr/local/bin`

**Set in shell environment** (before starting):
- `TELEGRAM_BOT_TOKEN`: Your bot token
- `RUST_LOG`: Logging level (info, debug, trace)
- `AI_COMMANDER_CONFIG`: Custom config file path

**Example custom configuration:**
```bash
# Create custom config
cat > ~/.ai-commander/config.toml <<EOF
[telegram]
token = "your_token_here"
polling_timeout = 10

[logging]
level = "info"
EOF

# Update plist to use custom config
# (Edit EnvironmentVariables section)
```

### Custom Aliases

Add to `~/.zshrc` or `~/.bashrc`:

```bash
# AI Commander aliases
alias aic='ai-commander'
alias aic-gui='open -a "AI Commander"'
alias aic-status='~/Projects/ai-commander/scripts/manage-services.sh status'
alias aic-logs='~/Projects/ai-commander/scripts/manage-services.sh logs'
alias aic-restart='~/Projects/ai-commander/scripts/manage-services.sh restart'
```

Reload shell:
```bash
source ~/.zshrc  # or source ~/.bashrc
```

### Preventing Auto-Start

If you don't want the bot to start on boot:

```bash
# Unload service
launchctl unload ~/Library/LaunchAgents/com.ai-commander.telegram.plist

# Remove from auto-start
rm ~/Library/LaunchAgents/com.ai-commander.telegram.plist

# Start manually when needed
ai-commander telegram start
```

## Updating

### Update Binaries

After pulling new code:

```bash
cd /Users/masa/Projects/ai-commander

# Rebuild
cargo build --release -p ai-commander
cd crates/commander-gui/ui && npm run build && cd ..
cargo tauri build

# Reinstall
./scripts/install-local.sh

# Restart services
./scripts/manage-services.sh restart
```

### Update Service Configuration

If you need to modify the service:

```bash
# Edit plist
nano ~/Library/LaunchAgents/com.ai-commander.telegram.plist

# Reload service
launchctl unload ~/Library/LaunchAgents/com.ai-commander.telegram.plist
launchctl load ~/Library/LaunchAgents/com.ai-commander.telegram.plist
```

## Uninstalling

To completely remove AI Commander:

```bash
# Stop services
./scripts/manage-services.sh stop

# Remove service configuration
rm ~/Library/LaunchAgents/com.ai-commander.telegram.plist

# Remove binaries
sudo rm /usr/local/bin/ai-commander
rm -rf ~/Applications/"AI Commander.app"

# Remove data and logs
rm -rf ~/.ai-commander

# Remove project (if desired)
rm -rf ~/Projects/ai-commander
```

## System Requirements

### Minimum Requirements
- macOS 10.13+
- 4GB RAM
- 100MB disk space

### Recommended Requirements
- macOS 12.0+
- 8GB RAM
- 500MB disk space (for logs and session data)

### Runtime Dependencies
- tmux (for session management)
- Terminal.app or iTerm2
- Internet connection (for Telegram bot)

## Security Considerations

### Telegram Bot Token

**Never commit the token to git:**
```bash
# Add to .gitignore
echo ".env" >> .gitignore
```

**Store securely:**
```bash
# Use environment variable
export TELEGRAM_BOT_TOKEN="your_token"

# Or use .env file
echo "TELEGRAM_BOT_TOKEN=your_token" > .env
```

### File Permissions

The installation script sets secure permissions:
- CLI: `755` (rwxr-xr-x)
- GUI app: `755` with quarantine removed
- Logs: `644` (rw-r--r--)

### Network Access

AI Commander requires:
- Outbound HTTPS (443) for Telegram API
- No inbound ports (unless using webhooks)

## Performance Tuning

### Reduce Memory Usage

If the bot uses too much memory:

```bash
# Set memory limit in plist
# Add under <dict>:
<key>SoftResourceLimits</key>
<dict>
    <key>MemoryLimit</key>
    <integer>512000000</integer>  <!-- 512MB -->
</dict>
```

### Reduce Log File Size

```bash
# Decrease log retention
# Run health check more frequently to rotate logs
# Or manually clear logs weekly
```

## Integration with Other Tools

### Shell Integration

Source completion (if available):
```bash
# Add to ~/.zshrc
eval "$(ai-commander completion zsh)"
```

### Alfred/Raycast Integration

Create custom commands to:
- Start/stop services
- Open GUI
- Check status
- View logs

## Support and Documentation

- **Project docs**: `/Users/masa/Projects/ai-commander/docs/`
- **Issue tracker**: GitHub Issues
- **Logs**: `~/.ai-commander/logs/`
- **Service config**: `~/Library/LaunchAgents/`

## Acceptance Criteria

- ✅ CLI runs from anywhere: `ai-commander --version`
- ✅ GUI opens from Applications folder
- ✅ Telegram bot auto-starts on boot
- ✅ Easy start/stop with management script
- ✅ Health monitoring detects failures
- ✅ Logs accessible and rotated
- ✅ No manual cargo commands needed

## Next Steps

After stable deployment:
1. Configure Telegram bot settings
2. Set up automated health checks (cron)
3. Customize shell aliases
4. Review logs regularly
5. Monitor disk usage

---

**Version**: 0.3.0
**Last Updated**: 2026-02-24
**Status**: Production-ready for local deployment
