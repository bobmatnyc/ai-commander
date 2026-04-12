<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { sessions, currentSession, sessionMessages, addMessageToSession } from '../stores/app';
  import { Activity, Plus, Terminal } from 'lucide-svelte';
  import type { Session } from '../stores/app';
  import CreateSessionModal from './CreateSessionModal.svelte';

  let interval: number;
  let showCreateModal = false;
  let lastError: string | null = null;
  let errorTimeout: number | null = null;

  function getDisplayName(sessionName: string): string {
    return sessionName;
  }

  function sessionsEqual(a: Session[], b: Session[]): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (a[i].name !== b[i].name || a[i].is_connected !== b[i].is_connected) return false;
    }
    return true;
  }

  async function loadSessions() {
    try {
      const result = await invoke('list_sessions') as Session[];
      if (!sessionsEqual(result, $sessions)) {
        sessions.set(result);
      }
    } catch (err) {
      console.error('Failed to load sessions:', err);
    }
  }

  async function connect(name: string) {
    // Clear any previous error
    lastError = null;
    if (errorTimeout) clearTimeout(errorTimeout);

    try {
      // Snapshot prior message count before we add anything.
      const priorMessages = $sessionMessages.get(name);
      const hasCachedHistory = priorMessages && priorMessages.length > 0;

      await invoke('connect_session', { name });
      const session = $sessions.find(s => s.name === name);
      if (session) {
        currentSession.set({ ...session, is_connected: true });

        // Always show a "Connected" system message so the user knows
        // which session is active, regardless of prior message history.
        addMessageToSession(name, {
          direction: 'system',
          content: `Connected to session: ${getDisplayName(name)}`,
          timestamp: new Date(),
        });

        // For sessions with no prior message history, immediately capture
        // the current terminal output so the user sees the tmux state right
        // away instead of waiting for the poll cycle (~500 ms).
        if (!hasCachedHistory) {
          try {
            const output = await invoke<string>('capture_session_output', { name });
            if (output && output.trim().length > 0) {
              addMessageToSession(name, {
                direction: 'received',
                content: output,
                timestamp: new Date(),
              });
            }
          } catch (_captureErr) {
            // Non-fatal: polling will deliver output within 500 ms.
          }
        }
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);

      // Show error banner
      lastError = `Cannot connect to ${getDisplayName(name)}: ${errorMessage}`;

      // Auto-clear after 5 seconds
      errorTimeout = setTimeout(() => {
        lastError = null;
      }, 5000);

      // Also add to messages
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

  async function openInIterm(sessionName: string) {
    await invoke('open_in_iterm', { sessionName });
  }

  function handleSessionCreated() {
    showCreateModal = false;
    loadSessions();
  }

  onMount(() => {
    loadSessions();
    interval = window.setInterval(loadSessions, 2000);
  });

  onDestroy(() => {
    clearInterval(interval);
  });
</script>

<div class="session-list">
  <div class="session-list-header">
    <h2 class="header-title">Sessions</h2>
    <button class="create-btn" on:click={() => showCreateModal = true} title="Create new session">
      <Plus size={16} />
      <span>New</span>
    </button>
  </div>
  {#if lastError}
    <div class="error-banner">
      ⚠️ {lastError}
    </div>
  {/if}
  <div class="session-items">
    {#each $sessions as session}
      <div
        class="session-item"
        class:active={$currentSession?.name === session.name}
      >
        <button class="session-main" on:click={() => connect(session.name)}>
          <span class="session-name">{getDisplayName(session.name)}</span>
          <Activity
            size={16}
            class={session.is_connected ? 'text-green-500' : 'text-gray-400'}
          />
        </button>
        <button
          class="iterm-btn"
          on:click|stopPropagation={() => openInIterm(session.name)}
          title="Open in iTerm2"
        >
          <Terminal size={14} />
        </button>
      </div>
    {:else}
      <div class="no-sessions">
        <p class="text-gray-500 text-sm">No sessions available</p>
      </div>
    {/each}
  </div>

  <CreateSessionModal
    bind:show={showCreateModal}
    on:created={handleSessionCreated}
  />
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

  .session-main {
    display: flex;
    flex: 1;
    justify-content: space-between;
    align-items: center;
    padding: 0.75rem 1rem;
    border: none;
    background: transparent;
    cursor: pointer;
    text-align: left;
    min-width: 0;
  }

  .session-name {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .iterm-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.375rem;
    margin-right: 0.375rem;
    border: 1px solid var(--border);
    border-radius: 0.25rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s, background 0.15s, color 0.15s;
    flex-shrink: 0;
  }

  .session-item:hover .iterm-btn {
    opacity: 1;
  }

  .iterm-btn:hover {
    background: var(--bg-surface);
    color: var(--accent);
    border-color: var(--accent);
  }

  .no-sessions {
    padding: 2rem 1rem;
    text-align: center;
    color: var(--text-secondary);
  }
</style>
