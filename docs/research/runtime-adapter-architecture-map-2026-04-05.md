# RuntimeAdapter Architecture Map

**Date:** 2026-04-05
**Purpose:** Map current `RuntimeAdapter` architecture to inform design of event-driven adapter extension.

## 1. Trait Contract

Location: `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/traits.rs`

```rust
pub trait RuntimeAdapter: Send + Sync {
    fn info(&self) -> &AdapterInfo;
    fn launch_command(&self, project_path: &str) -> (String, Vec<String>);
    fn analyze_output(&self, output: &str) -> OutputAnalysis;
    fn is_idle(&self, output: &str) -> bool { /* default via analyze_output */ }
    fn is_error(&self, output: &str) -> bool { /* default via analyze_output */ }
    fn format_message(&self, message: &str) -> String { /* default: identity */ }
    fn idle_patterns(&self) -> &[&str];
    fn error_patterns(&self) -> &[&str];
}
```

**Supporting types:**
- `AdapterInfo { id, name, description, command, default_args: Vec<String> }`
- `RuntimeState { Starting, Idle, Working, Error, Stopped }`
- `OutputAnalysis { state, confidence: f32, errors: Vec<String>, data: HashMap<String,String> }`

**Key observations:**
- Every method is a **pure function of a text string** (tmux scrollback). No I/O, no async, no streaming.
- `analyze_output` is the core: all other state-checking methods delegate to it.
- `launch_command` returns a `(cmd, args)` tuple intended to be run in a shell (tmux pane).

## 2. Call Graph — Who Calls Adapter Methods

| Call Site | Method | Purpose |
|---|---|---|
| `commander-runtime/src/poller.rs:108` | `adapter.analyze_output(&output)` | **Primary state-detection path.** Polls tmux, maps `RuntimeState` → `ProjectState`. |
| `commander-runtime/src/executor.rs:147` | `adapter.launch_command(&project.path)` | Builds shell command, sends to tmux pane via `send_line`. |
| `commander-telegram/src/state.rs:682, 858, 1987` | `adapter.launch_command(...)` | Telegram bot spawns tmux sessions the same way. |
| `commander-orchestrator/src/orchestrator.rs:151` | `.analyze_output(output)` | State sync for orchestrator-managed instances. |
| `commander-agent/src/session_agent/{analysis,tools}.rs` | `analyze_output(output).await` | SessionAgent's LLM-backed analyzer (different trait, not RuntimeAdapter; note: `async`). |
| `ai-commander/src/repl.rs:1405`, `tui/connection.rs:78` | `adapter.launch_command(...)` | CLI/TUI launch paths. |
| `commander-runtime/src/executor.rs:40` | `adapter.info().id` | Debug/logging. |

**Output capture mechanism (universal):**
All callers capture tmux pane output via `TmuxOrchestrator::capture_output(session, None, Some(50))` (last 50 lines) and feed that string into `analyze_output`. The `OutputPoller` runs on a fixed `poll_interval` ticker (`runtime/src/poller.rs:30-54`).

## 3. Session / Spawn Model

**Authoritative path:** `RuntimeExecutor::start()` in `commander-runtime/src/executor.rs:120-179`

1. Caller supplies `Arc<dyn RuntimeAdapter>` + `Project`.
2. Executor calls `adapter.launch_command(&project.path)` → `(cmd, args)`.
3. Creates a tmux session: `tmux.create_session(&session_name)`.
4. Sends `cmd args` as a shell line to the pane: `tmux.send_line(...)`.
5. Stores a `RunningInstance { project_id, session_name, adapter, started_at, last_output, state }` in `Arc<RwLock<HashMap<..>>>`.
6. `OutputPoller` runs in a separate task, ticks every `poll_interval`, for each instance:
   - Captures last 50 lines of tmux output.
   - If changed: emits `RuntimeEvent::OutputReceived { project_id, output }`.
   - Calls `adapter.analyze_output(&output)` → maps to `ProjectState`.
   - If state changed, emits state-change event via `executor.update_state`.

**Assumption baked in everywhere:** *the adapter's runtime is a long-lived process whose status is inferred by scraping its terminal output at a fixed interval.* Telegram layer adds a secondary time-based idleness check (`session.is_idle(1500)` = 1.5s silence) on top of adapter-level analysis.

## 4. MPM SDK Event Model

Location: `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/`

**Event enum** (`types.rs:74-84`):
```rust
pub enum AgentEvent { Text(String), ToolUse(String), Complete(AgentResult), Error(String) }
```

