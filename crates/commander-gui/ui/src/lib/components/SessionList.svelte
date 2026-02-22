<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { sessions, currentSession } from '../stores/app';
  import { Activity } from 'lucide-svelte';
  import type { Session } from '../stores/app';

  let interval: number;

  function getDisplayName(sessionName: string): string {
    return sessionName.replace(/^commander-/, '');
  }

  async function loadSessions() {
    try {
      const result = await invoke('list_sessions');
      sessions.set(result as Session[]);
    } catch (err) {
      console.error('Failed to load sessions:', err);
    }
  }

  async function connect(name: string) {
    try {
      await invoke('connect_session', { name });
      const session = $sessions.find(s => s.name === name);
      if (session) {
        currentSession.set({ ...session, is_connected: true });
      }
    } catch (err) {
      alert(`Failed to connect: ${err}`);
    }
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
  <h2 class="text-lg font-semibold px-4 py-3 border-b border-gray-200">Sessions</h2>
  <div class="session-items">
    {#each $sessions as session}
      <button
        class="session-item"
        class:active={$currentSession?.name === session.name}
        on:click={() => connect(session.name)}
      >
        <span class="session-name">{getDisplayName(session.name)}</span>
        <Activity
          size={16}
          class={session.is_connected ? 'text-green-500' : 'text-gray-400'}
        />
      </button>
    {:else}
      <div class="no-sessions">
        <p class="text-gray-500 text-sm">No sessions available</p>
      </div>
    {/each}
  </div>
</div>

<style>
  .session-list {
    display: flex;
    flex-direction: column;
    height: 100%;
    background-color: #fafafa;
  }

  .session-items {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem;
  }

  .session-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    padding: 0.75rem 1rem;
    margin-bottom: 0.5rem;
    border: none;
    border-radius: 0.5rem;
    background-color: white;
    cursor: pointer;
    transition: all 0.2s;
    text-align: left;
  }

  .session-item:hover {
    background-color: #f3f4f6;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  }

  .session-item.active {
    background-color: #dbeafe;
    border: 1px solid #3b82f6;
  }

  .session-name {
    font-size: 0.875rem;
    font-weight: 500;
    color: #1f2937;
  }

  .no-sessions {
    padding: 2rem 1rem;
    text-align: center;
  }
</style>
