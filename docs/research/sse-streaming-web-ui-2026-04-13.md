# SSE Streaming for Web UI Output Parity

**Date**: 2026-04-13
**Status**: Actionable research
**Related**: Item 2 - SSE streaming for web UI

## Current State

### Telegram (push-based)
- `event_consumer.rs` consumes `RuntimeEvent` stream (TextChunk, ToolUse, Complete, Error)
- Progressive summarization every 500 chars via `summarize_incremental_tiered()`
- Final summarization via `summarize_with_fallback()` for responses >2000 chars
- Live message editing every 2s (`EDIT_INTERVAL`)

### Web UI (poll-based)
- `ChatView.svelte` listens to Tauri events (`session-output`, `chat-event`)
- In **web mode**: no Tauri events fire. Output is only fetched via:
  - `interpret_session` on session switch (initial load)
  - Auto-interpret every 50 lines (`SUMMARY_THRESHOLD`) -- but only fires when `session-output` arrives, which requires Tauri
- **Gap**: Web client has NO push mechanism. The 50-line auto-summarize only works in Tauri because it depends on `listen('session-output')` events.

### Transport layer (`transport.ts`)
- Pure request/response. No SSE/EventSource support.
- Detects Tauri vs browser; browser mode uses `fetch()` against REST API.

## Implementation Plan

### Architecture

```
tmux pane --> poll loop (2s) --> diff detector --> interpret if changed
                                      |
                                      v
                               broadcast::channel
                                      |
                              +-------+-------+
                              |               |
                         SSE client 1    SSE client 2
                         (browser)       (browser)
```

### 1. Backend: SSE endpoint (`crates/commander-api/src/handlers/web.rs`)

**Axum 0.8 supports SSE** via `axum::response::sse::{Event, Sse}`. No new dependencies needed.

Add a background polling task + broadcast channel to `AppState`:

```rust
// In state.rs - add to AppState:
pub session_events: tokio::sync::broadcast::Sender<SessionEvent>,

#[derive(Clone, Debug, Serialize)]
pub struct SessionEvent {
    pub session: String,
    pub event_type: String, // "output" | "interpretation" | "idle"
    pub content: String,
}
```

**New handler** in `web.rs`:

```rust
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;

pub async fn session_event_stream(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.session_events.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(move |msg| {
            match msg {
                Ok(evt) if evt.session == name => {
                    let data = serde_json::to_string(&evt).unwrap_or_default();
                    Some(Ok(Event::default().data(data)))
                }
                _ => None,
            }
        });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
    )
}
```

**New route** in `router.rs`:
```rust
.route("/api/sessions/{name}/events", get(handlers::web::session_event_stream))
```

### 2. Background poller (new task in daemon or API startup)

Spawn a task that polls tmux for each active session every 2-3s, detects output changes, interprets, and broadcasts:

```rust
async fn session_output_poller(tmux: Arc<TmuxOrchestrator>, tx: broadcast::Sender<SessionEvent>) {
    let mut snapshots: HashMap<String, String> = HashMap::new();
    loop {
        if let Ok(sessions) = tmux.list_sessions() {
            for session in sessions.iter().filter(|s| s.name.starts_with("cmd-")) {
                let raw = tmux.capture_output(&session.name, None, Some(100)).unwrap_or_default();
                let prev = snapshots.get(&session.name).cloned().unwrap_or_default();
                if raw != prev {
                    snapshots.insert(session.name.clone(), raw.clone());
                    // Interpret
                    let cleaned = commander_core::clean_response(&raw);
                    let is_idle = commander_core::is_claude_ready(&cleaned);
                    let interpretation = tokio::task::spawn_blocking(move || {
                        commander_core::interpret_screen_context(&cleaned, is_idle)
                    }).await.unwrap_or(None);

                    if let Some(text) = interpretation {
                        let _ = tx.send(SessionEvent {
                            session: session.name.clone(),
                            event_type: "interpretation".into(),
                            content: text,
                        });
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
```

### 3. Frontend: EventSource in transport.ts

