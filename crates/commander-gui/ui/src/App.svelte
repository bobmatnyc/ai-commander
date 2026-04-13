<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import BotStatus from './lib/components/BotStatus.svelte';
  import { RotateCw, Sun, Moon } from 'lucide-svelte';
  import { resolvedTheme, setTheme } from './lib/stores/theme';
  import { currentSession } from './lib/stores/app';

  function toggleTheme() {
    setTheme($resolvedTheme === 'dark' ? 'light' : 'dark');
  }

  let rebuilding = false;
  let apiRunning = false;
  let daemonRunning = false;

  async function handleReload() {
    if (rebuilding) return;

    rebuilding = true;
    try {
      await invoke('rebuild_from_source');
      location.reload();
    } catch (_e) {
      location.reload();
    }
  }

  async function checkServices() {
    try {
      const resp = await fetch('http://localhost:8765/api/health');
      apiRunning = resp.ok;
    } catch {
      apiRunning = false;
    }

    // Daemon is always running if the Tauri app is running
    daemonRunning = true;
  }

  onMount(() => {
    function handleKeydown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === 'r') {
        event.preventDefault();
        handleReload();
      }
    }

    window.addEventListener('keydown', handleKeydown);

    checkServices();
    const svcInterval = setInterval(checkServices, 5000);

    return () => {
      window.removeEventListener('keydown', handleKeydown);
      clearInterval(svcInterval);
    };
  });
</script>

<main class="app">
  <header>
    <div class="header-left">
      <img src="/aic-logo.svg" alt="AIC" class="header-logo" />
      <h1>AI Commander</h1>
    </div>

    <div class="header-center">
      <div class="status-indicators">
        <span class="status-dot" class:active={apiRunning}>API</span>
        <span class="status-dot" class:active={daemonRunning}>Daemon</span>
        <BotStatus compact={true} />
      </div>
    </div>

    <div class="header-right">
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
      <button
        class="reload-btn"
        class:spinning={rebuilding}
        on:click={handleReload}
        disabled={rebuilding}
        title={rebuilding ? 'Building from source…' : 'Rebuild & reload (Cmd+R / Ctrl+R)'}
        aria-label={rebuilding ? 'Building from source' : 'Rebuild and reload'}
      >
        <RotateCw size={14} />
      </button>
    </div>
  </header>

  <div class="content">
    <aside>
      <SessionList />
    </aside>

    <section class="main-panel">
      <ChatView />
      <InputArea />
    </section>
  </div>
</main>

<style>
  /* ── Theme CSS variables (global so child components inherit them) ── */
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
    /* Semantic accent colors for terminal output */
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
    /* Semantic accent colors for terminal output */
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
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
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
  }

  .header-left {
    flex-shrink: 0;
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
    flex: 1;
    display: flex;
    justify-content: center;
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

  .status-indicators {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .status-dot {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.7rem;
    color: var(--text-secondary);
    padding: 0.2rem 0.5rem;
    border-radius: 9999px;
    background: var(--bg-surface);
    font-weight: 500;
    user-select: none;
  }

  .status-dot.active {
    color: #059669;
    background: rgba(5, 150, 105, 0.12);
  }

  .status-dot.active::before {
    content: '';
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #10b981;
    flex-shrink: 0;
  }

  .iterm-global-btn {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s;
    font-size: 0.75rem;
    font-weight: 500;
    flex-shrink: 0;
  }

  .iterm-global-btn:hover {
    background: var(--bg-surface);
    color: var(--accent);
    border-color: var(--accent);
  }

  .iterm-label {
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .theme-btn,
  .reload-btn {
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

  .theme-btn:hover,
  .reload-btn:hover {
    background: var(--bg-surface);
    border-color: var(--text-secondary);
    color: var(--text-primary);
  }

  .reload-btn:active {
    transform: rotate(180deg);
    background: var(--bg-surface);
  }

  .reload-btn:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .reload-btn.spinning :global(svg) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to   { transform: rotate(360deg); }
  }

  .content {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  aside {
    width: 250px;
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }

  .main-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
</style>
