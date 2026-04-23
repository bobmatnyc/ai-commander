<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import SettingsModal from './lib/components/SettingsModal.svelte';
  import CreateSessionModal from './lib/components/CreateSessionModal.svelte';
  import { Sun, Moon, Settings } from 'lucide-svelte';
  import { resolvedTheme, setTheme } from './lib/stores/theme';
  import { currentSession, sessions, serverRebuilding, githubStats, showCreateSessionModal, markSessionConnected, addMessageToSession } from './lib/stores/app';
  import { get } from 'svelte/store';
  import { invoke } from './lib/transport';

  // No auth needed — Tailscale handles network security

  // Bug 2 fix: the Monitor tab was replaced by a collapsible
  // ProcessMonitorPanel inside SessionList — see App.svelte for rationale.
  let showSettings = false;

  /**
   * Why: On mobile (≤768px) the two-panel layout collapses to a master-detail
   * flow. We track which "pane" is visible so the user lands on the session
   * list at startup rather than an empty chat view.
   * What: 'list' shows the session panel full-width; 'chat' shows the chat
   * panel full-width. On desktop both panels are always visible and this
   * variable has no effect.
   * Test: Load on a ≤768px viewport — assert the session list is visible and
   * the chat panel is not. Tap a session — assert the chat panel appears and
   * the session list is hidden. Tap "← Sessions" — assert the session list
   * returns and currentSession is cleared.
   */
  let mobileView: 'list' | 'chat' = 'list';

  // Version check + rebuild detection via dynamic health polling
  let loadedVersion: string | null = null;
  let newVersionAvailable = false;
  let healthFailures = 0;
  let healthTimeout: ReturnType<typeof setTimeout>;
  let githubInterval: ReturnType<typeof setInterval>;

  async function fetchGithubStats() {
    try {
      const result = await invoke('get_github_stats') as any;
      if (result?.stats) {
        $githubStats = new Map(Object.entries(result.stats));
      }
    } catch {}
  }

  async function checkHealth() {
    try {
      const resp = await fetch('/api/health');
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const data = await resp.json();

      // Server is back after downtime — check for new version
      if ($serverRebuilding && healthFailures > 0) {
        if (loadedVersion && data.version !== loadedVersion) {
          newVersionAvailable = true;
        }
      }

      healthFailures = 0;
      $serverRebuilding = false;

      if (loadedVersion === null) {
        loadedVersion = data.version;
      } else if (data.version !== loadedVersion) {
        newVersionAvailable = true;
      }

      // Poll again in 15s when healthy (fast enough to catch rebuilds)
      healthTimeout = setTimeout(checkHealth, 15000);
    } catch {
      healthFailures++;
      if (healthFailures >= 2) {
        $serverRebuilding = true;
      }
      // Poll more frequently when down (every 3s)
      healthTimeout = setTimeout(checkHealth, 3000);
    }
  }

  onMount(() => {
    checkHealth();
    fetchGithubStats();
    githubInterval = setInterval(fetchGithubStats, 1800000);
  });

  onDestroy(() => {
    if (healthTimeout) clearTimeout(healthTimeout);
    if (githubInterval) clearInterval(githubInterval);
  });

  function toggleTheme() {
    setTheme($resolvedTheme === 'dark' ? 'light' : 'dark');
  }

  // On mobile: navigate to chat when a session is selected, return to list
  // when the session is cleared (e.g. disconnect or back button).
  $: if ($currentSession) {
    mobileView = 'chat';
  } else {
    mobileView = 'list';
  }

  /**
   * Why: The back button in the mobile chat header needs to clear the current
   * session so the session list re-appears and the user can pick a different one.
   * What: Resets currentSession to null and switches mobileView to 'list'.
   * Test: While in chat view on mobile, tap back — assert mobileView === 'list'
   * and $currentSession === null.
   */
  function goBackToList() {
    currentSession.set(null);
    mobileView = 'list';
  }

</script>

