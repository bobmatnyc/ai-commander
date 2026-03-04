# GUI Feature Parity Analysis

**Research Date**: 2026-02-24
**Project**: ai-commander
**Context**: User wants GUI to have feature parity with TUI and Telegram bot

## Executive Summary

The GUI is currently missing **15 high-priority features** and **8 medium-priority features** compared to TUI and Telegram bot. The most critical gaps are:
- **No filesystem operations** (ls, cat, find, mkdir, etc.)
- **No inspect mode** for live tmux viewing
- **No session aliases** or project management
- **No Telegram pairing** integration
- **No agent/adapter selection**
- **No command history**

## Detailed Feature Comparison Matrix

| Feature Category | TUI | Telegram | GUI | Priority | Difficulty |
|-----------------|-----|----------|-----|----------|-----------|
| **Session Management** |
| List sessions | ✅ | ✅ | ✅ | - | - |
| Create session | ✅ | ✅ | ✅ | - | - |
| Connect to session | ✅ | ✅ | ✅ | - | - |
| Disconnect from session | ✅ | ✅ | ✅ | - | - |
| Stop session (with git commit) | ✅ | ✅ | ✅ | - | - |
| Rename session | ✅ | ❌ | ❌ | **HIGH** | Medium |
| Session status/activity | ✅ | ✅ | Partial | **HIGH** | Medium |
| Session picker (F3) | ✅ | ❌ | ❌ | MEDIUM | Low |
| Aliases (project shortcuts) | ✅ | ✅ | ❌ | **HIGH** | Medium |
| Session activity indicators | ✅ | ❌ | ❌ | **HIGH** | High |
| **Communication** |
| Send messages | ✅ | ✅ | ✅ | - | - |
| Receive messages | ✅ | ✅ | ✅ | - | - |
| Slash commands | ✅ | ✅ | Partial | - | - |
| @ routing (multi-session) | ✅ | ❌ | ❌ | **HIGH** | Medium |
| Command history (Up/Down) | ✅ | ❌ | ❌ | **HIGH** | Low |
| Auto-scroll to bottom | ✅ | ❌ | ✅ | - | - |
| Clear messages | ✅ | ✅ | ✅ | - | - |
| **Filesystem Operations** |
| ls (list directory) | ✅ | ❌ | ❌ | **HIGH** | Medium |
| cat/read (file contents) | ✅ | ❌ | ❌ | **HIGH** | Medium |
| head/tail | ✅ | ❌ | ❌ | MEDIUM | Low |
| find (search files) | ✅ | ❌ | ❌ | **HIGH** | Medium |
| mkdir | ✅ | ❌ | ❌ | MEDIUM | Low |
| touch | ✅ | ❌ | ❌ | MEDIUM | Low |
| mv (move/rename) | ✅ | ❌ | ❌ | MEDIUM | Low |
| cp (copy) | ✅ | ❌ | ❌ | MEDIUM | Low |
| rm (delete) | ✅ | ❌ | ❌ | MEDIUM | Low |
| pwd (current directory) | ✅ | ❌ | ❌ | MEDIUM | Low |
| **Telegram Integration** |
| Generate pairing code | ✅ | Native | ❌ | **HIGH** | Medium |
| Pairing (/pair) | ✅ | ✅ | ❌ | **HIGH** | Medium |
| Deep links | ❌ | ✅ | ❌ | MEDIUM | High |
| Bot status | ✅ | Native | ✅ | - | - |
| **Advanced Features** |
| Inspect mode (live tmux) | ✅ (F2) | ❌ | ❌ | **HIGH** | High |
| Git worktree support | ✅ | ✅ | ❌ | MEDIUM | Medium |
| Agent orchestrator | ✅ | ❌ | ❌ | LOW | High |
| Adapter selection (CC/MPM) | ✅ | ✅ | ❌ | **HIGH** | Medium |
| Option selection | ✅ | ✅ | ❌ | **HIGH** | Medium |
| **UI/UX** |
| Keyboard shortcuts | ✅ | ❌ | Partial | MEDIUM | Low |
| Scroll output | ✅ | ❌ | ✅ | - | - |
| Session activity preview | ✅ | ✅ | ❌ | **HIGH** | Medium |
| Indicator icons | ✅ | ❌ | Partial | MEDIUM | Low |
| Error notifications | ✅ | ✅ | ✅ | - | - |

## Missing Features by Category

### HIGH PRIORITY (Core Functionality)

These features are essential for feature parity and user workflow:

#### 1. **@ Routing (Multi-Session Messaging)**
- **What it does**: Send messages to multiple sessions simultaneously
- **TUI**: `@alias1 @alias2 message` routes to multiple sessions
- **GUI**: Not implemented
- **User impact**: Cannot manage multiple projects efficiently
- **Implementation**:
  - Parse `@` syntax in InputArea
  - Route messages to multiple sessions via backend
  - Show status messages per target
  - Backend command: Already exists in `tui/commands.rs:369`