**Client API** (`client.rs`):
- `MpmClient::discover() -> Result<Self, MpmError>`: uses `which claude-mpm`, cwd = current dir.
- `MpmClient::new(binary, cwd)`, `.with_timeout(secs)`.
- `run(agent_id, prompt) -> AgentResult` (collects all events, returns final).
- `run_streaming(agent_id, prompt, tx: mpsc::Sender<AgentEvent>) -> Result<()>` — the **event-driven entry point**.

**Under the hood:** spawns `claude-mpm run --headless --non-interactive --output-format stream-json -i <prompt>` as a Tokio child process, reads NDJSON lines from stdout line-by-line, parses each into an `AgentEvent`, and sends via `mpsc::Sender`. Session IDs are extracted from the init line and retained for `--resume`.

**Stateful-ish:** each `MpmClient` remembers `last_session_id`, but every `run` spawns a fresh subprocess. It is not a persistent connection — it's a per-turn subprocess that happens to be resumable via CLI flag.

**Prior-art usage:** `commander-telegram/src/handlers.rs:2691-2800` (`spawn_agent_with_streaming`):
- `MpmClient::discover()` → new client per command.
- Creates `mpsc::channel::<AgentEvent>(64)`, spawns `run_streaming` in a background task.
- Loops on `rx.recv().await`, translating events to Telegram message edits:
  - `Text(chunk)` → accumulate + edit status message (rate-limited to 2s).
  - `ToolUse(tool)` → debug log only.
  - `Complete(result)` → delete status message, send final message with cost/duration footer.
  - `Error(e)` → send error message.

This is a clean, **pull-based event loop driving a UI** — no tmux, no polling, no pattern matching.

## 5. The Architectural Gap

**Current (terminal-scraping) model:**
```
spawn CLI in tmux → poll scrollback → regex-match last 50 lines → emit ProjectState
```
- Stateless adapter methods, pure `&str -> OutputAnalysis`.
- Session identity = tmux session name.
- Output = tmux scrollback string.
- Control inversion: `OutputPoller` drives; adapters answer questions.

**SDK (event-driven) model:**
```
call client → subprocess emits NDJSON → typed AgentEvent stream → render however
```
- Stateful client (`MpmClient` with `last_session_id`).
- Session identity = `claude-mpm`'s own session ID (resume token).
- Output = sequence of typed events over an `mpsc::Sender`.
- Control inversion: adapter *produces* events; consumer drives the loop.

**Impedance mismatches:**

1. **Launch contract:** `launch_command() -> (String, Vec<String>)` assumes "things you run in a shell." An event-driven adapter has no shell command — it has a `run_streaming(agent_id, prompt, tx)` call. The tmux pane becomes irrelevant.

2. **State detection:** `analyze_output(&str)` is synchronous, pure, and text-based. Event-driven sources already *know* their state (they emit `Complete` / `Error` directly). There is no text to analyze.

3. **Session lifecycle:** `RunningInstance` assumes one tmux session per project, long-lived. MPM-SDK spawns a subprocess per turn. The adapter would need to own connection/session state that the current trait doesn't model.

4. **Who drives whom:** `OutputPoller` currently **pulls** by calling `analyze_output` on a timer. An event-driven adapter needs to **push** state changes when they happen, not when polled.

5. **Input path:** In the current model, user input is fed via `tmux.send_line` directly into the pane (bypassing the adapter). In the SDK model, input is an argument to `run_streaming(prompt, ...)` — the adapter must participate in the send path.

**What an event-driven adapter would need to produce to integrate with the existing poller:**
- **Option A (synthetic output):** Maintain an internal rolling buffer; translate `AgentEvent`s into synthetic terminal-style text (`"[ToolUse] bash\n"`, `"[Complete] done\n"`) for `capture_output` replacement. Low code churn, high semantic loss, ugly.
- **Option B (state callbacks):** Give the adapter a way to *push* `RuntimeState` changes directly to the executor, bypassing `OutputPoller`. Clean, but requires plumbing.
- **Option C (new trait):** Introduce a parallel trait for event-driven adapters and let the executor handle both shapes.

## 6. Design Options for Trait Extension

### Option 1 — Augment existing trait with default async methods
Add optional methods to `RuntimeAdapter` that event-driven adapters override; terminal adapters keep current behavior.