<main class="app">
    <header>
      <div class="header-left">
        <!-- Mobile back button: visible only in chat view on narrow viewports -->
        {#if mobileView === 'chat'}
          <button
            class="back-btn"
            on:click={goBackToList}
            aria-label="Back to session list"
          >
            &#8592; Sessions
          </button>
        {/if}
        <img src="/ai-commander.png" alt="AI Commander" class="header-logo" />
        <h1>AI Commander</h1>
      </div>
      <div class="header-center"></div>
      {#if $serverRebuilding}
        <div class="rebuild-banner">
          <span class="rebuild-spinner">&#x27F3;</span>
          Rebuilding...
        </div>
      {/if}
      {#if newVersionAvailable}
        <button class="update-banner" on:click={() => window.location.reload()}>
          🔄 Update available — Reload
        </button>
      {/if}
      <div class="header-right">
        <button
          class="theme-btn"
          on:click={() => showSettings = true}
          title="Settings"
          aria-label="Open settings"
        >
          <Settings size={14} />
        </button>
        <button
          class="theme-btn"
          on:click={toggleTheme}
          title="Toggle theme"
          aria-label="Toggle light/dark theme"
        >
          {#if $resolvedTheme === 'dark'}
            <Sun size={14} />
          {:else}
            <Moon size={14} />
          {/if}
        </button>
      </div>
    </header>

    <div class="content" class:mobile-show-list={mobileView === 'list'} class:mobile-show-chat={mobileView === 'chat'}>
      <aside>
        <SessionList />
      </aside>
      <section class="main-panel">
        <ChatView />
        <InputArea />
      </section>
    </div>
  </main>

  {#if showSettings}
    <SettingsModal on:close={() => showSettings = false} />
  {/if}

  {#if $showCreateSessionModal}
    <CreateSessionModal
      bind:show={$showCreateSessionModal}
      on:created={async (e) => {
        $showCreateSessionModal = false;
        // Auto-connect to the newly created session so the user lands directly
        // in ChatView rather than having to click the session row manually.
        try {
          const name: string = e.detail?.name;
          if (name) {
            const result = (await invoke('connect_session', { name })) as {
              session?: string;
              history?: Array<{ text: string; ts: number; hash: string }>;
            } | null;
            // Find the session object from the store after connect (SessionList
            // will refresh on its own poll, but we need a minimal object now).
            const sessionList = get(sessions);
            const session = sessionList.find(s => s.name === name) ?? {
              name,
              created_at: new Date().toISOString(),
              is_connected: true,
              session_state: 'connected' as const,
            };
            currentSession.set({ ...session, is_connected: true });
            markSessionConnected(name);
            // Hydrate history returned by connect, if any.
            if (result?.history?.length) {
              for (const entry of result.history) {
                const ts = new Date(entry.ts * 1000);
                addMessageToSession(name, {
                  direction: 'system',
                  content: `history ${ts.toLocaleTimeString()}: ${entry.text}`,
                  timestamp: ts,
                });
              }
            }
          }
        } catch {
          // Auto-connect is best-effort — session creation already succeeded.
          // The user can manually click the row in SessionList.
        }
      }}
      on:close={() => { $showCreateSessionModal = false; }}
    />
  {/if}

<style>
  /* ── Theme CSS variables (must be global for child components) ── */
  :global(:root),
  :global([data-theme="dark"]) {
    --bg-primary: #1e1e2e;
    --bg-secondary: #181825;
    --bg-surface: #313244;
    --text-primary: #cdd6f4;
    --text-secondary: #a6adc8;
    --border: #45475a;
    --accent: #6366f1;
    --header-bg: #181825;
    --header-border: #313244;
    --color-sent: #89dceb;
    --color-system: #a6e3a1;
    --color-connecting: #89b4fa;
    --color-waiting: #f9e2af;
    --color-scroll-btn: #89b4fa;
    --color-scroll-btn-hover: #b4befe;
    --color-scroll-btn-text: #1e1e2e;
  }

  :global([data-theme="light"]) {
    --bg-primary: #ffffff;
    --bg-secondary: #f8fafc;
    --bg-surface: #f1f5f9;
    --text-primary: #1e293b;
    --text-secondary: #64748b;
    --border: #e2e8f0;
    --accent: #6366f1;
    --header-bg: #ffffff;
    --header-border: #e2e8f0;
    --color-sent: #0369a1;
    --color-system: #15803d;
    --color-connecting: #2563eb;
    --color-waiting: #b45309;
    --color-scroll-btn: #6366f1;
    --color-scroll-btn-hover: #4f46e5;
    --color-scroll-btn-text: #ffffff;
  }

  :global(body) {
    background-color: var(--bg-primary);
    color: var(--text-primary);
    margin: 0;
    padding: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  }

  /* ── App shell ── */
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    height: 100dvh;
    overflow: hidden;
    background-color: var(--bg-primary);
    color: var(--text-primary);
  }

  header {
    display: flex;
    align-items: center;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--header-border);
    background-color: var(--header-bg);
    gap: 0.75rem;
    min-height: 3rem;
    position: sticky;
    top: 0;
    z-index: 50;
    flex-shrink: 0;
  }

  .header-left {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .header-logo {
    width: 28px;
    height: 28px;
    border-radius: 6px;
  }

  .header-center {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .rebuild-banner {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    background: var(--warning-bg, rgba(245, 158, 11, 0.15));
    color: var(--warning-text, #d97706);
    padding: 0.2rem 0.75rem;
    border-radius: 0.375rem;
    font-size: 0.8rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .rebuild-spinner {
    display: inline-block;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .update-banner {
    background: var(--accent, #6366f1);
    color: white;
    border: none;
    padding: 0.25rem 0.75rem;
    border-radius: 0.375rem;
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
    animation: pulse 2s infinite;
  }

  .update-banner:hover {
    opacity: 0.9;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.7; }
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    flex-shrink: 0;
  }

  h1 {
    font-size: 1.125rem;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }

  .theme-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.3rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
    flex-shrink: 0;
  }

  .theme-btn:hover {
    background: var(--bg-surface);
    border-color: var(--text-secondary);
    color: var(--text-primary);
  }

  .content {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  aside {
    width: 250px;
    border-right: 1px solid var(--border);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .main-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
  }

  aside :global(.session-list) {
    flex: 1;
    min-height: 0;
  }

  /* Back button: hidden on desktop, shown only on mobile when in chat view */
  .back-btn {
    display: none;
    align-items: center;
    gap: 0.25rem;
    background: none;
    border: none;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    color: var(--accent);
    padding: 0.25rem 0.5rem;
    border-radius: 0.375rem;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .back-btn:hover {
    background: var(--bg-surface);
  }

  @media (max-width: 768px) {
    .back-btn {
      display: flex;
    }

    /* Master-detail: both panels fill the viewport; only one is visible at a time */
    aside {
      position: absolute;
      inset: 0;
      width: 100%;
      z-index: 10;
      background-color: var(--bg-secondary);
      display: flex;
      flex-direction: column;
    }

    .main-panel {
      position: absolute;
      inset: 0;
      width: 100%;
      z-index: 10;
    }

    /* Show list, hide chat */
    .content.mobile-show-list aside {
      display: flex;
    }

    .content.mobile-show-list .main-panel {
      display: none;
    }

    /* Show chat, hide list */
    .content.mobile-show-chat aside {
      display: none;
    }

    .content.mobile-show-chat .main-panel {
      display: flex;
    }

    .content {
      position: relative;
    }
  }
</style>
