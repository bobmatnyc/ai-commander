<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { sessions, currentSession, currentView, hydrateSessionMessages, hiddenSessions, hideSession, unhideAll } from '../stores/app';
  import { Terminal, MessageSquare, Plus, Zap, EyeOff } from 'lucide-svelte';
  import type { Session } from '../stores/app';
  import CreateSessionModal from './CreateSessionModal.svelte';

  let refreshInterval: number;
  let showCreateModal = false;
  let connectingSession: string | null = null;
  let openingIterm: string | null = null;

  function getDisplayName(sessionName: string): string {
    return sessionName;
  }

  function timeAgo(dateStr: string): string {
    if (!dateStr) return '';
    const date = new Date(dateStr);
    if (isNaN(date.getTime())) return '';
    const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
    if (seconds < 5) return 'just now';
    if (seconds < 60) return `${seconds}s ago`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    return `${Math.floor(seconds / 86400)}d ago`;
  }

  function sessionsEqual(a: Session[], b: Session[]): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (
        a[i].name !== b[i].name ||
        a[i].is_connected !== b[i].is_connected ||
        a[i].is_active !== b[i].is_active ||
        a[i].status_line !== b[i].status_line
      ) return false;
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

  async function openChat(session: Session) {
    if (connectingSession) return;
    connectingSession = session.name;

    try {
      await invoke('connect_session', { name: session.name });

      // Hydrate messages from localStorage before switching views
      hydrateSessionMessages(session.name);

      currentSession.set({ ...session, is_connected: true });

      // Connection state is communicated via UI affordances (green pulse dot on
      // session rows, green tinge on the chat header). No system message needed.
      currentView.set('chat');
    } catch (err) {
      console.error('Failed to connect:', err);
    } finally {
      connectingSession = null;
    }
  }

  async function openInIterm(session: Session, e: MouseEvent) {
    e.stopPropagation();
    if (openingIterm) return;
    openingIterm = session.name;
    try {
      await invoke('open_in_iterm', { session_name: session.name });
    } catch (err) {
      console.error('Failed to open in iTerm:', err);
    } finally {
      openingIterm = null;
    }
  }

  function handleSessionCreated() {
    showCreateModal = false;
    loadSessions();
  }

  $: visibleSessions = $sessions.filter(s => !$hiddenSessions.has(s.name));
  $: hiddenCount = $sessions.length - visibleSessions.length;
  $: activeCount = visibleSessions.filter(s => s.is_active).length;
  $: idleCount = visibleSessions.length - activeCount;

  function unhideAllSessions() { unhideAll(); }

  onMount(() => {
    loadSessions();
    refreshInterval = window.setInterval(loadSessions, 3000);
  });

  onDestroy(() => {
    clearInterval(refreshInterval);
  });
</script>