```typescript
export function subscribeSessionEvents(
  sessionName: string,
  onEvent: (evt: { event_type: string; content: string }) => void,
  onError?: (err: Event) => void
): () => void {
  if (isDesktop()) {
    // Tauri: use existing listen() mechanism
    return () => {};
  }
  const url = `/api/sessions/${encodeURIComponent(sessionName)}/events`;
  const es = new EventSource(url);
  es.onmessage = (e) => {
    try { onEvent(JSON.parse(e.data)); } catch {}
  };
  es.onerror = (e) => { onError?.(e); };
  return () => es.close();
}
```

### 4. Frontend: ChatView.svelte integration

In `onMount`, when NOT in Tauri mode, subscribe to SSE:

```typescript
import { subscribeSessionEvents, isDesktop } from '../transport';

// Inside onMount, after existing listeners:
let unsubscribeSSE: (() => void) | null = null;

if (!isDesktop() && $currentSession) {
  unsubscribeSSE = subscribeSessionEvents($currentSession.name, (evt) => {
    if (evt.event_type === 'interpretation') {
      addMessageToSession($currentSession.name, {
        direction: 'system',
        content: evt.content,
        timestamp: new Date(),
      });
      if (autoScroll) setTimeout(scrollToBottom, 10);
    }
  });
}

// In cleanup:
return () => {
  unsubscribeSSE?.();
  // ...existing cleanup
};
```

## Data Flow

1. **tmux** has new output in session pane
2. **Poller** (every 3s) captures output, detects diff from previous snapshot
3. **Poller** calls `interpret_screen_context()` (blocking LLM call via OpenRouter)
4. **Poller** sends `SessionEvent` into `broadcast::channel`
5. **Axum SSE handler** filters events for the subscribed session, serializes as SSE `data:`
6. **Browser EventSource** receives event, parses JSON
7. **ChatView.svelte** adds interpretation as system message

## Files to Modify

| File | Change |
|------|--------|
| `crates/commander-api/src/state.rs` | Add `broadcast::Sender<SessionEvent>` to `AppState` |
| `crates/commander-api/src/handlers/web.rs` | Add `session_event_stream` SSE handler + `SessionEvent` type |
| `crates/commander-api/src/router.rs` | Add `/api/sessions/{name}/events` GET route |
| `crates/commander-api/src/lib.rs` or startup | Spawn `session_output_poller` task |
| `crates/commander-gui/ui/src/lib/transport.ts` | Add `subscribeSessionEvents()` function |
| `crates/commander-gui/ui/src/lib/components/ChatView.svelte` | Subscribe to SSE in web mode |

## Dependencies

- **No new crate dependencies**. Axum 0.8 has `axum::response::sse` built-in. `tokio-stream` (for `BroadcastStream`) is already a transitive dep of tokio.
- `tower-http` 0.6 is fine.
- `tokio::sync::broadcast` is in tokio (already a workspace dep).

## Risks and Gotchas

1. **LLM rate limiting**: Poller interprets every diff. If sessions are very active, this hammers OpenRouter. Mitigation: debounce -- only interpret if >500 chars changed or >5s since last interpretation.
2. **Broadcast channel backpressure**: `broadcast::channel` drops old messages if a slow receiver lags. Use capacity of ~64; SSE clients that reconnect will get fresh state.
3. **Connection cleanup**: `EventSource` auto-reconnects on disconnect. The Axum SSE stream drops when the client disconnects (stream returns `None`). No explicit cleanup needed.
4. **CORS**: Already configured with `Any` origin. SSE works over regular HTTP GET, so CORS is fine.
5. **Multiple sessions**: Each SSE connection filters by session name. Client opens one SSE per active session view; closes on session switch.
6. **`interpret_screen_context` is blocking**: Uses `reqwest::blocking`. Must run via `spawn_blocking` (already shown in plan). Consider migrating to async `reqwest` long-term.
7. **Auth**: SSE endpoint should check the auth token via query param (`?token=...`) since `EventSource` API doesn't support custom headers. Add a query param extractor.
