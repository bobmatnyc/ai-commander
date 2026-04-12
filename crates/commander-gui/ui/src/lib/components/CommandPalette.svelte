<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {
    sessions,
    currentSession,
    currentView,
    sessionMessages,
    addMessageToSession,
    hydrateSessionMessages,
    botRunning,
  } from '../stores/app';
  import { MessageSquare, Terminal, Zap, Power } from 'lucide-svelte';
  import type { Session } from '../stores/app';

  const dispatch = createEventDispatcher();

  let query = '';
  let inputEl: HTMLInputElement;
  let selectedIndex = 0;

  interface PaletteItem {
    id: string;
    label: string;
    description?: string;
    icon: typeof MessageSquare;
    action: () => void;
  }

  function getDisplayName(name: string) {
    return name;
  }

  async function connectAndChat(session: Session) {
    dispatch('close');
    try {
      await invoke('connect_session', { name: session.name });
      hydrateSessionMessages(session.name);
      currentSession.set({ ...session, is_connected: true });
      const existingMessages = $sessionMessages.get(session.name);
      if (!existingMessages || existingMessages.length === 0) {
        addMessageToSession(session.name, {
          direction: 'system',
          content: `Connected to session: ${getDisplayName(session.name)}`,
          timestamp: new Date(),
        });
      }
      currentView.set('chat');
    } catch (err) {
      console.error('Connect failed:', err);
    }
  }

  async function openIterm(session: Session) {
    dispatch('close');
    try {
      await invoke('open_in_iterm', { session_name: session.name });
    } catch (err) {
      console.error('iTerm open failed:', err);
    }
  }

  async function toggleBot() {
    dispatch('close');
    if ($botRunning) {
      await invoke('stop_bot');
      botRunning.set(false);
    } else {
      const info: any = await invoke('start_bot');
      botRunning.set(info.running);
    }
  }

  function goToDashboard() {
    dispatch('close');
    currentView.set('dashboard');
  }

  $: allItems = [
    ...$sessions.map((s): PaletteItem => ({
      id: `chat:${s.name}`,
      label: getDisplayName(s.name),
      description: `Open chat`,
      icon: MessageSquare,
      action: () => connectAndChat(s),
    })),
    ...$sessions.map((s): PaletteItem => ({
      id: `iterm:${s.name}`,
      label: `${getDisplayName(s.name)} in iTerm`,
      description: `Open terminal`,
      icon: Terminal,
      action: () => openIterm(s),
    })),
    {
      id: 'dashboard',
      label: 'Go to Dashboard',
      description: 'View all sessions',
      icon: Zap,
      action: goToDashboard,
    },
    {
      id: 'toggle-bot',
      label: $botRunning ? 'Stop Bot' : 'Start Bot',
      description: $botRunning ? 'Stop the Telegram bot' : 'Start the Telegram bot',
      icon: Power,
      action: toggleBot,
    },
  ];

  $: filteredItems = query.trim()
    ? allItems.filter(item =>
        item.label.toLowerCase().includes(query.toLowerCase()) ||
        (item.description ?? '').toLowerCase().includes(query.toLowerCase())
      )
    : allItems;

  $: if (filteredItems.length > 0 && selectedIndex >= filteredItems.length) {
    selectedIndex = 0;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIndex = (selectedIndex + 1) % filteredItems.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIndex = (selectedIndex - 1 + filteredItems.length) % filteredItems.length;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filteredItems[selectedIndex]) {
        filteredItems[selectedIndex].action();
      }
    } else if (e.key === 'Escape') {
      dispatch('close');
    }
  }

  onMount(() => {
    inputEl?.focus();
  });
</script>

<div
  class="palette-overlay"
  on:click={() => dispatch('close')}
  on:keydown
  role="presentation"
>
  <div
    class="palette"
    on:click|stopPropagation
    on:keydown={handleKeydown}
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
  >
    <div class="palette-input-row">
      <span class="palette-prompt">{'>'}</span>
      <input
        bind:this={inputEl}
        bind:value={query}
        type="text"
        placeholder="Search sessions or commands..."
        class="palette-input"
        autocomplete="off"
        spellcheck="false"
      />
      <kbd class="esc-hint">esc</kbd>
    </div>

    <div class="palette-results">
      {#if filteredItems.length === 0}
        <div class="palette-empty">No results for "{query}"</div>
      {:else}
        {#each filteredItems as item, i (item.id)}
          <button
            class="palette-item"
            class:selected={i === selectedIndex}
            on:click={item.action}
            on:mouseenter={() => (selectedIndex = i)}
          >
            <span class="palette-item-icon">
              <svelte:component this={item.icon} size={14} />
            </span>
            <span class="palette-item-label">{item.label}</span>
            {#if item.description}
              <span class="palette-item-desc">{item.description}</span>
            {/if}
          </button>
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .palette-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 15vh;
    z-index: 2000;
    backdrop-filter: blur(6px);
  }

  .palette {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: 90%;
    max-width: 520px;
    overflow: hidden;
    box-shadow: 0 32px 64px rgba(0, 0, 0, 0.6);
  }

  .palette-input-row {
    display: flex;
    align-items: center;
    gap: 0.625rem;
    padding: 0.875rem 1rem;
    border-bottom: 1px solid var(--border);
  }

  .palette-prompt {
    color: var(--accent);
    font-family: var(--font-mono);
    font-size: 0.95rem;
    font-weight: 700;
    flex-shrink: 0;
    user-select: none;
  }

  .palette-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 0.9rem;
    font-family: var(--font-mono);
  }

  .palette-input::placeholder {
    color: var(--text-secondary);
  }

  .esc-hint {
    font-size: 0.65rem;
    padding: 0.15rem 0.4rem;
    border-radius: 3px;
    border: 1px solid var(--border);
    color: var(--text-secondary);
    background: var(--bg);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .palette-results {
    max-height: 360px;
    overflow-y: auto;
    padding: 0.375rem;
  }

  .palette-item {
    display: flex;
    align-items: center;
    gap: 0.625rem;
    width: 100%;
    padding: 0.575rem 0.75rem;
    border-radius: 6px;
    border: none;
    background: transparent;
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
  }

  .palette-item.selected,
  .palette-item:hover {
    background: var(--surface-hover);
  }

  .palette-item.selected {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .palette-item-icon {
    color: var(--text-secondary);
    display: flex;
    flex-shrink: 0;
  }

  .palette-item.selected .palette-item-icon {
    color: var(--accent);
  }

  .palette-item-label {
    flex: 1;
    font-size: 0.85rem;
    color: var(--text-primary);
    font-family: var(--font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .palette-item-desc {
    font-size: 0.72rem;
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .palette-empty {
    padding: 2rem;
    text-align: center;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }
</style>
