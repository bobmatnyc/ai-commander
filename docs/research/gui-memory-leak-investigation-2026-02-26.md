# GUI Memory Leak Investigation

**Date**: 2026-02-26
**Project**: AI Commander GUI (Tauri + Svelte)
**Type**: Actionable - Bug Investigation
**Status**: Complete

---

## Executive Summary

Investigation of the AI Commander GUI application identified **5 memory leak sources** across the Rust backend and Svelte frontend. The most severe issue is an **unbounded message history** in the Svelte store that grows without limit as session output is polled every 500ms. Combined with session polling (every 2 seconds) and bot status polling (every 5/10 seconds), the application accumulates significant memory over time.

---

## LEAK 1: Unbounded Session Message History (CRITICAL - Severity: HIGH)

**File**: `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/stores/app.ts`
**Lines**: 36-41

```typescript
export function addMessageToSession(sessionName: string, message: Message) {
  sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];
    map.set(sessionName, [...msgs, message]);  // LEAK: grows without limit
    return new Map(map);
  });
}
```

**Root Cause**: Every time the backend emits a `session-output` event (every 500ms when output changes), a new message is appended to the array. The `sessionMessages` Map is never pruned, bounded, or garbage-collected. Over hours of use:

- The `session-output` listener in `ChatView.svelte` (line 105-118) receives FULL tmux capture output (50 lines) each time
- Each capture is stored as a complete message object with a `Date` timestamp
- Old sessions remain in the Map even after disconnection
- The `clearSessionMessages` function exists (line 45-50) but is only called manually via `/clear` command

**Impact**: With 500ms polling, a busy session could add ~7,200 messages per hour. Each message containing up to 50 lines of terminal output means potentially megabytes of string data accumulated per hour.

**Recommended Fix**:
```typescript
const MAX_MESSAGES_PER_SESSION = 500;

export function addMessageToSession(sessionName: string, message: Message) {
  sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];
    const updated = [...msgs, message];
    // Keep only the most recent messages
    map.set(sessionName, updated.length > MAX_MESSAGES_PER_SESSION
      ? updated.slice(-MAX_MESSAGES_PER_SESSION)
      : updated);
    return new Map(map);
  });
}
```

Additionally, clear messages for sessions that are destroyed:
```typescript
// In stop_session handler or when session disappears from list
clearSessionMessages(sessionName);
```

---

## LEAK 2: Stale Session Entries in sessionMessages Map (MEDIUM)

**File**: `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/stores/app.ts`
**Lines**: 21

```typescript
export const sessionMessages = writable<Map<string, Message[]>>(new Map());
```

**Root Cause**: When a session is stopped or destroyed (via `stop_session` command in `commands.rs` line 63-81), the `sessionMessages` Map entry for that session is never removed. Messages for dead sessions persist in memory indefinitely.

**Evidence**: In `ChatView.svelte` line 68, after stopping a session, `currentSession.set(null)` is called, but `clearSessionMessages(sessionName)` is never called. Same issue in `InputArea.svelte` line 65.

**Impact**: Each stopped session retains its full message history in memory. If users create and destroy sessions frequently, memory accumulates.

**Recommended Fix**: Call `clearSessionMessages(sessionName)` when:
1. A session is stopped (`/stop` command or Stop button)
2. A session disappears from the session list (detected during polling)

---

## LEAK 3: Unbounded Hash Map in Backend Session Polling (LOW-MEDIUM)

**File**: `/Users/masa/Projects/ai-commander/crates/commander-gui/src/events.rs`
**Lines**: 18, 31-36

```rust
let last_hashes: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
// ...
hashes.insert(session_name.clone(), current_hash);
```

**Root Cause**: The `last_hashes` HashMap stores the last output hash for each session name ever connected to. When sessions are destroyed, their entries remain in this map. Since this runs in an infinite loop (`loop` at line 20), the HashMap only grows.

**Impact**: Each entry is small (String key + u64 value), so this is a slow leak. Over weeks of continuous use with many sessions created/destroyed, it could accumulate thousands of dead entries. Low severity individually but represents poor resource management.

**Recommended Fix**:
```rust
// Periodically clean up hashes for sessions that no longer exist
if let Some(tmux) = &state.tmux {
    if let Ok(active_sessions) = tmux.list_sessions() {
        let active_names: HashSet<String> = active_sessions.iter()
            .map(|s| s.name.clone()).collect();
        hashes.retain(|name, _| active_names.contains(name));
    }
}
```

---

## LEAK 4: Session List Polling Creates New Objects Every 2 Seconds (LOW)

**File**: `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/SessionList.svelte`
**Lines**: 77-80

```typescript
onMount(() => {
  loadSessions();
  interval = window.setInterval(loadSessions, 2000);
});
```

Combined with `commands.rs` line 20-37:
```rust
pub async fn list_sessions(state: State<'_, GuiState>) -> Result<Vec<SessionInfo>, String> {
    // Creates new Vec<SessionInfo> every call
    Ok(sessions.into_iter().map(|s| SessionInfo { ... }).collect())
}
```

**Root Cause**: Every 2 seconds, `loadSessions()` invokes the Tauri command which creates a new `Vec<SessionInfo>`, serializes it to JSON, sends it across IPC, deserializes on the frontend, and then `sessions.set(result)` replaces the store value. While the old value should be garbage collected, the high frequency (2s) combined with Svelte's reactivity system means:
- New arrays are allocated frequently
- Svelte subscriber callbacks fire every 2 seconds
- Each `sessions.set()` triggers re-evaluation of all reactive statements using `$sessions`

**Impact**: This is primarily a GC pressure issue rather than a true leak. However, it contributes to elevated memory usage because the GC may not keep up with allocation rate during busy periods.