<div class="dashboard">
  <div class="dashboard-inner">
    <!-- Section header -->
    <div class="section-header">
      <div class="section-title-group">
        <h2 class="section-title">Sessions</h2>
        {#if $sessions.length > 0}
          <div class="session-counts">
            {#if activeCount > 0}
              <span class="count-badge active">{activeCount} active</span>
            {/if}
            {#if idleCount > 0}
              <span class="count-badge idle">{idleCount} idle</span>
            {/if}
          </div>
        {/if}
      </div>

      <button
        class="new-btn"
        on:click={() => (showCreateModal = true)}
        title="Create new session"
      >
        <Plus size={14} />
        <span>New Session</span>
      </button>
    </div>

    <!-- Cards grid -->
    {#if $sessions.length === 0}
      <div class="empty-state">
        <div class="empty-icon">
          <Zap size={32} />
        </div>
        <h3 class="empty-title">No sessions yet</h3>
        <p class="empty-desc">Create your first session to get started</p>
        <button class="empty-cta" on:click={() => (showCreateModal = true)}>
          <Plus size={16} />
          Create Session
        </button>
      </div>
    {:else}
      <div class="cards-grid">
        {#each visibleSessions as session (session.name)}
          <div
            class="session-card"
            class:is-active={session.is_active}
            class:is-selected={$currentSession?.name === session.name}
            class:is-connecting={connectingSession === session.name}
          >
            <button
              class="hide-btn"
              on:click|stopPropagation={() => hideSession(session.name)}
              title="Hide from dashboard"
            >
              <EyeOff size={12} />
            </button>

            <div class="card-header">
              <div class="card-identity">
                <span
                  class="card-dot"
                  class:dot-active={session.is_active}
                  title={session.is_active ? 'Active' : 'Idle'}
                ></span>
                <div class="card-name-group">
                  <span class="card-name">{getDisplayName(session.name)}</span>
                  {#if session.status_line}
                    <p class="card-status-preview">{session.status_line}</p>
                  {/if}
                </div>
              </div>
              <span class="card-status-text">
                {session.is_active ? 'Active' : 'Idle'}
              </span>
            </div>

            <div class="card-meta">
              <span class="card-time">{timeAgo(session.created_at)}</span>
            </div>

            <div class="card-actions">
              <button
                class="card-btn primary"
                on:click={() => openChat(session)}
                disabled={connectingSession !== null}
                title="Open chat view"
              >
                <MessageSquare size={13} />
                {connectingSession === session.name ? 'Connecting...' : 'Chat'}
              </button>
              <button
                class="card-btn secondary"
                on:click={(e) => openInIterm(session, e)}
                disabled={openingIterm !== null}
                title="Open in iTerm2"
              >
                <Terminal size={13} />
                iTerm
              </button>
            </div>
          </div>
        {/each}

        <!-- New session card -->
        <button
          class="session-card new-card"
          on:click={() => (showCreateModal = true)}
        >
          <Plus size={20} />
          <span>New Session</span>
        </button>
      </div>

      {#if hiddenCount > 0}
        <div class="hidden-notice">
          <EyeOff size={12} />
          {hiddenCount} hidden session{hiddenCount !== 1 ? 's' : ''} —
          <button class="unhide-link" on:click={unhideAllSessions}>show all</button>
        </div>
      {/if}
    {/if}
  </div>

  <CreateSessionModal
    bind:show={showCreateModal}
    on:created={handleSessionCreated}
  />
</div>

<style>
  .dashboard {
    flex: 1;
    overflow-y: auto;
    background: var(--bg);
  }

  .dashboard-inner {
    max-width: 960px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }

  .section-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1.5rem;
  }

  .section-title-group {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .section-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: -0.01em;
  }

  .session-counts {
    display: flex;
    gap: 0.375rem;
  }

  .count-badge {
    font-size: 0.7rem;
    font-weight: 500;
    padding: 0.15rem 0.5rem;
    border-radius: 9999px;
    letter-spacing: 0.01em;
  }

  .count-badge.active {
    background: color-mix(in srgb, var(--green) 12%, transparent);
    color: var(--green);
    border: 1px solid color-mix(in srgb, var(--green) 25%, transparent);
  }

  .count-badge.idle {
    background: color-mix(in srgb, var(--amber) 12%, transparent);
    color: var(--amber);
    border: 1px solid color-mix(in srgb, var(--amber) 25%, transparent);
  }

  .new-btn {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.4rem 0.875rem;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-secondary);
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .new-btn:hover {
    border-color: var(--accent);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  /* Cards grid */
  .cards-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 1rem;
  }

  /* Session card */
  .session-card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 1.125rem;
    display: flex;
    flex-direction: column;
    gap: 0.875rem;
    transition: all 0.15s;
    position: relative;
    overflow: hidden;
  }

  .session-card::before {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 1px;
    background: transparent;
    transition: background 0.15s;
  }

  .session-card.is-active::before {
    background: linear-gradient(90deg, transparent, var(--green), transparent);
    opacity: 0.6;
  }

  .session-card.is-selected {
    border-color: var(--accent);
  }

  .session-card:hover {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--surface-hover);
  }

  .session-card.is-connecting {
    opacity: 0.7;
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .card-identity {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  .card-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-secondary);
    flex-shrink: 0;
    opacity: 0.5;
    transition: all 0.2s;
  }

  .card-dot.dot-active {
    background: var(--green);
    box-shadow: 0 0 8px var(--green);
    opacity: 1;
    animation: pulse-glow 2.5s ease-in-out infinite;
  }

  @keyframes pulse-glow {
    0%, 100% { box-shadow: 0 0 8px var(--green); }
    50% { box-shadow: 0 0 14px var(--green); }
  }

  .card-name {
    font-size: 0.875rem;
    font-weight: 600;
    color: var(--text-primary);
    font-family: var(--font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-status-text {
    font-size: 0.7rem;
    color: var(--text-secondary);
    font-weight: 500;
    flex-shrink: 0;
  }

  .session-card.is-active .card-status-text {
    color: var(--green);
  }

  .card-name-group {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .card-status-preview {
    font-size: 0.7rem;
    font-family: var(--font-mono);
    color: var(--text-muted);
    margin: 0.25rem 0 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
    opacity: 0.8;
  }

  .card-meta {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .card-time {
    font-size: 0.72rem;
    color: var(--text-secondary);
  }

  .card-actions {
    display: flex;
    gap: 0.5rem;
  }

  .card-btn {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.375rem 0.75rem;
    border-radius: 5px;
    border: 1px solid var(--border);
    font-size: 0.75rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
    flex: 1;
    justify-content: center;
  }

  .card-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .card-btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  .card-btn.primary:hover:not(:disabled) {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .card-btn.secondary {
    background: transparent;
    color: var(--text-secondary);
  }

  .card-btn.secondary:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
    border-color: var(--text-secondary);
  }

  /* New session card */
  .new-card {
    background: transparent;
    border-style: dashed;
    cursor: pointer;
    justify-content: center;
    align-items: center;
    flex-direction: row;
    gap: 0.5rem;
    color: var(--text-secondary);
    font-size: 0.85rem;
    font-weight: 500;
    min-height: 120px;
  }

  .new-card:hover {
    border-color: var(--accent);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  /* Empty state */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 5rem 2rem;
    text-align: center;
    gap: 0.75rem;
  }

  .empty-icon {
    color: var(--text-secondary);
    opacity: 0.4;
    margin-bottom: 0.5rem;
  }

  .empty-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .empty-desc {
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .empty-cta {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    margin-top: 1rem;
    padding: 0.5rem 1.25rem;
    border-radius: 6px;
    border: 1px solid var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--accent);
    font-size: 0.85rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s;
  }

  .empty-cta:hover {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  /* Hide button */
  .hide-btn {
    position: absolute;
    top: 0.5rem;
    right: 0.5rem;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0.2rem;
    border-radius: 3px;
    display: flex;
    align-items: center;
    opacity: 0;
    transition: opacity 0.15s, color 0.15s;
  }

  .session-card:hover .hide-btn {
    opacity: 1;
  }

  .hide-btn:hover {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 10%, transparent);
  }

  /* Hidden sessions notice */
  .hidden-notice {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--text-muted);
    font-size: 0.75rem;
    padding: 0.5rem 0;
    margin-top: 0.5rem;
  }

  .unhide-link {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: 0.75rem;
    padding: 0;
    text-decoration: underline;
  }

  .unhide-link:hover {
    color: var(--accent-hover);
  }
</style>