```rust
pub trait RuntimeAdapter: Send + Sync {
    // existing methods unchanged...

    /// If true, adapter produces events; skip polling.
    fn is_event_driven(&self) -> bool { false }

    /// Optional event stream. Terminal adapters return None.
    async fn run_session(&self, ctx: SessionContext, prompt: &str)
        -> Option<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>> { None }
}
```
- **Pros:** Single trait, gradual migration, registry/dispatch unchanged.
- **Cons:** Trait becomes schizophrenic — half the methods are no-ops for each kind. `async fn` in traits adds `async-trait` or trait-object complexity. Executor still needs branching logic (`if is_event_driven { stream } else { poll }`).

### Option 2 — Sibling trait + enum dispatch
Keep `RuntimeAdapter` as-is; add `EventDrivenAdapter` with its own contract.

```rust
pub trait EventDrivenAdapter: Send + Sync {
    fn info(&self) -> &AdapterInfo;
    async fn start_session(&self, ctx: SessionContext) -> Result<SessionHandle>;
    async fn send(&self, handle: &SessionHandle, msg: &str) -> Result<()>;
    fn subscribe(&self, handle: &SessionHandle) -> impl Stream<Item = RuntimeEvent>;
}

pub enum AnyAdapter {
    Terminal(Arc<dyn RuntimeAdapter>),
    EventDriven(Arc<dyn EventDrivenAdapter>),
}
```
- **Pros:** Clean separation of concerns; each trait is cohesive; type system enforces correct usage; registry can hold both.
- **Cons:** Executor + poller + telegram/TUI all need `match` statements at every adapter boundary. Two parallel code paths forever. Shared concerns (info, naming) duplicated.

### Option 3 — Unify around an event stream (recommended)
Redefine the adapter contract around a single event stream; terminal adapters get a shim that runs the poller internally and emits the same event enum.

```rust
pub trait RuntimeAdapter: Send + Sync {
    fn info(&self) -> &AdapterInfo;
    async fn spawn(&self, project: &Project) -> Result<SessionHandle>;
    async fn send(&self, handle: &SessionHandle, msg: &str) -> Result<()>;
    fn events(&self, handle: &SessionHandle)
        -> Pin<Box<dyn Stream<Item = AdapterEvent> + Send>>;
    async fn stop(&self, handle: SessionHandle) -> Result<()>;
}

pub enum AdapterEvent {
    StateChanged(RuntimeState),
    Output(String),              // raw text for UI display
    ToolUse { name: String },    // structured
    Error(String),
    Complete { summary: Option<String> },
}
```
Terminal adapters implement this by internally owning a tmux session + poll loop, translating `analyze_output` results into `AdapterEvent::StateChanged`. Event-driven adapters (mpm-sdk) map `AgentEvent` → `AdapterEvent` directly.

- **Pros:** Unified consumer API; poller concern moves inside terminal-adapter impls where it belongs; telegram/TUI consume one stream type; naturally handles both kinds without branching; future adapter kinds (WebSocket, SSE, gRPC) fit without trait changes.
- **Cons:** Larger refactor — every call site of `analyze_output`/`launch_command` must migrate. Requires designing `SessionHandle` carefully. Tmux-based state becomes the adapter's private concern (actually a feature).

## Recommendation

Option 3 is the correct long-term shape: it treats the existing terminal-scraping as *one implementation strategy* among many, rather than baking it into the trait. The path of least disruption is a two-step migration:
1. Introduce Option 3's trait as `RuntimeAdapterV2` (or `EventSource`) alongside the current trait.
2. Provide a `TerminalAdapter<T: RuntimeAdapter>` wrapper that implements V2 by running the existing pattern-match poll loop internally. This lets every existing adapter keep working unchanged.
3. Implement `MpmSdkAdapter: RuntimeAdapterV2` directly on top of `MpmClient::run_streaming`.
4. Migrate consumers (poller, telegram, TUI) to V2 one at a time. Eventually delete the old trait.

## Key Files

- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/traits.rs` — trait definition
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/{claude_code,mpm,shell}.rs` — concrete adapters
- `/Users/masa/Projects/ai-commander/crates/commander-adapters/src/registry.rs` — `AdapterRegistry`
- `/Users/masa/Projects/ai-commander/crates/commander-runtime/src/executor.rs` — `RunningInstance`, spawn path (line 120-179)
- `/Users/masa/Projects/ai-commander/crates/commander-runtime/src/poller.rs` — `OutputPoller` (line 57-128 is the core loop)
- `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/client.rs` — `MpmClient`, `run_streaming`
- `/Users/masa/Projects/ai-commander/crates/mpm-sdk/src/types.rs` — `AgentEvent` variants
- `/Users/masa/Projects/ai-commander/crates/commander-telegram/src/handlers.rs` — `spawn_agent_with_streaming` (line 2691-2800), prior art for event-driven flow