#### 2. **Filesystem Operations**
- **What it does**: Browse, read, and manipulate files without leaving UI
- **TUI**: Full suite (ls, cat, find, mkdir, touch, mv, cp, rm, pwd)
- **GUI**: Not implemented
- **User impact**: Must switch to terminal for file operations
- **Implementation**:
  - Add file browser component or command palette
  - Implement filesystem commands as slash commands
  - Show file contents in chat view
  - Backend: Commands exist in TUI, need GUI bindings

#### 3. **Command History**
- **What it does**: Navigate previous commands with Up/Down arrows
- **TUI**: Up/Down keys cycle through history
- **GUI**: Not implemented
- **User impact**: Must retype common commands
- **Implementation**:
  - Store input history in component state
  - Bind Up/Down keys to navigate history
  - Persist history across sessions
  - Difficulty: Low (pure frontend)

#### 4. **Session Aliases**
- **What it does**: Short names for projects (e.g., `@ai` instead of `commander-ai-commander`)
- **TUI**: `/alias [project] [alias]` to manage
- **GUI**: Not implemented
- **User impact**: Must use long session names
- **Implementation**:
  - Add alias management UI (modal or settings)
  - Backend commands exist: `tui/commands.rs:571-739`
  - Show aliases in session list
  - Difficulty: Medium

#### 5. **Inspect Mode (Live Tmux View)**
- **What it does**: F2 toggles live view of tmux session output
- **TUI**: ViewMode::Inspect shows raw tmux output
- **GUI**: Not implemented
- **User impact**: Cannot debug session behavior
- **Implementation**:
  - Add toggle button or keyboard shortcut
  - Stream tmux output continuously
  - Backend: `tui/inspect.rs`
  - Difficulty: High (requires continuous streaming)

#### 6. **Telegram Pairing**
- **What it does**: Generate pairing code to link Telegram bot
- **TUI**: `/telegram` command
- **GUI**: Not implemented
- **User impact**: Cannot use Telegram bot integration
- **Implementation**:
  - Add pairing button or command
  - Show QR code or pairing code
  - Backend: `tui/commands.rs:329-366`
  - Difficulty: Medium

#### 7. **Adapter Selection**
- **What it does**: Choose Claude Code vs MPM when creating session
- **TUI**: `-a <adapter>` flag on `/connect`
- **Telegram**: Adapter selection in connect command
- **GUI**: Not implemented (hardcoded or defaults to one adapter)
- **User impact**: Cannot specify which AI adapter to use
- **Implementation**:
  - Add adapter dropdown in CreateSessionModal
  - Pass adapter parameter to backend
  - Difficulty: Medium

#### 8. **Session Activity Indicators**
- **What it does**: Shows what session is currently doing (processing, waiting, etc.)
- **TUI**: [Claude], [Shell], [?] indicators + activity summary
- **GUI**: Activity icon only (no detail)
- **User impact**: Cannot tell if session is busy or idle
- **Implementation**:
  - Poll session status regularly
  - Extract activity from tmux output
  - Show indicator + tooltip with activity
  - Backend: `tui/commands.rs:448-493`
  - Difficulty: Medium

#### 9. **Rename Session**
- **What it does**: Rename tmux session without destroying it
- **TUI**: `/rename <new-name>`
- **GUI**: Not implemented
- **User impact**: Must destroy and recreate session to rename
- **Implementation**:
  - Add rename action in session list context menu
  - Backend command exists in TUI
  - Difficulty: Medium

#### 10. **Option Selection**
- **What it does**: Interactive option picker for Claude suggestions
- **TUI**: Recent feature (see docs/research/option-selection-analysis-2026-02-21.md)
- **Telegram**: Inline buttons for options
- **GUI**: Not implemented
- **User impact**: Must type option numbers manually
- **Implementation**:
  - Parse option messages from Claude
  - Show interactive buttons below message
  - Send selected option back to session
  - Difficulty: Medium-High

### MEDIUM PRIORITY (Nice to Have)

#### 11. **Session Picker (F3)**
- **What it does**: Full-screen session selector with keyboard navigation
- **TUI**: F3 key shows list
- **GUI**: Not needed (sidebar always visible)
- **User impact**: Minimal (GUI has persistent sidebar)
- **Implementation**: Skip (GUI has better UX already)

#### 12. **Git Worktree Support**
- **What it does**: Create session from git worktree
- **TUI**: `/connect-tree <name>`
- **Telegram**: `/ct <name>`
- **GUI**: Not implemented
- **User impact**: Advanced git users only
- **Implementation**:
  - Add worktree option in create modal
  - Backend command exists
  - Difficulty: Medium

