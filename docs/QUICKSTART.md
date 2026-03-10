# AI Commander - Local Deployment Quickstart

**5-minute setup for stable local deployment on macOS**

## Build and Install

```bash
cd /Users/masa/Projects/ai-commander

# 1. Build binaries (~50 seconds)
cargo build --release -p ai-commander
cd crates/commander-gui/ui && npm run build && cd .. && cargo tauri build && cd ../..

# 2. Install (~5 seconds, requires sudo password)
./scripts/install-local.sh

# 3. Setup Telegram bot service (~5 seconds)
./scripts/setup-telegram-service.sh
```

## Verify Installation

```bash
# Test CLI
ai-commander --version

# Test GUI
open -a "AI Commander"

# Check services
./scripts/manage-services.sh status
```

## Daily Commands

```bash
# Start everything
./scripts/manage-services.sh start

# Check status
./scripts/manage-services.sh status

# View logs
./scripts/manage-services.sh logs

# Stop everything
./scripts/manage-services.sh stop

# Restart if needed
./scripts/manage-services.sh restart
```

## What You Get

- **CLI**: Available globally as `ai-commander`
- **GUI**: Opens from Applications folder or Spotlight
- **Telegram Bot**: Auto-starts on boot, restarts on crash
- **Logs**: `~/.ai-commander/logs/telegram.log`
- **Health Checks**: Optional automated monitoring

## Troubleshooting

### Bot not starting?
```bash
./scripts/manage-services.sh logs-error
# Check for missing TELEGRAM_BOT_TOKEN
```

### GUI won't open?
```bash
xattr -cr ~/Applications/"AI Commander.app"
open -a "AI Commander"
```

### Need to rebuild?
```bash
cargo build --release -p ai-commander
./scripts/install-local.sh
./scripts/manage-services.sh restart
```

## File Locations

- **CLI binary**: `/usr/local/bin/ai-commander`
- **GUI app**: `~/Applications/AI Commander.app`
- **Service config**: `~/Library/LaunchAgents/com.ai-commander.telegram.plist`
- **Logs**: `~/.ai-commander/logs/`
- **Scripts**: `~/Projects/ai-commander/scripts/`

## Optional: Shell Aliases

Add to `~/.zshrc`:

```bash
alias aic='ai-commander'
alias aic-status='~/Projects/ai-commander/scripts/manage-services.sh status'
alias aic-logs='~/Projects/ai-commander/scripts/manage-services.sh logs'
```

## Full Documentation

See `docs/LOCAL_DEPLOYMENT.md` for complete guide.

---

**That's it! You now have a stable, production-like AI Commander setup.**

Test with: `ai-commander --version && ./scripts/manage-services.sh status`
