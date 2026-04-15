<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { sessions, currentSession, sessionMessages, addMessageToSession, activeSessions } from '../stores/app';
  import { Activity, Plus, Terminal, Pencil, Settings, Square, Monitor } from 'lucide-svelte';
  import type { Session } from '../stores/app';
  import CreateSessionModal from './CreateSessionModal.svelte';

  let interval: number;
  let showCreateModal = false;
  let lastError: string | null = null;
  let errorTimeout: number | null = null;

  // Detect iOS/iPadOS — hide iTerm/Terminal buttons on these platforms
  const isIOS = typeof navigator !== 'undefined' && (
    /iPad|iPhone|iPod/.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  );

  // Rename state
  let renamingSession: string | null = null;
  let renameValue = '';
  let renameInput: HTMLInputElement | null = null;

  // Dropdown state: tracks which session's gear menu is open
  let openDropdown: string | null = null;

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
    lastError = null;
    if (errorTimeout) clearTimeout(errorTimeout);

    try {
      const priorMessages = $sessionMessages.get(name);
      const hasCachedHistory = priorMessages && priorMessages.length > 0;

      await invoke('connect_session', { name });
      const session = $sessions.find(s => s.name === name);
      if (session) {
        currentSession.set({ ...session, is_connected: true });

        addMessageToSession(name, {
          direction: 'system',
          content: `Connected to session: ${getDisplayName(name)}`,
          timestamp: new Date(),
        });

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
      lastError = `Cannot connect to ${getDisplayName(name)}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);

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
    closeDropdown();
    await invoke('open_in_iterm', { sessionName });
  }

  async function openInTerminal(sessionName: string) {
    closeDropdown();
    await invoke('open_in_terminal_app', { sessionName });
  }

  async function stopSession(sessionName: string) {
    closeDropdown();
    try {
      await invoke('stop_session', { name: sessionName });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to stop ${sessionName}: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  function startRename(sessionName: string) {
    closeDropdown();
    renamingSession = sessionName;
    renameValue = sessionName;
    // Focus the input on next tick
    setTimeout(() => renameInput?.focus(), 0);
  }

  async function commitRename() {
    if (!renamingSession) return;
    const oldName = renamingSession;
    const newName = renameValue.trim();

    renamingSession = null;

    if (!newName || newName === oldName) return;

    try {
      await invoke('rename_session', { oldName, newName });
      await loadSessions();
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      lastError = `Failed to rename: ${errorMessage}`;
      errorTimeout = setTimeout(() => { lastError = null; }, 5000);
    }
  }

  function cancelRename() {
    renamingSession = null;
    renameValue = '';
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelRename();
    }
  }

  function toggleDropdown(sessionName: string, e: MouseEvent) {
    e.stopPropagation();
    openDropdown = openDropdown === sessionName ? null : sessionName;
  }

  function closeDropdown() {
    openDropdown = null;
  }

  function handleGlobalClick() {
    closeDropdown();
  }

  function handleSessionCreated() {
    showCreateModal = false;
    loadSessions();
  }

  onMount(() => {
    loadSessions();
    interval = window.setInterval(loadSessions, 2000);
    window.addEventListener('click', handleGlobalClick);
  });

  onDestroy(() => {
    clearInterval(interval);
    window.removeEventListener('click', handleGlobalClick);
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
      {lastError}
    </div>
  {/if}

  <div class="session-items">
    {#each $sessions as session}
      <div
        class="session-item"
        class:active={$currentSession?.name === session.name}
      >
        {#if renamingSession === session.name}
          <!-- Inline rename editor -->
          <div class="rename-row">
            <input
              bind:this={renameInput}
              bind:value={renameValue}
              class="rename-input"
              on:keydown={handleRenameKeydown}
              on:blur={commitRename}
              spellcheck="false"
            />
          </div>
        {:else}
          <!-- Normal session row -->
          <button class="session-main" on:click={() => connect(session.name)}>
            <span class="status-dot" class:active={$activeSessions.has(session.name)}></span>
            <span class="session-name">{getDisplayName(session.name)}</span>
            <Activity
              size={16}
              class={session.is_connected ? 'text-green-500' : 'text-gray-400'}
            />
          </button>

          <!-- Action buttons: always visible -->
          <div class="session-actions">
            <!-- iTerm2 button - hidden on iOS/iPadOS -->
            {#if !isIOS}
              <button
                class="action-btn iterm-btn"
                on:click|stopPropagation={() => openInIterm(session.name)}
                title="Open in iTerm2"
              >
                <Terminal size={14} />
              </button>
            {/if}

            <!-- Rename button -->
            <button
              class="action-btn rename-btn"
              on:click|stopPropagation={() => startRename(session.name)}
              title="Rename session"
            >
              <Pencil size={13} />
            </button>

            <!-- Gear dropdown button -->
            <div class="dropdown-wrapper">
              <button
                class="action-btn gear-btn"
                class:gear-open={openDropdown === session.name}
                on:click={(e) => toggleDropdown(session.name, e)}
                title="Session options"
              >
                <Settings size={13} />
              </button>

              {#if openDropdown === session.name}
                <div class="dropdown-menu" on:click|stopPropagation>
                  <button class="dropdown-item" on:click={() => startRename(session.name)}>
                    <Pencil size={13} />
                    <span>Rename</span>
                  </button>
                  {#if !isIOS}
                    <button class="dropdown-item" on:click={() => openInIterm(session.name)}>
                      <Terminal size={13} />
                      <span>Open in iTerm2</span>
                    </button>
                    <button class="dropdown-item" on:click={() => openInTerminal(session.name)}>
                      <Monitor size={13} />
                      <span>Open in Terminal.app</span>
                    </button>
                  {/if}
                  <div class="dropdown-divider"></div>
                  <button class="dropdown-item danger" on:click={() => stopSession(session.name)}>
                    <Square size={13} />
                    <span>Stop Session</span>
                  </button>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <div class="no-sessions">
        <p>No sessions available</p>
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
    gap: 0.5rem;
    padding: 0.625rem 0.75rem;
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

  /* Action buttons row - always visible */
  .session-actions {
    display: flex;
    align-items: center;
    gap: 0.125rem;
    padding-right: 0.375rem;
    flex-shrink: 0;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: 1px solid var(--border);
    border-radius: 0.25rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
    flex-shrink: 0;
  }

  .action-btn:hover {
    background: var(--bg-surface);
    color: var(--text-primary);
    border-color: var(--text-secondary);
  }

  .iterm-btn:hover {
    color: var(--accent);
    border-color: var(--accent);
  }

  .rename-btn:hover {
    color: #f59e0b;
    border-color: #f59e0b;
  }

  .gear-btn:hover,
  .gear-btn.gear-open {
    color: var(--text-primary);
    border-color: var(--text-secondary);
    background: var(--bg-surface);
  }

  /* Rename inline input */
  .rename-row {
    flex: 1;
    padding: 0.375rem 0.5rem;
  }

  .rename-input {
    width: 100%;
    padding: 0.25rem 0.5rem;
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--text-primary);
    background: var(--bg-primary);
    border: 1px solid var(--accent);
    border-radius: 0.25rem;
    outline: none;
    box-sizing: border-box;
  }

  .rename-input:focus {
    box-shadow: 0 0 0 2px rgba(99, 102, 241, 0.3);
  }

  /* Dropdown */
  .dropdown-wrapper {
    position: relative;
  }

  .dropdown-menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 100;
    min-width: 180px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    padding: 0.25rem;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.45rem 0.625rem;
    border: none;
    border-radius: 0.25rem;
    background: transparent;
    color: var(--text-primary);
    font-size: 0.8125rem;
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
  }

  .dropdown-item:hover {
    background: var(--bg-surface);
  }

  .dropdown-item.danger {
    color: #dc2626;
  }

  .dropdown-item.danger:hover {
    background: rgba(220, 38, 38, 0.1);
  }

  .dropdown-divider {
    height: 1px;
    background: var(--border);
    margin: 0.25rem 0;
  }

  .no-sessions {
    padding: 2rem 1rem;
    text-align: center;
    color: var(--text-secondary);
    font-size: 0.875rem;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-secondary, #999);
    flex-shrink: 0;
  }

  .status-dot.active {
    background: #22c55e;
    animation: pulse-dot 1.5s ease-in-out infinite;
  }

  @keyframes pulse-dot {
    0%, 100% { opacity: 1; box-shadow: 0 0 0 0 rgba(34, 197, 94, 0.4); }
    50% { opacity: 0.8; box-shadow: 0 0 0 4px rgba(34, 197, 94, 0); }
  }
</style>
