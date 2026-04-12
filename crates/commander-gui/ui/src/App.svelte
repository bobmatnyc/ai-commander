<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import SessionList from './lib/components/SessionList.svelte';
  import ChatView from './lib/components/ChatView.svelte';
  import InputArea from './lib/components/InputArea.svelte';
  import BotStatus from './lib/components/BotStatus.svelte';
  import { RotateCw } from 'lucide-svelte';

  let rebuilding = false;

  async function handleReload() {
    if (rebuilding) return;

    rebuilding = true;
    try {
      await invoke('rebuild_from_source');
      // Build succeeded — reload the webview to pick up the new binary.
      location.reload();
    } catch (_e) {
      // Source not available or build failed — fall back to a plain webview reload.
      location.reload();
    }
  }

  onMount(() => {
    function handleKeydown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === 'r') {
        event.preventDefault();
        handleReload();
      }
    }

    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  });
</script>

<main class="app">
  <header>
    <h1>AI Commander</h1>
    <BotStatus />
    <button
      class="reload-btn"
      class:spinning={rebuilding}
      on:click={handleReload}
      disabled={rebuilding}
      title={rebuilding ? 'Building from source…' : 'Rebuild & reload (Cmd+R / Ctrl+R)'}
      aria-label={rebuilding ? 'Building from source' : 'Rebuild and reload'}
    >
      <RotateCw size={16} />
    </button>
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
    justify-content: space-between;
    align-items: center;
    padding: 1rem 1.5rem;
    border-bottom: 1px solid #e5e7eb;
    background-color: white;
  }

  h1 {
    font-size: 1.5rem;
    font-weight: 700;
    color: #1f2937;
  }

  .reload-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.375rem;
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
