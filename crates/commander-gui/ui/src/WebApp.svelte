<script lang="ts">
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import { Sun, Moon } from 'lucide-svelte';
  import { resolvedTheme, setTheme } from './lib/stores/theme';

  // No auth needed — Tailscale handles network security

  function toggleTheme() {
    setTheme($resolvedTheme === 'dark' ? 'light' : 'dark');
  }

</script>

<main class="app">
    <header>
      <div class="header-left">
        <img src="/ai-commander.png" alt="AI Commander" class="header-logo" />
        <h1>AI Commander</h1>
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
  }

  aside :global(.session-list) {
    flex: 1;
    min-height: 0;
  }
</style>
