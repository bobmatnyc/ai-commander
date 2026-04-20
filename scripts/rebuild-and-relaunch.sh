#!/usr/bin/env bash
# Rebuild and relaunch AIC Tauri app
# Called automatically after each git commit via Claude Code hook

set -e
PROJECT="/Users/masa/Projects/ai-commander"
LOG="/tmp/aic-rebuild.log"

echo "[$(date)] Starting AIC rebuild..." >> "$LOG"

# Kill existing instance
pkill -9 -f "commander-gui" 2>/dev/null || true
sleep 1

# Build Svelte frontend
echo "[$(date)] Building frontend..." >> "$LOG"
cd "$PROJECT/crates/commander-gui/ui"
npm run build >> "$LOG" 2>&1

# Build Tauri release bundle
echo "[$(date)] Building Tauri bundle..." >> "$LOG"
cd "$PROJECT/crates/commander-gui"
cargo tauri build --bundles app >> "$LOG" 2>&1

# Launch
echo "[$(date)] Launching..." >> "$LOG"
open "$PROJECT/target/release/bundle/macos/AIC - AI Commander.app"

echo "[$(date)] AIC rebuild complete." >> "$LOG"
