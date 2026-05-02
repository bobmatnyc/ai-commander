<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { sessions, currentSession, sessionMessages, addMessageToSession, activeSessions, githubStats, lastActivityAt, markSessionDataReceived, lastConnectedAt, markSessionConnected, showCreateSessionModal } from '../stores/app';
  import { subscribeSessionEvents, isDesktop, invoke, type SessionEventData } from '../transport';
  // Why: SessionList ships in both Tauri and web bundles. The transport wrapper
  // routes invoke() calls to either Tauri IPC or REST, depending on context. The
  // raw `@tauri-apps/api/core` invoke silently rejects in web mode, which is why
  // session click / disconnect / unregister all appeared to do nothing in the
  // web UI even though the buttons were wired up correctly.
  // Test: Run `npm run dev:web`, click a session row — assert the chat view
  // switches; click the unregister X — assert the row disappears.
  import { Activity, Plus, Terminal, Pencil, Settings, Square, Monitor, X } from 'lucide-svelte';
  import type { Session } from '../stores/app';
  import ProcessMonitorPanel from './ProcessMonitorPanel.svelte';

  // Sort mode: 'alpha' (A→Z by display name) or 'recent' (last active first,
  // connected-first fallback, then alpha). Persisted in localStorage so the
  // preference survives reload.
  type SortMode = 'alpha' | 'recent';

  function loadSortMode(): SortMode {
    try {
      const stored = localStorage.getItem('aic-session-sort');
      if (stored === 'alpha' || stored === 'recent') return stored;
    } catch {}
    return 'recent';
  }

  let sessionSort: SortMode = loadSortMode();

  /**
   * Why: Toggles between alpha and recent sort and persists the choice.
   * What: Flips sessionSort and writes to localStorage.
   * Test: Click sort toggle; assert sessionSort changes between 'alpha'/'recent'
   *       and localStorage.getItem('aic-session-sort') matches.
   */
  function toggleSort() {
    sessionSort = sessionSort === 'alpha' ? 'recent' : 'alpha';
    try {
      localStorage.setItem('aic-session-sort', sessionSort);
    } catch {}
  }

  /**
   * Why: Provides a deterministic sort of the session list for display.
   * What: For 'alpha' mode sorts A→Z by display name. For 'recent' mode sorts
   *       connected sessions first (by lastConnectedAt desc, then alpha), then
   *       disconnected (same), then registered (alpha). Uses connection time
   *       rather than activity time so sessions stay ranked by when the user
   *       chose to connect, not when random tmux output last arrived.
   * Test: Given sessions [C-disconnected, A-connected, B-registered], 'alpha'
   *       mode should return [A, B, C]; 'recent' mode with A connected most
   *       recently should return [A-connected, C-disconnected, B-registered].
   */
  function sortSessions(list: Session[], mode: SortMode, connectedMap: Map<string, number>): Session[] {
    const copy = [...list];
    if (mode === 'alpha') {
      copy.sort((a, b) => {
        const nameA = (a.nickname ?? a.name).toLowerCase();
        const nameB = (b.nickname ?? b.name).toLowerCase();
        return nameA.localeCompare(nameB);
      });
    } else {
      // Recent: connected first, then disconnected, then registered.
      // Within each group, most recently connected first; ties broken by alpha.
      const stateOrder = { connected: 0, disconnected: 1, registered: 2 } as const;
      const getState = (s: Session): 'connected' | 'disconnected' | 'registered' =>
        (s.session_state as any) || (s.is_connected ? 'connected' : 'disconnected');
      copy.sort((a, b) => {
        const sa = getState(a);
        const sb = getState(b);
        if (sa !== sb) return stateOrder[sa] - stateOrder[sb];
        const ta = connectedMap.get(a.name) ?? 0;
        const tb = connectedMap.get(b.name) ?? 0;
        if (ta !== tb) return tb - ta; // more recently connected first
        return (a.nickname ?? a.name).localeCompare(b.nickname ?? b.name);
      });
    }
    return copy;
  }

  // Derived sorted session list — re-evaluated whenever sessions, sort mode, or
  // connection map changes. sessionSort is a local variable so we reference it
  // inside a reactive block to ensure Svelte tracks it.
  $: sortedSessions = sortSessions($sessions, sessionSort, $lastConnectedAt);

  let interval: number;
  let lastError: string | null = null;
  let errorTimeout: number | null = null;
  let loadingSessionsInProgress = false;

  // Detect iOS/iPadOS — hide iTerm/Terminal buttons on these platforms
  const isIOS = typeof navigator !== 'undefined' && (
    /iPad|iPhone|iPod/.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  );

  // Git user — fetch once on mount for the initial badge
  let gitUser: string | null = null;
  let gitUserInitial = '';

  // Fetch git user.name for the user badge
  async function fetchGitUser() {
    try {
      const resp = await fetch('/api/health');
      // We don't have a git user endpoint yet, so use a static approach:
      // Read from the health response or fallback
      gitUser = null; // Will be populated if we add an endpoint
    } catch {}
  }

  // Try getting git user from the API config
  (async () => {
    try {
      const resp = await invoke('get_config');
      const config = resp as Record<string, unknown>;
      if (config?.git_user && typeof config.git_user === 'string') {
        gitUser = config.git_user;
      }
    } catch {}
    // Fallback: try fetch from /api/git-user if we add it later
    if (!gitUser) {
      try {
        const resp = await fetch('/api/git-user');
        if (resp.ok) {
          const data = await resp.json();
          gitUser = data.name || data.user || null;
        }
      } catch {}
    }
    if (gitUser) {
      // Get unique initial — first letter of first name
      gitUserInitial = gitUser.charAt(0).toUpperCase();
    }
  })();

  // Nickname editor state
  // Why: Per Fix 4 we replaced the tmux-rename flow with a "set display
  // nickname" flow. The nickname is a display-only override that's recorded
  // in ~/.ai-commander/session-overrides.json and leaves the underlying tmux
  // session name untouched — safer for existing tooling that matches on tmux
  // names.
  let editingNickname: string | null = null;
  let nicknameInput = '';
  let nicknameInputEl: HTMLInputElement | null = null;

  // Dropdown state: tracks which session's gear menu is open
  let openDropdown: string | null = null;

  // Unregister confirmation state: tracks which session is awaiting second click.
  // Why: A single-click would be too easy to trigger by accident — the two-click
  // confirmation matches the "Confirm?" pattern the user asked for and adds
  // friction without a full modal dialog. Auto-clears after 3s so a forgotten
  // pending row doesn't linger.
  let pendingUnregister: string | null = null;
  let unregisterTimeout: number | null = null;

  /**
   * Why: Sessions that share the same nickname (e.g. three tmux windows for the
   *      same project) would otherwise appear as identical rows, confusing the user.
   * What: Returns the nickname when unique, or appends the raw tmux name in brackets
   *       when another session resolves to the same display name.
   * Test: Pass two sessions with the same nickname; assert both results include [tmux-name].
   *       Pass two sessions with different nicknames; assert neither has a bracket suffix.
   */
  function getDisplayName(session: Session, allSessions: Session[]): string {
    const base = session.nickname ?? session.name;
    const hasDuplicate = allSessions.some(
      s => s.name !== session.name && (s.nickname ?? s.name) === base
    );
    return hasDuplicate ? `${base} [${session.name}]` : base;
  }

  /** Look up GitHub stats by session name, trying multiple key variants. */
  function getGithubStats(sessionName: string): { repo: string; open_issues: number; open_prs: number } | undefined {
    // Direct match
    if ($githubStats.has(sessionName)) return $githubStats.get(sessionName);
    // Strip common prefixes (e.g. "cmd-ai-commander" -> "ai-commander")
    const stripped = sessionName.replace(/^cmd-/, '');
    if ($githubStats.has(stripped)) return $githubStats.get(stripped);
    // Case-insensitive scan
    for (const [key, stats] of $githubStats.entries()) {
      if (key.toLowerCase() === sessionName.toLowerCase() ||
          key.toLowerCase() === stripped.toLowerCase()) {
        return stats;
      }
    }
    return undefined;
  }

  function sessionsEqual(a: Session[], b: Session[]): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (a[i].name !== b[i].name || a[i].is_connected !== b[i].is_connected) return false;
    }
    return true;
  }

  async function loadSessions() {
    // Guard against overlapping calls: if a fetch is already in flight, skip
    // this tick. This prevents duplicate entries appearing when the backend
    // responds slowly (>2 s) and the 2 s polling interval fires again.
    if (loadingSessionsInProgress) return;
    loadingSessionsInProgress = true;
    try {
      const result = await invoke('list_sessions') as Session[];
      // Deduplicate by session name as a frontend safety net in case the
      // backend ever returns the same tmux name more than once (e.g. two
      // project JSON files both matching the same session by path).
      const seen = new Set<string>();
      const deduped = result.filter(s => {
        if (seen.has(s.name)) return false;
        seen.add(s.name);
        return true;
      });
      if (!sessionsEqual(deduped, $sessions)) {
        sessions.set(deduped);
      }
    } catch (err) {
      console.error('Failed to load sessions:', err);
    } finally {
      loadingSessionsInProgress = false;
    }
  }

  async function connect(name: string) {
    lastError = null;
    if (errorTimeout) clearTimeout(errorTimeout);

    try {
      const priorMessages = $sessionMessages.get(name);
      const hasCachedHistory = priorMessages && priorMessages.length > 0;

      // `connect_session` now returns `{session, history}` — pre-populate the
      // chat with the persisted JSONL log so users see prior summaries without
      // waiting for the polling loop to re-emit them.
      const result = (await invoke('connect_session', { name })) as {
        session?: string;
        history?: Array<{ text: string; ts: number; hash: string }>;
      } | null;

      const session = $sessions.find(s => s.name === name);
      if (session) {
        currentSession.set({ ...session, is_connected: true });
        markSessionConnected(session.name);

        // Why: Connection state is now signaled visually (green pulse dot on
        // the row + green tinge in the ChatView header + live activity
        // counter). A "Connected to session" chat bubble was noisy and
        // redundant once those signals landed.
        // What: Just set currentSession and continue to history hydration —
        // no system message injection.
        // Test: Click Connect on a session, assert no "Connected to session"
        // message is added to $sessionMessages for that session.

        // History is replayed by ChatView.loadLogHistory via appendSummaryBullet
        // (triggered when $currentSession changes above). Hydrating here with
        // addMessageToSession created separate direction:'system' bubbles using
        // the old 'history HH:MM: text' format that bypassed consolidation.

        // Why: Previously we called `capture_session_output` here and injected
        // the raw 500-line tmux dump as a `direction: 'received'` ("claude")
        // message. That dumped raw terminal content into Summary view
        // immediately, BEFORE any LLM summarization ran. The polling loop
        // delivers output within 500 ms and always routes through the LLM
        // summarizer first, so the raw capture here was both redundant and
        // actively harmful to Summary view's contract ("only LLM-interpreted
        // messages, never raw terminal output"). Connect marker + log history
        // replay remain intact above.
        // Test: Connect to a fresh session, assert Summary view does NOT
        // contain a "claude" message with raw tmux content before the first
        // LLM interpretation arrives.
      }
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      const sessionObj = $sessions.find(s => s.name === name);
      const displayName = sessionObj ? getDisplayName(sessionObj, $sessions) : name;
      lastError = `Cannot connect to ${displayName}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);

      if ($currentSession) {
        addMessageToSession($currentSession.name, {
          direction: 'system',
          content: lastError,
          timestamp: new Date(),
        });
      }

      console.error('Failed to connect:', err);
    }
  }

  /**
   * Why: Users need to stop monitoring a specific session without touching
   * the ChatView (and without destroying the underlying tmux session).
   * What: Removes the session from the backend `connected_sessions` set and
   * refreshes the list so the row re-renders as "disconnected" (dimmed).
   * Test: Connect to a session, call this, assert the row's session_state
   * transitions from "connected" to "disconnected" and the ChatView clears
   * if the disconnected session was the current one.
   */
  async function disconnectSession(name: string) {
    closeDropdown();
    try {
      await invoke('disconnect_session', { name });
      if ($currentSession?.name === name) {
        currentSession.set(null);
      }
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to disconnect ${name}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  /**
   * Why: Registered-only rows (no tmux session) need a clean delete path.
   * Stop Session is inapplicable because there's nothing to destroy, so we
   * surface a dedicated confirmation-gated delete.
   * What: Prompts the user, then invokes `delete_registration` and reloads.
   * Test: Seed a registered project, click Delete, confirm, assert the row
   * disappears and the underlying JSON file is gone.
   */
  async function deleteRegistration(session: Session) {
    closeDropdown();
    const label = getDisplayName(session, $sessions);
    if (!confirm(`Remove registration for "${label}"?`)) return;
    try {
      await invoke('delete_registration', { name: session.name });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to delete registration: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  /**
   * Why: Registered projects need a one-click way to launch a tmux session
   * and immediately connect — the equivalent of opening a saved workspace.
   * What: Creates a tmux session using the project's stored path (default
   * adapter "mpm" — TODO: persist the adapter choice in the registration),
   * then reloads and auto-connects so the user lands directly in ChatView.
   * Test: Click Start on a registered row, assert a new tmux session appears
   * and the row transitions to "connected".
   */
  async function quickstart(session: Session) {
    closeDropdown();
    try {
      await invoke('create_session', {
        name: session.name,
        directory: session.path || '',
        adapter: 'mpm', // TODO: persist adapter choice in project registration
      });
      await loadSessions();
      await connect(session.name);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to start ${session.name}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  /** Convenience: label of a session's current lifecycle state. */
  function stateOf(session: Session): 'connected' | 'disconnected' | 'registered' {
    return (session.session_state as any) || (session.is_connected ? 'connected' : 'disconnected');
  }

  async function openInIterm(sessionName: string) {
    closeDropdown();
    await invoke('open_in_iterm', { sessionName });
  }

  async function openInTerminal(sessionName: string) {
    closeDropdown();
    await invoke('open_in_terminal_app', { sessionName });
  }

  /**
   * Why: Users want to "forget" a session — drop its project JSON from the
   * AI Commander registry — without destroying the underlying tmux session.
   * `delete_registration` only matches by name; `unregister_session` also
   * matches by pane path, so it cleanly works for running sessions.
   * What: First click on the ✕ button on a session row flags it as pending
   * and re-labels the button "Confirm?". Second click fires the invoke.
   * A timeout clears the flag after 3s to avoid stuck state.
   * Test: Click ✕ once, assert the button flips to "Confirm?". Click again,
   * assert `unregister_session` is invoked with the session name and the row
   * re-renders without the session while the tmux process survives.
   */
  async function handleUnregisterClick(sessionName: string, e: MouseEvent) {
    e.stopPropagation();
    if (unregisterTimeout) {
      clearTimeout(unregisterTimeout);
      unregisterTimeout = null;
    }
    if (pendingUnregister !== sessionName) {
      pendingUnregister = sessionName;
      unregisterTimeout = window.setTimeout(() => {
        pendingUnregister = null;
      }, 3000);
      return;
    }
    // Second click — fire the command.
    pendingUnregister = null;
    try {
      // Tauri v2 maps Rust snake_case params to JS camelCase invoke keys.
      // Rust param is `session_name: String`, so JS must pass `sessionName`.
      await invoke('unregister_session', { sessionName });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to unregister ${sessionName}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  /** Dropdown-menu entry variant — keeps the same two-click confirmation. */
  async function unregisterFromMenu(sessionName: string) {
    closeDropdown();
    try {
      // Tauri v2: Rust `session_name` param ↔ JS `sessionName` invoke key.
      await invoke('unregister_session', { sessionName });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to unregister ${sessionName}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  async function stopSession(sessionName: string) {
    closeDropdown();
    try {
      await invoke('stop_session', { name: sessionName });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to stop ${sessionName}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  /**
   * Why: Pre-fill the editor with the current display name (nickname if set,
   * otherwise the tmux session name) so the user can tweak it in place.
   * What: Opens the inline nickname editor for the given session and focuses
   * the input on the next tick.
   * Test: Click Set Nickname on a session with nickname "Foo" — assert the
   * input is populated with "Foo" and focused.
   */
  function startEditNickname(session: Session) {
    closeDropdown();
    editingNickname = session.name;
    nicknameInput = session.nickname ?? session.name;
    // Focus the input on next tick
    setTimeout(() => nicknameInputEl?.focus(), 0);
  }

  /**
   * Why: Persist the user's chosen nickname via the REST/Tauri endpoint, then
   * refresh the session list so the new display name renders.
   * What: Calls `set_session_nickname` with the trimmed input. An empty value
   * removes the override on the server side.
   * Test: Type a nickname, press enter, assert the session row re-renders with
   * the new display name after loadSessions() resolves.
   */
  async function saveNickname() {
    if (!editingNickname) return;
    const sessionName = editingNickname;
    const nickname = nicknameInput.trim();
    editingNickname = null;

    try {
      // Tauri v2: Rust `session_name` param ↔ JS `sessionName` invoke key.
      await invoke('set_session_nickname', { sessionName, nickname });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to set nickname: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  /**
   * Why: Users need an explicit way to revert a nicknamed session back to the
   * project-derived default; pressing "x" is faster than clearing the input
   * and hitting enter.
   * What: Posts an empty nickname, which on the server removes the override
   * entry entirely.
   * Test: Seed an override for a session, call this, assert the override is
   * gone from the JSON file.
   */
  async function clearNickname() {
    if (!editingNickname) return;
    const sessionName = editingNickname;
    editingNickname = null;

    try {
      // Tauri v2: Rust `session_name` param ↔ JS `sessionName` invoke key.
      await invoke('set_session_nickname', { sessionName, nickname: '' });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to clear nickname: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  function cancelNickname() {
    editingNickname = null;
    nicknameInput = '';
  }

  function handleNicknameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      saveNickname();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelNickname();
    }
  }

  function toggleDropdown(sessionName: string, e: MouseEvent) {
    e.stopPropagation();
    openDropdown = openDropdown === sessionName ? null : sessionName;
  }

  function closeDropdown() {
    openDropdown = null;
  }

  function handleGlobalClick() {
    closeDropdown();
  }

  // Tauri unlistener for the `session-auto-connected` backend event. Stored so
  // we can drop the subscription on component destroy.
  let unlistenAutoConnected: (() => void) | null = null;
  // Tauri unlistener for `session-output` — fires whenever any session emits
  // new content, used to pulse the green dot on the matching row.
  let unlistenSessionOutput: (() => void) | null = null;

  // Web mode: one SSE subscription per *connected* session so the dot pulses
  // for rows other than the currently-selected one. Keyed by session name so
  // we can tear down stale subscriptions when sessions disconnect.
  const sseSubscriptions = new Map<string, () => void>();

  // Ticker that forces a reactive re-evaluation of "recently active" state
  // every 200 ms. Why: Svelte reactivity only re-runs when a depended-upon
  // store changes; without this ticker, a row would pulse indefinitely or
  // never stop because the decay is purely time-based.
  let nowTick = Date.now();
  let nowInterval: number | null = null;

  /**
   * Why: Defines "recently active" so the pulse animation runs for a bounded
   * window after the last event. 3 s matches the activity decay used in
   * ChatView and feels snappy without flickering on rapid-fire events.
   * What: Returns true when the session's last activity timestamp is within
   * the last 3 s relative to the current `nowTick`.
   * Test: Call markSessionDataReceived('foo'), assert isRecentlyActive('foo')
   * is true; advance nowTick 4s into the future, assert it becomes false.
   */
  function isRecentlyActive(name: string): boolean {
    const ts = $lastActivityAt.get(name);
    return !!ts && (nowTick - ts) < 3000;
  }

  /**
   * Why: Keep SSE subscriptions in sync with the session list. When a new
   * session becomes "connected", we subscribe so its row can pulse. When it
   * disconnects, we tear down the subscription to avoid leaking EventSources.
   * What: Diffs the current set of connected session names against the
   * existing subscriptions and adds/removes as needed.
   * Test: Start with 0 connected sessions, reload with 2 connected — assert
   * sseSubscriptions.size === 2. Disconnect one — assert it drops to 1.
   */
  function syncSseSubscriptions(connectedNames: string[]) {
    if (isDesktop()) return; // Tauri events handle this
    const wanted = new Set(connectedNames);
    // Tear down subscriptions for sessions that are no longer connected
    for (const [name, cleanup] of sseSubscriptions) {
      if (!wanted.has(name)) {
        cleanup();
        sseSubscriptions.delete(name);
      }
    }
    // Add subscriptions for newly-connected sessions
    for (const name of wanted) {
      if (sseSubscriptions.has(name)) continue;
      const cleanup = subscribeSessionEvents(name, (data: SessionEventData) => {
        markSessionDataReceived(data.session_name || name);
      });
      sseSubscriptions.set(name, cleanup);
    }
  }

  // Reactive: whenever the session list updates, reconcile SSE subscriptions
  // against the set of connected sessions so the pulse dot tracks every
  // connected row, not just the selected one.
  $: syncSseSubscriptions(
    $sessions
      .filter(s => stateOf(s) === 'connected')
      .map(s => s.name)
  );

  onMount(() => {
    loadSessions();
    interval = window.setInterval(loadSessions, 2000);
    window.addEventListener('click', handleGlobalClick);

    // Re-evaluate "recently active" every 200ms. Keeps the pulse animation
    // starting/stopping cleanly without needing per-session timers.
    nowInterval = window.setInterval(() => { nowTick = Date.now(); }, 200);

    // Backend emits `session-auto-connected` once per session during startup
    // auto-connect. Refresh immediately rather than waiting for the 2s poll
    // so freshly-connected rows flip to the "connected" state without delay.
    listen('session-auto-connected', () => {
      loadSessions();
    })
      .then((fn) => { unlistenAutoConnected = fn; })
      .catch(() => { /* web mode uses the no-op shim; nothing to wire up */ });

    // Tauri mode: `session-output` fires for the currently-polled session.
    // We mark the session as recently active so its row pulses regardless
    // of whether it's the currently-selected one in the chat view.
    listen('session-output', (event: any) => {
      const payload = event?.payload || {};
      const name = payload.session_name || payload.session || payload.name || $currentSession?.name;
      if (name) markSessionDataReceived(name);
    })
      .then((fn) => { unlistenSessionOutput = fn; })
      .catch(() => { /* web mode — no-op */ });
  });

  onDestroy(() => {
    clearInterval(interval);
    if (nowInterval) clearInterval(nowInterval);
    window.removeEventListener('click', handleGlobalClick);
    if (unlistenAutoConnected) unlistenAutoConnected();
    if (unlistenSessionOutput) unlistenSessionOutput();
    // Tear down all SSE subscriptions
    for (const cleanup of sseSubscriptions.values()) cleanup();
    sseSubscriptions.clear();
  });
</script>

<div class="session-list">
  <div class="session-list-header">
    <h2 class="header-title">Sessions</h2>
    <div class="header-actions">
      <button
        class="sort-btn"
        on:click={toggleSort}
        title={sessionSort === 'alpha' ? 'Sorted A→Z — click for recent first' : 'Sorted by recent — click for A→Z'}
        aria-label="Toggle sort order"
      >
        {sessionSort === 'alpha' ? 'A↓' : '↓t'}
      </button>
      <button class="create-btn" on:click={() => $showCreateSessionModal = true} title="Create new session">
        <Plus size={16} />
        <span>New</span>
      </button>
    </div>
  </div>

  {#if lastError}
    <div class="error-banner">
      {lastError}
    </div>
  {/if}

  <div class="session-items">
    {#each sortedSessions as session}
      {@const s = stateOf(session)}
      <div
        class="session-item"
        class:active={$currentSession?.name === session.name}
        class:connected={s === 'connected'}
        class:disconnected={s === 'disconnected'}
        class:registered={s === 'registered'}
      >
        {#if editingNickname === session.name}
          <!-- Inline nickname editor (non-destructive display label) -->
          <form class="nickname-form" on:submit|preventDefault={saveNickname}>
            <input
              bind:this={nicknameInputEl}
              bind:value={nicknameInput}
              class="nickname-input"
              placeholder="Display nickname…"
              on:keydown={handleNicknameKeydown}
              spellcheck="false"
            />
            <button type="submit" class="nickname-save" title="Save nickname">✓</button>
            <button
              type="button"
              class="nickname-clear"
              on:click={clearNickname}
              title="Remove nickname (revert to project name)"
            >✕</button>
          </form>
        {:else}
          <!--
            `active` reflects whether a `session-output` / SSE event hit this
            session in the last 3s. `nowTick` forces Svelte re-evaluation
            every 200ms so the class decays cleanly without per-session timers.
          -->
          {@const active = s === 'connected' && nowTick > 0 && isRecentlyActive(session.name)}
          <!-- Normal session row -->
          <button
            class="session-main"
            on:click={() => s === 'registered' ? quickstart(session) : connect(session.name)}
            title={s === 'registered' ? 'Quickstart registered project' : 'Connect to session'}
          >
            <div class="session-info">
              <div class="name-row">
                <!--
                  Pulse dot — solid circle that reflects session state and
                  flashes when data arrives.
                -->
                <span
                  class="state-dot"
                  class:dot-connected={s === 'connected'}
                  class:dot-disconnected={s === 'disconnected'}
                  class:dot-registered={s === 'registered'}
                  class:dot-active={active}
                  title={s === 'connected' && active ? 'receiving data' : s}
                  aria-label={s === 'connected' && active ? 'receiving data' : s}
                ></span>
                <span class="session-name" title={session.name}>{getDisplayName(session, $sessions)}</span>
              </div>
              {#if session.path}
                <span class="session-path" title={session.path}>{session.path.replace(/^\/Users\/[^/]+/, '~')}</span>
              {/if}
            </div>
            <div class="session-badges">
              {#if getGithubStats(session.name)}
                {@const stats = getGithubStats(session.name)}
                {#if stats && stats.open_issues > 0}
                  <span class="badge badge-issues" title="{stats.repo}: {stats.open_issues} open issue{stats.open_issues > 1 ? 's' : ''}">
                    {stats.open_issues}
                  </span>
                {/if}
                {#if stats && stats.open_prs > 0}
                  <span class="badge badge-prs" title="{stats.repo}: {stats.open_prs} open PR{stats.open_prs > 1 ? 's' : ''}">
                    {stats.open_prs}
                  </span>
                {/if}
              {/if}
              {#if gitUser}
                <span class="user-initial" title={gitUser}>{gitUserInitial}</span>
              {/if}
              <span class="activity-icon" class:active={$activeSessions.has(session.name)} title={$activeSessions.has(session.name) ? 'Active' : 'Idle'}>
                <Activity size={14} />
              </span>
            </div>
          </button>

          <!-- Action buttons: always visible -->
          <div class="session-actions">
            <!-- iTerm2 button - hidden on iOS/iPadOS -->
            {#if !isIOS}
              <button
                class="action-btn iterm-btn"
                on:click|stopPropagation={() => openInIterm(session.name)}
                title="Open in iTerm2"
              >
                <Terminal size={14} />
              </button>
            {/if}

            <!-- Unregister button: two-click confirmation. Hidden on
                 registered-only rows where `delete_registration` is the correct
                 path (there's no tmux session to dissociate from). -->
            {#if s !== 'registered'}
              <button
                class="action-btn unregister-btn"
                class:pending={pendingUnregister === session.name}
                on:click={(e) => handleUnregisterClick(session.name, e)}
                title={pendingUnregister === session.name
                  ? 'Click again to confirm unregister'
                  : 'Unregister (remove from AIC registry, keep tmux alive)'}
              >
                {#if pendingUnregister === session.name}
                  <span class="confirm-label">Confirm?</span>
                {:else}
                  <X size={14} />
                {/if}
              </button>
            {/if}

            <!-- Gear dropdown button -->
            <div class="dropdown-wrapper">
              <button
                class="action-btn gear-btn"
                class:gear-open={openDropdown === session.name}
                on:click={(e) => toggleDropdown(session.name, e)}
                title="Session options"
              >
                <Settings size={13} />
              </button>

              {#if openDropdown === session.name}
                <div class="dropdown-menu" on:click|stopPropagation>
                  {#if s === 'registered'}
                    <!-- Registered: no tmux session running yet. -->
                    <button class="dropdown-item" on:click={() => quickstart(session)}>
                      <Activity size={13} />
                      <span>Start</span>
                    </button>
                    <div class="dropdown-divider"></div>
                    <button class="dropdown-item danger" on:click={() => deleteRegistration(session)}>
                      <Square size={13} />
                      <span>Delete Registration</span>
                    </button>
                  {:else if s === 'disconnected'}
                    <!-- Disconnected: tmux exists, not monitored. -->
                    <button class="dropdown-item" on:click={() => connect(session.name)}>
                      <Activity size={13} />
                      <span>Connect</span>
                    </button>
                    <button class="dropdown-item" on:click={() => startEditNickname(session)}>
                      <Pencil size={13} />
                      <span>Set Nickname</span>
                    </button>
                    {#if !isIOS}
                      <button class="dropdown-item" on:click={() => openInIterm(session.name)}>
                        <Terminal size={13} />
                        <span>Open in iTerm2</span>
                      </button>
                      <button class="dropdown-item" on:click={() => openInTerminal(session.name)}>
                        <Monitor size={13} />
                        <span>Open in Terminal.app</span>
                      </button>
                    {/if}
                    <div class="dropdown-divider"></div>
                    <button class="dropdown-item" on:click={() => unregisterFromMenu(session.name)}>
                      <X size={13} />
                      <span>Unregister (keep tmux)</span>
                    </button>
                    <button class="dropdown-item danger" on:click={() => stopSession(session.name)}>
                      <Square size={13} />
                      <span>Stop Session</span>
                    </button>
                    <button class="dropdown-item danger" on:click={() => deleteRegistration(session)}>
                      <Square size={13} />
                      <span>Delete Registration</span>
                    </button>
                  {:else}
                    <!-- Connected: tmux + monitored. -->
                    <button class="dropdown-item" on:click={() => disconnectSession(session.name)}>
                      <Activity size={13} />
                      <span>Disconnect</span>
                    </button>
                    <button class="dropdown-item" on:click={() => startEditNickname(session)}>
                      <Pencil size={13} />
                      <span>Set Nickname</span>
                    </button>
                    {#if !isIOS}
                      <button class="dropdown-item" on:click={() => openInIterm(session.name)}>
                        <Terminal size={13} />
                        <span>Open in iTerm2</span>
                      </button>
                      <button class="dropdown-item" on:click={() => openInTerminal(session.name)}>
                        <Monitor size={13} />
                        <span>Open in Terminal.app</span>
                      </button>
                    {/if}
                    <div class="dropdown-divider"></div>
                    <button class="dropdown-item" on:click={() => unregisterFromMenu(session.name)}>
                      <X size={13} />
                      <span>Unregister (keep tmux)</span>
                    </button>
                    <button class="dropdown-item danger" on:click={() => stopSession(session.name)}>
                      <Square size={13} />
                      <span>Stop Session</span>
                    </button>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <div class="no-sessions">
        <p>No sessions available</p>
      </div>
    {/each}
  </div>

  <!--
    Process monitor collapsible panel — pinned to the bottom of the session
    list. Bug 2 fix: surfaces process info in the main window instead of a
    separate Monitor tab, so users can spot stale children without a tab switch.
    Collapsed by default to preserve visual calm on the default view.
  -->
  <ProcessMonitorPanel defaultExpanded={false} />
</div>

<style>
  .session-list {
    display: flex;
    flex-direction: column;
    height: 100%;
    background-color: var(--bg-secondary);
  }

  .error-banner {
    background: rgba(220, 38, 38, 0.1);
    color: #dc2626;
    padding: 0.75rem;
    margin: 0.5rem;
    border-radius: 4px;
    border-left: 3px solid #dc2626;
    font-size: 0.875rem;
  }

  .session-list-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border);
    background-color: var(--bg-primary);
  }

  .header-title {
    font-size: 1.125rem;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.375rem;
  }

  /* Sort toggle — small, muted; matches the aesthetic of the action-btn row. */
  .sort-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 28px;
    min-width: 28px;
    padding: 0 6px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-secondary);
    font-size: 0.7rem;
    font-weight: 600;
    cursor: pointer;
    letter-spacing: 0.02em;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }

  .sort-btn:hover {
    background: var(--bg-surface);
    color: var(--text-primary);
    border-color: var(--text-secondary);
  }

  .create-btn {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.375rem 0.75rem;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 500;
    transition: background 0.2s;
  }

  .create-btn:hover {
    filter: brightness(1.1);
  }

  .session-items {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem;
  }

  .session-item {
    display: flex;
    align-items: center;
    width: 100%;
    margin-bottom: 0.5rem;
    border: 1px solid transparent;
    border-radius: 0.5rem;
    background-color: var(--bg-primary);
    transition: all 0.2s;
  }

  .session-item:hover {
    background-color: var(--bg-surface);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  }

  .session-item.active {
    background-color: var(--bg-surface);
    border-color: var(--accent);
  }

  /* Tri-state lifecycle visuals. Connected rows stay full-color, disconnected
   * rows dim, and registered rows go grayscale to hint "start me first". */
  .session-item.connected { opacity: 1.0; }
  .session-item.disconnected { opacity: 0.7; }
  .session-item.registered { opacity: 0.45; filter: grayscale(0.6); }

  .name-row {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    min-width: 0;
  }

  /*
   * 8px circular pulse dot — gray when disconnected/registered, solid green
   * when connected-idle, and animated (expanding ring) when data is flowing
   * within the last 3 s. Implemented with a background-color on the element
   * and an ::after pseudo for the expanding ring so we don't need to add a
   * separate DOM node per row.
   */
  .state-dot {
    position: relative;
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    background-color: var(--text-secondary, #999);
    transition: background-color 0.2s, box-shadow 0.2s;
  }

  .dot-connected {
    background-color: #22c55e;
  }

  .dot-disconnected {
    background-color: var(--text-secondary, #999);
    opacity: 0.55;
  }

  .dot-registered {
    background-color: var(--text-secondary, #999);
    opacity: 0.35;
  }

  /* Active pulse — brighter core + expanding ring */
  .dot-active {
    background-color: #4ade80;
    box-shadow: 0 0 6px rgba(74, 222, 128, 0.7);
  }

  .dot-active::after {
    content: '';
    position: absolute;
    inset: -2px;
    border-radius: 50%;
    border: 2px solid rgba(74, 222, 128, 0.7);
    animation: dot-ring 1.2s ease-out infinite;
    pointer-events: none;
  }

  @keyframes dot-ring {
    0% {
      transform: scale(0.8);
      opacity: 0.9;
    }
    100% {
      transform: scale(2.2);
      opacity: 0;
    }
  }

  .session-main {
    display: flex;
    flex: 1;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 0.75rem;
    border: none;
    background: transparent;
    cursor: pointer;
    text-align: left;
    min-width: 0;
  }

  .session-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .session-badges {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    flex-shrink: 0;
  }

  .session-name {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    max-width: 100%;
  }

  .session-path {
    font-size: 0.7rem;
    color: var(--text-secondary);
    opacity: 0.65;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
    text-align: left;
  }

  /* Action buttons row - always visible */
  .session-actions {
    display: flex;
    align-items: center;
    gap: 0.125rem;
    padding-right: 0.375rem;
    flex-shrink: 0;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: 1px solid var(--border);
    border-radius: 0.25rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
    flex-shrink: 0;
  }

  .action-btn:hover {
    background: var(--bg-surface);
    color: var(--text-primary);
    border-color: var(--text-secondary);
  }

  .iterm-btn:hover {
    color: var(--accent);
    border-color: var(--accent);
  }

  /* Unregister button — neutral by default, bright red in pending state so the
   * "Confirm?" label is impossible to miss. */
  .unregister-btn:hover {
    color: #dc2626;
    border-color: #dc2626;
  }

  .unregister-btn.pending {
    color: white;
    background: #dc2626;
    border-color: #dc2626;
    width: auto;
    padding: 0 6px;
  }

  .unregister-btn.pending:hover {
    filter: brightness(1.1);
  }

  .confirm-label {
    font-size: 0.7rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .gear-btn:hover,
  .gear-btn.gear-open {
    color: var(--text-primary);
    border-color: var(--text-secondary);
    background: var(--bg-surface);
  }

  /* Nickname inline form (compact, inline) */
  .nickname-form {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 1;
    padding: 0.375rem 0.5rem;
  }

  .nickname-input {
    flex: 1;
    font-size: 0.8rem;
    padding: 2px 6px;
    border: 1px solid #3b82f6;
    border-radius: 4px;
    background: var(--bg-primary);
    color: var(--text-primary);
    outline: none;
  }

  .nickname-input:focus {
    box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.3);
  }

  .nickname-save,
  .nickname-clear {
    background: none;
    border: none;
    cursor: pointer;
    padding: 2px 4px;
    font-size: 0.8rem;
  }

  .nickname-save { color: #10b981; }
  .nickname-clear { color: #ef4444; }

  /* Dropdown */
  .dropdown-wrapper {
    position: relative;
  }

  .dropdown-menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 100;
    min-width: 180px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    padding: 0.25rem;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.45rem 0.625rem;
    border: none;
    border-radius: 0.25rem;
    background: transparent;
    color: var(--text-primary);
    font-size: 0.8125rem;
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
  }

  .dropdown-item:hover {
    background: var(--bg-surface);
  }

  .dropdown-item.danger {
    color: #dc2626;
  }

  .dropdown-item.danger:hover {
    background: rgba(220, 38, 38, 0.1);
  }

  .dropdown-divider {
    height: 1px;
    background: var(--border);
    margin: 0.25rem 0;
  }

  .no-sessions {
    padding: 2rem 1rem;
    text-align: center;
    color: var(--text-secondary);
    font-size: 0.875rem;
  }

  .activity-icon {
    flex-shrink: 0;
    color: var(--text-secondary, #999);
    opacity: 0.4;
    display: flex;
    align-items: center;
  }

  .activity-icon.active {
    color: #22c55e;
    opacity: 1;
    animation: ekg-pulse 1.2s ease-in-out infinite;
  }

  @keyframes ekg-pulse {
    0%, 100% { transform: scaleY(1); opacity: 1; }
    25% { transform: scaleY(1.3); opacity: 1; }
    50% { transform: scaleY(0.8); opacity: 0.7; }
    75% { transform: scaleY(1.2); opacity: 1; }
  }

  .badge {
    font-size: 0.65rem;
    padding: 0.1rem 0.35rem;
    border-radius: 9999px;
    font-weight: 600;
    line-height: 1;
    flex-shrink: 0;
  }

  .badge-issues {
    background: rgba(245, 158, 11, 0.15);
    color: #d97706;
  }

  .badge-prs {
    background: rgba(59, 130, 246, 0.15);
    color: #3b82f6;
  }

  .user-initial {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: rgba(139, 92, 246, 0.15);
    color: #8b5cf6;
    font-size: 0.6rem;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    cursor: default;
  }
</style>
