# Rebuild Detection Indicator Research

**Date:** 2026-04-13
**Feature:** Web UI and Telegram should detect when the API server is rebuilding and show a "Rebuilding..." indicator.

---

## 1. Web UI Health Polling (WebApp.svelte)

**File:** `crates/commander-gui/ui/src/WebApp.svelte`, lines 19-44

**Current behavior:**
- `checkVersion()` polls `/api/health` every 60 seconds (line 39)
- On success, compares `data.version` to `loadedVersion` to detect new version deployments
- On fetch failure (server unreachable), the `catch` block silently ignores the error (line 32-34: `catch { // Silently ignore -- server may be restarting }`)
- There is NO tracking of server-down state -- errors are swallowed completely

**Health endpoint returns:** `{ status: "ok", version: "0.3.9", uptime_seconds: N }` (see `crates/commander-api/src/handlers/health.rs`, line 9-15)

**Recommended changes to `checkVersion()` (lines 23-35):**
```
- Track consecutive failures: add `let failCount = 0;` and `let serverRebuilding = false;`
- On success: reset failCount to 0, set serverRebuilding = false
- On failure: increment failCount; if failCount >= 2, set serverRebuilding = true
- When serverRebuilding is true, poll every 5s instead of 60s (dynamic interval)
- When serverRebuilding transitions false -> true, switch interval
- When serverRebuilding transitions true -> false, switch back to 60s
```

**Implementation approach -- replace setInterval with dynamic polling:**
```typescript
// Replace fixed interval with recursive setTimeout for dynamic timing
let pollTimeoutId: ReturnType<typeof setTimeout>;
let serverRebuilding = false;
let failCount = 0;

function schedulePoll() {
  const delay = serverRebuilding ? 5000 : 60000;
  pollTimeoutId = setTimeout(async () => {
    await checkVersion();
    schedulePoll();
  }, delay);
}
```

**UI indicator (lines 85-89):** Add a rebuilding banner similar to the existing `newVersionAvailable` banner:
```svelte
{#if serverRebuilding}
  <div class="rebuild-banner">Rebuilding...</div>
{/if}
```

---

## 2. InputArea Disabled State

**File:** `crates/commander-gui/ui/src/lib/components/InputArea.svelte`, lines 1-197

**Current disabled mechanism (line 14):**
```typescript
$: isDisabled = !$currentSession || isSending;
```

The input field is `disabled={isDisabled}` (line 184), which uses the HTML `disabled` attribute. When disabled:
- Input cannot be typed in (cursor shows not-allowed)
- Send button is also disabled (line 190)
- Visual: opacity 0.6, gray background (CSS lines 229-234)

**Problem for rebuild indicator:** Using `disabled` prevents BOTH typing and sending. We want to allow typing but prevent sending during rebuild.

**Recommended approach:**
- Add a new reactive variable: `$: canSend = !isDisabled && !$serverRebuilding;`
- Keep `disabled={isDisabled}` on the input field as-is (only disables when no session or sending)
- Change send button: `disabled={!canSend || !input.trim()}`
- Change `sendMessage()` guard: `if (!input.trim() || !canSend) return;`
- Update placeholder when rebuilding: `'Server rebuilding, please wait...'`
- The `serverRebuilding` state needs to come from a store (see section 4)

**Key lines to modify:**
- Line 14: Add `serverRebuilding` import from store
- Line 122: Guard `sendMessage()` with rebuild check
- Lines 177-182: Update placeholder logic
- Line 190: Update send button disabled condition

---

## 3. Telegram Rebuild Detection

**File:** `crates/commander-telegram/src/version.rs` (lines 1-243)

**Current mechanism:**
- `BotVersion` struct tracks `binary_hash`, `last_start`, `start_count`
- `compute_binary_hash()` hashes the executable's file size + modified time (lines 63-100)
- `check_rebuild()` loads saved version, calls `update()`, compares hashes (lines 179-194)
- Returns `(is_rebuild, is_first_start, start_count)` tuple
- This only detects **Telegram bot binary** changes, NOT API server rebuilds

**File:** `crates/commander-telegram/src/bot.rs` (lines 126-155)

**Current restart notification (lines 130-155, 841-862):**
- On bot startup, `check_rebuild()` is called (line 130)
- If not first start, `send_restart_notification()` sends a message to all restored sessions (line 149-155)
- The notification says: "Bot restarted -- reconnected to {project}" (line 848)

**Extending for API server rebuild detection:**
The Telegram bot could poll the API server health endpoint similarly to the Web UI. However, the Telegram bot and API server are separate processes. Options:

**Option A (recommended):** Add health polling in `poll_output_loop` or a new background task:
- Poll `http://localhost:9876/api/health` every 10 seconds
- Track consecutive failures
- When server goes down: send a message to connected chats: "API server rebuilding..."
- When server comes back: send "API server back online" with uptime info
- This leverages the existing `authorized_chats` list

**Option B:** Share the `version.rs` pattern -- have the API server write its own version file, and the Telegram bot reads it. More complex, less reliable for detecting "server is down right now."

**Key integration points:**
- `poll_output_loop()` at line 423 already runs every 500ms -- could add a less-frequent health check counter
- `poll_notifications_loop()` at line 745 runs every 2000ms -- alternative location
- `send_restart_notification()` at line 841 shows the pattern for broadcasting to connected users

---

## 4. App Store for Global State

**File:** `crates/commander-gui/ui/src/lib/stores/app.ts` (lines 1-73)

**Current stores:**
- `sessionMessages` -- Map<string, Message[]>
- `currentSession` -- Session | null
- `messages` -- derived from above
- `sessions` -- Session[]
- `botRunning` -- boolean
- `botPid` -- number | null

**There is no `serverStatus` or rebuild-related state.**

**Recommended addition (after line 72):**
```typescript
// Server health status for rebuild detection
export const serverRebuilding = writable<boolean>(false);
```

This allows:
- `WebApp.svelte` to write to it when health check fails
- `InputArea.svelte` to import and react to it
- Any other component to show rebuild status

**Alternative: richer status object:**
```typescript
export type ServerStatus = 'online' | 'rebuilding' | 'unknown';
export const serverStatus = writable<ServerStatus>('unknown');
```

---

## Summary of Changes Needed

### Files to Modify

| File | Change | Lines |
|------|--------|-------|
| `crates/commander-gui/ui/src/lib/stores/app.ts` | Add `serverRebuilding` writable store | After line 72 |
| `crates/commander-gui/ui/src/WebApp.svelte` | Track health failures, set store, dynamic poll interval, add rebuild banner | Lines 19-44, 85-89 |
| `crates/commander-gui/ui/src/lib/components/InputArea.svelte` | Import store, prevent send during rebuild, update placeholder | Lines 2-14, 122, 177-190 |
| `crates/commander-telegram/src/bot.rs` | Add API health polling background task | New function + spawn in `start_polling()` |

### Implementation Order

1. Add `serverRebuilding` store to `app.ts`
2. Update `WebApp.svelte` health polling to detect downtime and write to store
3. Update `InputArea.svelte` to read store and disable send (but not typing)
4. Add UI banner for rebuilding state
5. Add Telegram API server health polling (separate from bot rebuild detection)

### Polling Strategy

| State | Web UI Poll Interval | Telegram Poll Interval |
|-------|---------------------|----------------------|
| Server online | 60s | 10s |
| Server down (rebuilding) | 5s | 5s |
| Threshold to declare "rebuilding" | 2 consecutive failures | 3 consecutive failures |
