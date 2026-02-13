# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2025-02-12

### Added
- TUI clickable session links with mouse support - click session names to connect
- Telegram inline keyboard buttons for improved mobile interaction
- Telegram Forum Topics support with /groupmode, /topic, and /topics commands
- Comprehensive autocomplete enhancements for TUI and REPL
- Incremental AI summaries every 50 lines of output
- Real-time progress messages during output collection
- LLM interpretation added to /status command
- Consolidated /session into /connect command

### Changed
- Improved session management workflow with streamlined commands

## [0.1.0] - 2026-01-29

### Added
- Phase 1: commander-models - Core data types with newtype IDs (ProjectId, TaskId, MessageId) and builders
- Phase 2: commander-persistence - Atomic JSON file storage with transactional writes
- Phase 3: commander-adapters - Runtime adapter trait and implementations (ClaudeCode, Aider, Cursor)
- Phase 4: commander-cli - CLI with clap and interactive REPL with rustyline
- Phase 5: commander-events - Event pub/sub system; commander-work - Priority work queue with dependency tracking
- Phase 6: commander-tmux - Tmux session orchestration for multi-pane workflows
- Phase 7: commander-runtime - Async runtime with tokio for process management
- Phase 8: commander-api - REST API with axum for external integrations

### Stats
- 9 crates in workspace
- 293 tests passing (8 ignored)
- Rust 2021 edition

[Unreleased]: https://github.com/owner/ai-commander/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/owner/ai-commander/compare/v0.1.0...v0.3.0
[0.1.0]: https://github.com/owner/ai-commander/releases/tag/v0.1.0
