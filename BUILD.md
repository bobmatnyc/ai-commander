# AI Commander Build & Deploy Guide

## Quick Reference

### Tauri Desktop App (AIC)

```bash
cd /Users/masa/Projects/ai-commander

# 1. Build Svelte frontend (MUST run from ui/ directory)
cd crates/commander-gui/ui && rm -rf dist && npm run build

# 2. Build Tauri app (MUST run from commander-gui/ directory)
cd /Users/masa/Projects/ai-commander/crates/commander-gui && cargo tauri build --bundles app

# 3. Launch
open "target/release/bundle/macos/AIC - AI Commander.app"
```

### Web UI

```bash
cd /Users/masa/Projects/ai-commander

# 1. Build web frontend
cd crates/commander-gui/ui && npm run build:web

# 2. Deploy to web-dist (served by API on port 9876)
rm -rf /Users/masa/Projects/ai-commander/web-dist/*
cp -r dist-web/* /Users/masa/Projects/ai-commander/web-dist/
cp dist-web/web.html /Users/masa/Projects/ai-commander/web-dist/index.html
cp public/ai-commander.png /Users/masa/Projects/ai-commander/web-dist/

# 3. Update the Tauri app bundle (MUST rm -rf first to avoid stale files)
rm -rf "/Users/masa/Projects/ai-commander/target/release/bundle/macos/AIC - AI Commander.app/Contents/Resources/web-dist"
cp -r /Users/masa/Projects/ai-commander/web-dist "/Users/masa/Projects/ai-commander/target/release/bundle/macos/AIC - AI Commander.app/Contents/Resources/web-dist"

# 4. Restart the app to serve new files
pkill -9 -f "commander-gui" 2>/dev/null; sleep 2
open "/Users/masa/Projects/ai-commander/target/release/bundle/macos/AIC - AI Commander.app"
```

### One-liner: Full rebuild (desktop + web + deploy)

```bash
cd /Users/masa/Projects/ai-commander && \
  pkill -9 -f "commander-gui" 2>/dev/null; sleep 1 && \
  (cd crates/commander-gui/ui && rm -rf dist dist-web && npm run build && npm run build:web) && \
  (cd crates/commander-gui && cargo tauri build --bundles app) && \
  rm -rf web-dist/* && \
  cp -r crates/commander-gui/ui/dist-web/* web-dist/ && \
  cp web-dist/web.html web-dist/index.html && \
  cp crates/commander-gui/ui/public/ai-commander.png web-dist/ && \
  rm -rf "target/release/bundle/macos/AIC - AI Commander.app/Contents/Resources/web-dist" && \
  cp -r web-dist "target/release/bundle/macos/AIC - AI Commander.app/Contents/Resources/web-dist" && \
  open "target/release/bundle/macos/AIC - AI Commander.app"
```

### Workspace checks

```bash
cargo check --workspace          # Check all crates compile
cargo test --workspace           # Run all tests
cargo check -p commander-gui     # Check just the GUI crate
cargo check -p commander-api     # Check just the API
cargo test -p commander-core     # Test just core
```

## Common Mistakes

| Mistake | Symptom | Fix |
|---------|---------|-----|
| `npm run build` from wrong dir | Stale frontend, old UI | Must run from `crates/commander-gui/ui/` |
| `cargo tauri build` from workspace root | "not a Tauri project" | Must run from `crates/commander-gui/` |
| Not rebuilding frontend before Tauri | Old JS bundled in app | Always `npm run build` first |
| `cp -r` web-dist without `rm -rf` first | Stale JS files served | Always `rm -rf` bundle web-dist first |
| Forgetting `web.html` → `index.html` | 404 on root URL | Copy `web.html` to `index.html` |
| Old process still running on port | Old code serves requests | `pkill -9 -f "commander-gui"` |

## Architecture

```
crates/commander-gui/ui/
  ├── src/App.svelte          # Tauri desktop entry
  ├── src/WebApp.svelte       # Web browser entry
  ├── src/lib/transport.ts    # Tauri↔REST abstraction
  ├── src/lib/tauri-shim.ts   # Web: replaces @tauri-apps/api/core
  ├── src/lib/tauri-event-shim.ts  # Web: replaces @tauri-apps/api/event
  ├── vite.config.ts          # Tauri build → dist/
  ├── vite.web.config.ts      # Web build → dist-web/
  ├── dist/                   # Tauri frontend (embedded in .app)
  └── dist-web/               # Web frontend (served by API)

web-dist/                     # Deployed web assets (served on port 9876)

target/release/bundle/macos/AIC - AI Commander.app/
  └── Contents/Resources/web-dist/  # Bundle copy (must match web-dist/)
```

## Ports

| Port | Service | Configured by |
|------|---------|---------------|
| 9876 | commander-api + web UI | `ApiConfig::default()` or `AIC_BIND_ADDRESS` |
| 11434 | Ollama (local LLM) | Ollama app |

## Key Defaults

- **Ollama model**: `qwen2.5-coder:7b-instruct`
- **Claude Code flag**: `--dangerously-skip-permissions`
- **Tmux sessions**: project name directly (no prefix)
- **Web UI auth**: none (Tailscale handles security)
