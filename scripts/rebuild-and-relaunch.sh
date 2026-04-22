#!/usr/bin/env bash
# Rebuild and relaunch the AIC Tauri desktop app.
#
# Usage:
#   scripts/rebuild-and-relaunch.sh          # normal (background-friendly)
#   scripts/rebuild-and-relaunch.sh --sync   # wait for build to finish (make rebuild)
#
# The script always detects the project root via git so it works regardless
# of the current working directory when invoked.

SYNC=false
[[ "$1" == "--sync" ]] && SYNC=true

# Resolve project root from git (works even when cwd is inside a sub-directory)
PROJECT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null)"
if [[ -z "$PROJECT" ]]; then
  echo "ERROR: could not determine project root via git" >&2
  exit 1
fi

LOG="/tmp/aic-build.log"
APP_BUNDLE="$PROJECT/target/release/bundle/macos/AIC - AI Commander.app"

notify() {
  osascript -e "display notification \"$1\" with title \"AIC Build\"" 2>/dev/null || true
}

do_build() {
  echo "[$(date '+%H:%M:%S')] === AIC rebuild started ===" > "$LOG"

  # ---- Kill existing instance ----
  echo "[$(date '+%H:%M:%S')] Killing running instance..." >> "$LOG"
  pkill -9 -f "commander-gui" 2>/dev/null || true
  sleep 1

  # ---- Build Svelte frontend (Tauri target) ----
  echo "[$(date '+%H:%M:%S')] Building Svelte frontend (Tauri)..." >> "$LOG"
  if ! (cd "$PROJECT/crates/commander-gui/ui" && npm run build >> "$LOG" 2>&1); then
    echo "[$(date '+%H:%M:%S')] FAILED: npm run build" >> "$LOG"
    notify "AIC build FAILED (frontend) — check /tmp/aic-build.log"
    exit 1
  fi

  # ---- Build Svelte frontend (Web target) ----
  echo "[$(date '+%H:%M:%S')] Building Svelte frontend (Web)..." >> "$LOG"
  if ! (cd "$PROJECT/crates/commander-gui/ui" && npm run build:web >> "$LOG" 2>&1); then
    echo "[$(date '+%H:%M:%S')] FAILED: npm run build:web" >> "$LOG"
    notify "AIC build FAILED (web frontend) — check /tmp/aic-build.log"
    exit 1
  fi

  # ---- Deploy web-dist ----
  echo "[$(date '+%H:%M:%S')] Deploying web-dist..." >> "$LOG"
  rm -rf "$PROJECT/web-dist"/*
  cp -r "$PROJECT/crates/commander-gui/ui/dist-web/"* "$PROJECT/web-dist/"
  cp "$PROJECT/web-dist/web.html" "$PROJECT/web-dist/index.html"
  cp "$PROJECT/crates/commander-gui/ui/public/ai-commander.png" "$PROJECT/web-dist/" 2>/dev/null || true

  # ---- Build Tauri release bundle ----
  echo "[$(date '+%H:%M:%S')] Building Tauri bundle (cargo tauri build)..." >> "$LOG"
  if ! (cd "$PROJECT/crates/commander-gui" && cargo tauri build --bundles app >> "$LOG" 2>&1); then
    echo "[$(date '+%H:%M:%S')] FAILED: cargo tauri build" >> "$LOG"
    notify "AIC build FAILED (Tauri) — check /tmp/aic-build.log"
    exit 1
  fi

  # ---- Copy web-dist into the app bundle ----
  echo "[$(date '+%H:%M:%S')] Copying web-dist into app bundle..." >> "$LOG"
  rm -rf "$APP_BUNDLE/Contents/Resources/web-dist"
  cp -r "$PROJECT/web-dist" "$APP_BUNDLE/Contents/Resources/web-dist"

  # ---- Launch ----
  echo "[$(date '+%H:%M:%S')] Launching $APP_BUNDLE..." >> "$LOG"
  open "$APP_BUNDLE"

  echo "[$(date '+%H:%M:%S')] === AIC rebuild complete ===" >> "$LOG"
  notify "AIC build succeeded — app relaunched"
}

if $SYNC; then
  do_build
else
  # Run in background, disowned so the parent (git commit) can exit immediately
  do_build &
  disown
fi