#### 13. **Keyboard Shortcuts**
- **What it does**: F2 (inspect), F3 (sessions), Ctrl+L (clear), etc.
- **TUI**: Full keyboard navigation
- **GUI**: Limited (Enter to send, Shift+Enter for newline)
- **User impact**: Power users miss shortcuts
- **Implementation**:
  - Add keyboard event handlers
  - Show shortcut cheat sheet
  - Difficulty: Low

#### 14. **Detailed Status Command**
- **What it does**: Shows full session status (path, adapter, activity)
- **TUI**: `/status [project]` with AI interpretation
- **GUI**: Only sends `/status` to session
- **User impact**: Less information displayed
- **Implementation**:
  - Parse status response
  - Show structured status UI
  - Difficulty: Medium

#### 15. **Deep Links (Telegram)**
- **What it does**: `tg://resolve?domain=...` links to jump to sessions
- **Telegram**: Generated for quick access
- **GUI**: Not applicable (desktop app)
- **User impact**: None (Telegram-specific)
- **Implementation**: N/A

### LOW PRIORITY (Advanced/Rarely Used)

#### 16. **Agent Orchestrator**
- **What it does**: Multi-agent workflow orchestration
- **TUI**: Feature-gated, experimental
- **GUI**: Not implemented
- **User impact**: Minimal (experimental feature)
- **Implementation**: Defer until orchestrator is stable
- **Difficulty**: High

#### 17. **LLM-Interpreted Status**
- **What it does**: Uses LLM to explain what Claude is doing
- **TUI**: Falls back to LLM interpretation if no activity detected
- **GUI**: Not implemented
- **User impact**: Less context-aware status messages
- **Implementation**: Call LLM API for interpretation
- **Difficulty**: Medium

## Implementation Roadmap

### Phase 1: Essential Commands (Week 1)
**Goal**: Achieve command parity with TUI

1. **Command History** (1 day)
   - Store input history in component state
   - Bind Up/Down arrow keys
   - File: `InputArea.svelte`

2. **Session Aliases** (2 days)
   - Add alias management UI
   - Bind `/alias` and `/unalias` commands
   - Display aliases in session list
   - Files: `SessionList.svelte`, backend bindings

3. **@ Routing** (2 days)
   - Parse `@alias message` syntax
   - Route to multiple sessions
   - Show routing status
   - File: `InputArea.svelte`

4. **Adapter Selection** (1 day)
   - Add dropdown to CreateSessionModal
   - Pass adapter parameter to backend
   - File: `CreateSessionModal.svelte`

### Phase 2: Filesystem Operations (Week 2)
**Goal**: Enable file browsing without leaving GUI

5. **Basic Filesystem Commands** (3 days)
   - Implement `/ls`, `/cat`, `/pwd`
   - Show file contents in chat
   - Add file path autocompletion
   - Files: `InputArea.svelte`, new backend bindings

6. **File Manipulation** (2 days)
   - Implement `/mkdir`, `/touch`, `/mv`, `/cp`, `/rm`
   - Confirm destructive operations
   - Files: `InputArea.svelte`, backend bindings

### Phase 3: Advanced Features (Week 3)
**Goal**: Add power user features

7. **Inspect Mode** (3 days)
   - Add toggle button
   - Stream tmux output continuously
   - Add scrollback buffer
   - New file: `InspectView.svelte`

8. **Telegram Pairing** (1 day)
   - Add `/telegram` command
   - Show pairing code modal
   - File: `InputArea.svelte`, new modal

9. **Session Activity Indicators** (2 days)
   - Poll session status
   - Extract activity from tmux
   - Show in session list
   - File: `SessionList.svelte`

10. **Option Selection** (2 days)
    - Parse option messages
    - Render interactive buttons
    - Send selected option
    - File: `ChatView.svelte`

### Phase 4: Polish (Week 4)
**Goal**: Improve UX and add remaining features

11. **Rename Session** (1 day)
    - Add context menu to session list
    - Implement rename dialog
    - File: `SessionList.svelte`

12. **Keyboard Shortcuts** (1 day)
    - Add global keyboard handler
    - Show shortcut help modal
    - File: `App.svelte`

13. **Git Worktree Support** (1 day)
    - Add worktree option to create modal
    - File: `CreateSessionModal.svelte`

## Technical Implementation Details

### Backend Commands Available (Rust)

Located in `crates/ai-commander/src/tui/`:

```rust
// commands.rs
handle_command() {
  "/connect" => connect(),
  "/disconnect" => disconnect(),
  "/list" => list_sessions(),
  "/status" => show_status(),
  "/stop" => stop_session(),
  "/rename" => rename_session(),
  "/telegram" => generate_telegram_pairing(),
  "/alias" => handle_alias(),
  "/unalias" => handle_unalias(),
  "/inspect" => toggle_inspect_mode(),
  "/sessions" => show_sessions(),
}

// Filesystem (when connected)
ls, cat, head, tail, find, mkdir, touch, mv, cp, rm, pwd
```