**Recommended Fix**: Only update the store when the data actually changes:
```typescript
async function loadSessions() {
  try {
    const result = await invoke('list_sessions');
    const newSessions = result as Session[];
    // Only update if sessions actually changed
    if (JSON.stringify(newSessions) !== JSON.stringify($sessions)) {
      sessions.set(newSessions);
    }
  } catch (err) {
    console.error('Failed to load sessions:', err);
  }
}
```

Or increase the polling interval to 5 seconds since session changes are infrequent.

---

## LEAK 5: Multiple Simultaneous Polling Timers Without Coordination (LOW)

**Files**: Multiple components set up independent polling intervals:

| Component | File | Interval | What it polls |
|-----------|------|----------|---------------|
| SessionList.svelte | Line 79 | 2,000ms | `list_sessions` (Tauri IPC) |
| BotStatus.svelte | Line 125 | 5,000ms | `get_bot_status` (Tauri IPC) |
| BotStatus.svelte | Line 126 | 10,000ms | `check_telegram_connection` (Tauri IPC) |
| events.rs | Line 52 | 500ms | tmux output capture (backend) |

**Root Cause**: While each timer is individually cleaned up on component destroy (via `onMount` return / `onDestroy`), they all run simultaneously and independently. This creates overlapping IPC calls that:
- Keep the JavaScript event loop busy
- Prevent efficient garbage collection (GC cannot run during busy loops)
- Create serialization/deserialization overhead every cycle
- The `App.svelte` root component never unmounts, so these timers run for the entire application lifetime

**Impact**: The combined effect of 4 independent polling loops (with intervals from 500ms to 10s) means the application is constantly allocating and deallocating IPC payloads, contributing to memory fragmentation and GC pressure.

**Recommended Fix**: Consider a unified polling mechanism or event-driven approach:
- Replace session list polling with Tauri events emitted when sessions change
- Replace bot status polling with Tauri events emitted on status change
- Use a single polling loop on the backend that emits different events as needed

---

## Additional Observations (Not Leaks but Worth Noting)

### Double-spawned task in events.rs

**File**: `/Users/masa/Projects/ai-commander/crates/commander-gui/src/events.rs`
**Lines**: 17-54

```rust
pub async fn start_session_polling(app: AppHandle, state: GuiState) {
    tokio::spawn(async move {  // Inner spawn is redundant
        // ...
    });
}
```

This function is already called inside a `tauri::async_runtime::spawn` in `main.rs` line 18. The inner `tokio::spawn` creates a second detached task. This is not a leak per se, but means the polling task is orphaned from the original spawn and cannot be cancelled or tracked.

### Map recreation on every update

**File**: `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/stores/app.ts`
**Lines**: 37-41

```typescript
sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];
    map.set(sessionName, [...msgs, message]);
    return new Map(map);  // Creates a FULL COPY of the Map every update
});
```

Every message addition creates a new Map and a new array (spread operator). With messages arriving every 500ms, this is significant GC churn. A mutable update pattern would be more efficient:

```typescript
sessionMessages.update(map => {
    const msgs = map.get(sessionName) || [];
    msgs.push(message);
    if (msgs.length > MAX_MESSAGES) msgs.splice(0, msgs.length - MAX_MESSAGES);
    map.set(sessionName, msgs);
    return map;  // Same map reference - use {invalidate} pattern or trigger manually
});
```

Note: Svelte stores require a new reference to trigger reactivity, so the `new Map(map)` is intentional but expensive.

---

## Severity Ranking

| # | Leak | Severity | Memory Growth Rate | Fix Difficulty |
|---|------|----------|-------------------|----------------|
| 1 | Unbounded message history | **HIGH** | ~MB/hour with active session | Easy |
| 2 | Stale session entries | **MEDIUM** | Proportional to session count | Easy |
| 3 | Unbounded hash map (Rust) | **LOW-MEDIUM** | Bytes/session, slow growth | Easy |
| 4 | Session list polling churn | **LOW** | GC pressure, not true leak | Easy |
| 5 | Multiple polling timers | **LOW** | GC pressure, fragmentation | Medium |

---

## Recommended Fix Priority

### Immediate (fixes the primary leak):
1. Add `MAX_MESSAGES_PER_SESSION` cap to `addMessageToSession()` in `app.ts`
2. Call `clearSessionMessages()` when sessions are stopped

### Short-term (reduces memory pressure):
3. Clean up `last_hashes` HashMap entries for dead sessions in `events.rs`
4. Only update session store when data changes in `SessionList.svelte`

### Medium-term (architectural improvement):
5. Replace polling with event-driven approach using Tauri events
6. Remove double-spawn in `events.rs`
7. Optimize Map/array creation pattern in store updates

---

## Files Analyzed

| File | Path |
|------|------|
| Main entry | `/Users/masa/Projects/ai-commander/crates/commander-gui/src/main.rs` |
| Events/polling | `/Users/masa/Projects/ai-commander/crates/commander-gui/src/events.rs` |
| State definition | `/Users/masa/Projects/ai-commander/crates/commander-gui/src/state.rs` |
| Commands | `/Users/masa/Projects/ai-commander/crates/commander-gui/src/commands.rs` |
| Store | `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/stores/app.ts` |
| ChatView | `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/ChatView.svelte` |
| BotStatus | `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/BotStatus.svelte` |
| SessionList | `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/SessionList.svelte` |
| InputArea | `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/InputArea.svelte` |
| CreateSessionModal | `/Users/masa/Projects/ai-commander/crates/commander-gui/ui/src/lib/components/CreateSessionModal.svelte` |
