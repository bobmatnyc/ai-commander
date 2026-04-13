<script lang="ts">
  import { onMount } from 'svelte';
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import { Sun, Moon } from 'lucide-svelte';
  import { resolvedTheme, setTheme } from './lib/stores/theme';

  let authenticated = false;
  let pairCode = '';
  let pairing = false;

  function toggleTheme() {
    setTheme($resolvedTheme === 'dark' ? 'light' : 'dark');
  }

  async function checkAuth() {
    try {
      const status = await fetch('/api/auth/status', {
        headers: { Authorization: `Bearer ${localStorage.getItem('aic-auth-token') || ''}` }
      });
      if (status.ok) {
        const data = await status.json();
        authenticated = data.authenticated;
      }
    } catch {
      // No auth required if endpoint doesn't exist (Tailscale-only mode)
      authenticated = true;
    }
  }

  async function pair() {
    pairing = true;
    try {
      const resp = await fetch('/api/auth/pair', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ code: pairCode }),
      });
      if (resp.ok) {
        const data = await resp.json();
        localStorage.setItem('aic-auth-token', data.token);
        authenticated = true;
      }
    } catch (e) {
      console.error('Pairing failed:', e);
    } finally {
      pairing = false;
    }
  }

  onMount(() => {
    checkAuth();
  });
</script>

{#if !authenticated}
  <main class="auth-screen">
    <img src="/ai-commander.png" alt="AI Commander" class="auth-logo" />
    <h1>AI Commander</h1>
    <p>Enter pairing code from the server</p>
    <input bind:value={pairCode} placeholder="ABC123" maxlength="6" class="pair-input" />
    <button on:click={pair} disabled={pairing || pairCode.length < 6} class="pair-btn">
      {pairing ? 'Pairing...' : 'Connect'}
    </button>
  </main>
{:else}
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
{/if}

<style>
  /* ── Auth screen ── */
  .auth-screen {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background-color: var(--bg-primary);
    color: var(--text-primary);
    gap: 1rem;
  }

  .auth-logo {
    width: 64px;
    height: 64px;
    border-radius: 12px;
  }

  .auth-screen h1 {
    font-size: 1.75rem;
    font-weight: 700;
    margin: 0;
  }

  .auth-screen p {
    color: var(--text-secondary);
    margin: 0;
  }

  .pair-input {
    padding: 0.625rem 0.875rem;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    color: var(--text-primary);
    font-size: 1.25rem;
    font-weight: 600;
    letter-spacing: 0.2em;
    text-align: center;
    text-transform: uppercase;
    width: 10rem;
    outline: none;
    transition: border-color 0.2s;
  }

  .pair-input:focus {
    border-color: var(--accent);
  }

  .pair-btn {
    padding: 0.625rem 1.5rem;
    background: var(--accent);
    color: #ffffff;
    border: none;
    border-radius: 0.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    transition: opacity 0.2s;
  }

  .pair-btn:hover:not(:disabled) {
    opacity: 0.85;
  }

  .pair-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  /* ── App shell (mirrors App.svelte) ── */
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