### Telegram Bot Commands (Rust)

Located in `crates/commander-telegram/src/handlers.rs`:

```rust
pub enum Command {
  Start, Help, Pair,
  Connect, C,  // Aliases
  Disconnect,
  Stop, S,  // Aliases
  ConnectTree, Ct,  // Worktree
  Send,  // Direct message
  Status,
  List,
}
```

### GUI Implementation Status

Current GUI files:
- ✅ `App.svelte` - Main layout
- ✅ `SessionList.svelte` - Session management
- ✅ `ChatView.svelte` - Message display
- ✅ `InputArea.svelte` - Message input + slash commands
- ✅ `BotStatus.svelte` - Telegram bot status
- ✅ `CreateSessionModal.svelte` - New session creation

Slash commands implemented in `InputArea.svelte`:
- ✅ `/status` - Send status command
- ✅ `/list` - List sessions (local only)
- ✅ `/disconnect` - Disconnect from session
- ✅ `/stop` - Stop session
- ✅ `/clear` - Clear messages
- ✅ `/help` - Show help
- ✅ `/send` - Send literal text

Missing commands:
- ❌ `/connect` - Only via session list
- ❌ `/rename` - No rename functionality
- ❌ `/telegram` - No pairing
- ❌ `/alias`, `/unalias` - No alias management
- ❌ `/inspect` - No inspect mode
- ❌ `/sessions` - No session picker (N/A)
- ❌ `@routing` - No multi-session messaging
- ❌ Filesystem commands (ls, cat, find, etc.)

## Priority Ranking Rationale

**HIGH PRIORITY** features are those that:
1. Both TUI and Telegram have (consistency)
2. Frequently used in daily workflows
3. Block common user tasks
4. Required for feature parity claim

**MEDIUM PRIORITY** features are:
1. Available in TUI but niche use case
2. Power user features
3. Can be worked around

**LOW PRIORITY** features are:
1. Experimental or unstable
2. Platform-specific (Telegram deep links)
3. Rarely used

## User Workflows Blocked

### 1. Multi-Project Developer
**Current pain**: Must switch between GUI and TUI to manage multiple projects
**Missing features**:
- @ routing (send commands to multiple sessions)
- Session aliases (quick project switching)
- Activity indicators (see which projects need attention)

### 2. File Operations
**Current pain**: Must use terminal for file operations
**Missing features**:
- ls, cat, find (browse and read files)
- mkdir, touch, mv, cp, rm (manipulate files)
- pwd (current directory context)

### 3. Session Management
**Current pain**: Limited session control
**Missing features**:
- Rename session (change project name)
- Inspect mode (debug session issues)
- Activity preview (know if session is busy)

### 4. Command Efficiency
**Current pain**: Repetitive typing
**Missing features**:
- Command history (Up/Down)
- Adapter selection (choose CC vs MPM)

### 5. Telegram Integration
**Current pain**: Cannot use Telegram bot
**Missing features**:
- Pairing code generation
- Unified session management

## Recommendations

### Immediate (Week 1)
1. **Command History** - Quick win, high user impact
2. **@ Routing** - Essential for multi-project workflows
3. **Adapter Selection** - Required for flexibility

### Short-Term (Weeks 2-3)
4. **Filesystem Operations** - High-value, moderate effort
5. **Session Aliases** - Improves UX significantly
6. **Inspect Mode** - Critical for debugging

### Medium-Term (Week 4+)
7. **Telegram Pairing** - Completes integration
8. **Activity Indicators** - Polish feature
9. **Option Selection** - New feature parity

### Defer
- Agent Orchestrator (experimental)
- Deep Links (Telegram-only)
- Git Worktree (advanced users)

## Success Metrics

After implementation, measure:
- **Feature Coverage**: % of TUI features available in GUI
- **User Adoption**: % of TUI users switching to GUI
- **Workflow Efficiency**: Time saved per common task
- **Bug Reports**: Issues with missing features

**Target**: 95%+ feature parity with TUI by end of Phase 3

## Related Documentation

- [Option Selection Analysis](option-selection-analysis-2026-02-21.md)
- [TUI Restart State Preservation](tui-restart-state-preservation-2026-02-21.md)
- `crates/ai-commander/src/tui/commands.rs` - TUI command implementation
- `crates/commander-telegram/src/handlers.rs` - Telegram bot commands
- `crates/commander-gui/ui/src/lib/components/` - GUI component code

---

**Research Complete**: 2026-02-24
**Next Action**: Prioritize HIGH features from Phase 1 for immediate implementation
