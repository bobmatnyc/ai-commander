# AI Commander Local Deployment - Verification Checklist

Use this checklist to verify your local deployment is working correctly.

## Pre-Deployment Verification

- [ ] Rust toolchain installed: `rustc --version`
- [ ] Node.js installed: `node --version`
- [ ] npm installed: `npm --version`
- [ ] tmux installed: `tmux -V`
- [ ] Git repository up to date: `git status`

## Build Verification

- [ ] CLI binary built: `ls -lh target/release/ai-commander`
- [ ] GUI frontend built: `ls -lh crates/commander-gui/ui/dist/`
- [ ] GUI binary built: `ls -lh target/release/commander-gui`
- [ ] No build errors in output

## Installation Verification

- [ ] CLI installed: `/Users/masa/.local/bin/ai-commander --version`
- [ ] GUI app exists: `ls -ld ~/Applications/"AI Commander.app"`
- [ ] GUI bundle structure correct: `ls ~/Applications/"AI Commander.app"/Contents/MacOS/`
- [ ] Scripts executable: `ls -l scripts/*.sh`

## Functionality Tests

### CLI Tests
- [ ] CLI runs: `/Users/masa/.local/bin/ai-commander --version`
- [ ] Help works: `/Users/masa/.local/bin/ai-commander --help`
- [ ] Subcommands listed: `/Users/masa/.local/bin/ai-commander help`

### GUI Tests
- [ ] GUI opens: `open -a "AI Commander"`
- [ ] No crash on startup
- [ ] Window appears and is responsive
- [ ] Can close cleanly

### Service Management Tests
- [ ] Status command works: `./scripts/manage-services.sh status`
- [ ] Shows correct state (not running initially)
- [ ] No script errors

## Telegram Bot Setup (Optional)

If using Telegram bot:

- [ ] Bot token available: `echo $TELEGRAM_BOT_TOKEN`
- [ ] Service setup script runs: `./scripts/setup-telegram-service.sh`
- [ ] Plist created: `ls ~/Library/LaunchAgents/com.ai-commander.telegram.plist`
- [ ] Bot starts: `launchctl list | grep ai-commander`
- [ ] Logs created: `ls ~/.ai-commander/logs/telegram.log`

## Management Script Tests

- [ ] Start works: `./scripts/manage-services.sh start`
- [ ] Status shows running: `./scripts/manage-services.sh status`
- [ ] Logs accessible: `./scripts/manage-services.sh logs` (Ctrl+C to exit)
- [ ] Stop works: `./scripts/manage-services.sh stop`
- [ ] Status shows stopped: `./scripts/manage-services.sh status`

## Health Check Tests

- [ ] Health check runs: `./scripts/health-check.sh`
- [ ] No critical errors
- [ ] Health log created: `ls ~/.ai-commander/logs/health.log`

## Post-Deployment Verification

- [ ] Can run CLI from project directory: `./target/release/ai-commander --version`
- [ ] Can run CLI from user bin: `/Users/masa/.local/bin/ai-commander --version`
- [ ] GUI in Applications: Spotlight search "AI Commander"
- [ ] No quarantine warnings when opening GUI

## Integration Tests

### PATH Configuration (Optional)
- [ ] Added to .zshrc: `grep ".local/bin" ~/.zshrc`
- [ ] PATH reloaded: `source ~/.zshrc`
- [ ] CLI accessible globally: `ai-commander --version`

### Boot Auto-Start Test (If Telegram Setup)
- [ ] Service will auto-start: `launchctl print gui/$(id -u)/com.ai-commander.telegram | grep KeepAlive`
- [ ] Restart to verify (optional): reboot and check `launchctl list | grep ai-commander`

## Documentation Verification

- [ ] Deployment guide exists: `ls docs/LOCAL_DEPLOYMENT.md`
- [ ] Quick start exists: `ls DEPLOYMENT_QUICKSTART.md`
- [ ] Completion doc exists: `ls DEPLOYMENT_COMPLETE.md`
- [ ] This checklist exists: `ls VERIFICATION_CHECKLIST.md`

## Cleanup Tests

### Test Reinstall
- [ ] Can reinstall: `./scripts/install-user.sh`
- [ ] No errors during reinstall
- [ ] Still works after reinstall

### Test Rebuild
- [ ] Can rebuild: `cargo build --release -p ai-commander`
- [ ] No compilation errors
- [ ] Can reinstall after rebuild

## Success Criteria

All core features working:
- [ ] ✅ CLI binary is functional
- [ ] ✅ GUI app opens and runs
- [ ] ✅ Service management works
- [ ] ✅ Scripts are executable
- [ ] ✅ Documentation is complete

Optional features (if configured):
- [ ] ✅ Telegram bot starts
- [ ] ✅ Auto-start configured
- [ ] ✅ Health checks working

## Known Issues / Notes

Use this space to note any issues encountered:

```
[Space for notes]
```

## Final Verification Command

Run this to verify everything:

```bash
# Quick verification
/Users/masa/.local/bin/ai-commander --version && \
ls ~/Applications/"AI Commander.app" && \
./scripts/manage-services.sh status && \
echo "✅ All verified!"
```

---

**Date Completed**: ________________
**Verified By**: ________________
**Status**: [ ] Passed  [ ] Failed  [ ] Partial

**Notes**: 
