<script lang="ts">
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import MonitorView from './lib/components/MonitorView.svelte';
  import SettingsModal from './lib/components/SettingsModal.svelte';
  import { Sun, Moon, Settings, Activity, MessageSquare } from 'lucide-svelte';
  import { resolvedTheme, setTheme } from './lib/stores/theme';
  import { currentSession } from './lib/stores/app';

  // No auth needed — Tailscale handles network security

  let currentView: 'chat' | 'monitor' = 'chat';
  let showSettings = false;
  let sidebarOpen = false;

  function toggleTheme() {
    setTheme($resolvedTheme === 'dark' ? 'light' : 'dark');
  }

  // Close sidebar on mobile when session selected
  $: if ($currentSession) sidebarOpen = false;

</script>

<main class="app">
    <header>
      <div class="header-left">
        <button class="hamburger-btn" on:click={() => sidebarOpen = !sidebarOpen}>
          ☰
        </button>
        <img src="/ai-commander.png" alt="AI Commander" class="header-logo" />
        <h1>AI Commander</h1>
      </div>
      <div class="header-center">
        <button
          class="tab-btn"
          class:active={currentView === 'chat'}
          on:click={() => currentView = 'chat'}
        >
          <MessageSquare size={13} />
          Chat
        </button>
        <button
          class="tab-btn"
          class:active={currentView === 'monitor'}
          on:click={() => currentView = 'monitor'}
        >
          <Activity size={13} />
          Monitor
        </button>
      </div>
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

    <div class="content">
      <aside class:open={sidebarOpen}>
        <SessionList />
      </aside>
      {#if sidebarOpen}
        <div class="sidebar-backdrop" on:click={() => sidebarOpen = false} on:keydown={() => sidebarOpen = false} role="button" tabindex="-1" aria-label="Close sidebar"></div>
      {/if}
      <section class="main-panel">
        {#if currentView === 'chat'}
          <ChatView />
          <InputArea />
        {:else if currentView === 'monitor'}
          <MonitorView />
        {/if}
      </section>
    </div>
  </main>

  {#if showSettings}
    <SettingsModal on:close={() => showSettings = false} />
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

  .header-center {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .tab-btn {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.625rem;
    border: 1px solid transparent;
    border-radius: 0.375rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 0.8rem;
    font-weight: 500;
    transition: all 0.15s;
  }

  .tab-btn:hover {
    background: var(--bg-surface);
    color: var(--text-primary);
  }

  .tab-btn.active {
    background: var(--bg-surface);
    color: var(--text-primary);
    border-color: var(--border);
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

  .hamburger-btn {
    display: none;
    background: none;
    border: none;
    font-size: 1.5rem;
    cursor: pointer;
    color: var(--text-primary);
    padding: 0.25rem 0.5rem;
  }

  .sidebar-backdrop {
    display: none;
  }

  @media (max-width: 768px) {
    .hamburger-btn {
      display: flex;
      align-items: center;
    }

    aside {
      position: fixed;
      top: 0;
      left: 0;
      bottom: 0;
      width: 250px;
      z-index: 100;
      transform: translateX(-100%);
      transition: transform 0.2s ease;
      background-color: var(--bg-secondary);
    }

    aside.open {
      transform: translateX(0);
    }

    .sidebar-backdrop {
      display: block;
      position: fixed;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      background: rgba(0, 0, 0, 0.4);
      z-index: 99;
    }

    .content {
      position: relative;
    }
  }
</style>
