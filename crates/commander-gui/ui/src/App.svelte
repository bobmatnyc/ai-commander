<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import BotStatus from './lib/components/BotStatus.svelte';
  import { RotateCw } from 'lucide-svelte';

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
      <h1>AIC</h1>
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
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background-color: white;
  }

  header {
    display: flex;
    align-items: center;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid #e5e7eb;
    background-color: white;
    gap: 0.75rem;
    min-height: 3rem;
  }

  .header-left {
    flex-shrink: 0;
  }

  .header-center {
    flex: 1;
    display: flex;
    justify-content: center;
  }

  .header-right {
    flex-shrink: 0;
  }

  h1 {
    font-size: 1.125rem;
    font-weight: 700;
    color: #1f2937;
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
    color: #6b7280;
    padding: 0.2rem 0.5rem;
    border-radius: 9999px;
    background: #f3f4f6;
    font-weight: 500;
    user-select: none;
  }

  .status-dot.active {
    color: #059669;
    background: #ecfdf5;
  }

  .status-dot.active::before {
    content: '';
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #10b981;
    flex-shrink: 0;
  }

  .reload-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.3rem;
    border: 1px solid #e5e7eb;
    border-radius: 0.375rem;
    background: transparent;
    color: #6b7280;
    cursor: pointer;
    transition: all 0.2s;
    flex-shrink: 0;
  }

  .reload-btn:hover {
    background: #f3f4f6;
    border-color: #9ca3af;
    color: #374151;
  }

  .reload-btn:active {
    transform: rotate(180deg);
    background: #e5e7eb;
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
    border-right: 1px solid #e5e7eb;
    overflow-y: auto;
  }

  .main-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
</style>
