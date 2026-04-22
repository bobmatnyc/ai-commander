<script lang="ts">
  /**
   * Why: The standalone Monitor tab hid useful process info behind an extra
   *      click. Bug 2 asks us to surface processes inline in the session list
   *      so users can spot (and optionally kill) stale children without
   *      context-switching. Active/running processes render as read-only so a
   *      user never accidentally kills something they still care about.
   * What: Collapsible section rendered at the bottom of SessionList. Shows a
   *      one-line header with a live count; expands to a compact list. Auto
   *      refreshes every 30s (or on demand via the refresh button).
   * Test: Mount the component, expand the panel, assert a network call to
   *      `list_processes` fires and a row is rendered per process. Click
   *      "Kill" on a stale row, assert `kill_stale_processes` is invoked and
   *      the list refreshes.
   */
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '../transport';
  import { ChevronRight, ChevronDown, Trash2, RefreshCw, Activity } from 'lucide-svelte';

  interface ProcessInfo {
    pid: number;
    name: string;
    cpu: number;
    memory_mb: number;
    session: string | null;
    age_seconds: number;
    stale: boolean;
  }

  /** Expanded by default? Keep collapsed to reduce visual noise on load. */
  export let defaultExpanded = false;
  /** Refresh interval in ms (30s matches the task spec). */
  export let refreshMs = 30000;

  let expanded = defaultExpanded;
  let processes: ProcessInfo[] = [];
  let loading = false;
  let lastRefresh: Date | null = null;
  let interval: ReturnType<typeof setInterval> | null = null;
  let isKilling = false;
  let killMessage: string | null = null;
  let killMessageTimeout: ReturnType<typeof setTimeout> | null = null;
  let destroyed = false;
  /** URGENT safety: require user confirmation before sending any kill. */
  let confirmDialogOpen = false;
  /** Snapshot of stale processes shown inside the confirm dialog. */
  let confirmTargets: ProcessInfo[] = [];

  async function refreshProcesses() {
    if (loading || destroyed) return;
    loading = true;
    try {
      const result = await invoke('list_processes');
      if (destroyed) return;
      processes = (Array.isArray(result) ? result : []).map((p: any) => ({
        pid: p.pid ?? 0,
        name: p.name ?? '',
        cpu: p.cpu ?? 0,
        memory_mb: p.memory_mb ?? 0,
        session: p.session ?? null,
        age_seconds: p.age_seconds ?? 0,
        stale: p.stale ?? false,
      }));
      lastRefresh = new Date();
    } catch (err) {
      console.error('Failed to list processes:', err);
      if (!destroyed) processes = [];
    } finally {
      if (!destroyed) loading = false;
    }
  }

  /**
   * Open the confirmation dialog showing exactly what would be killed.
   *
   * Why: URGENT — this button was previously a one-click SIGTERM trigger.
   * Combined with a classifier bug, clicking it killed active claude-mpm
   * sessions. The user must now see the full list and explicitly confirm.
   */
  function requestKillAllStale() {
    if (isKilling) return;
    confirmTargets = processes.filter(p => p.stale);
    confirmDialogOpen = true;
  }

  function cancelKill() {
    confirmDialogOpen = false;
    confirmTargets = [];
  }

  /**
   * Actually invoke kill_stale_processes with confirm=true after the user
   * clicks "Confirm kill" in the dialog. The backend enforces its own
   * "is this really stale?" gate (tmux has-session + connected + protected
   * allowlist), so this is a user-intent trigger, not the source of truth.
   */
  async function confirmKillAllStale() {
    if (isKilling) return;
    isKilling = true;
    killMessage = null;
    confirmDialogOpen = false;
    try {
      // URGENT: always pass confirm=true here. Backend defaults to dry-run.
      const result = await invoke('kill_stale_processes', { confirm: true });
      if (destroyed) return;
      // Tauri returns a number; REST returns { count, killed, message }.
      const count = typeof result === 'number'
        ? result
        : (result as any)?.count ?? 0;
      killMessage = count > 0
        ? `Killed ${count} stale process${count === 1 ? '' : 'es'}`
        : 'No stale processes';
      await refreshProcesses();
      killMessageTimeout = setTimeout(() => { killMessage = null; }, 5000);
    } catch (err) {
      console.error('Failed to kill stale processes:', err);
      if (!destroyed) {
        killMessage = 'Failed to clean processes';
        killMessageTimeout = setTimeout(() => { killMessage = null; }, 5000);
      }
    } finally {
      if (!destroyed) isKilling = false;
      confirmTargets = [];
    }
  }

  /** Kill a single process — opens the same confirm dialog for consistency. */
  async function killOne(_pid: number, _name: string) {
    // Per-row kill uses the same confirm flow so the user always sees the
    // full list of what would be killed.
    requestKillAllStale();
  }

  function formatAge(seconds: number): string {
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    return `${Math.floor(seconds / 3600)}h${Math.floor((seconds % 3600) / 60)}m`;
  }

  function formatMemory(mb: number): string {
    if (mb < 1024) return `${mb.toFixed(0)}M`;
    return `${(mb / 1024).toFixed(1)}G`;
  }

  /** Derived: count of stale processes for the header badge. */
  $: staleCount = processes.filter(p => p.stale).length;
  $: totalCount = processes.length;

  function toggle() {
    expanded = !expanded;
    if (expanded) refreshProcesses();
  }

  onMount(() => {
    if (expanded) refreshProcesses();
    // Poll regardless of expanded state so the header badge stays accurate
    // (otherwise stale counts would lag until the user opens the panel).
    refreshProcesses();
    interval = setInterval(refreshProcesses, refreshMs);
  });

  onDestroy(() => {
    destroyed = true;
    if (interval) clearInterval(interval);
    if (killMessageTimeout) clearTimeout(killMessageTimeout);
  });
</script>

<div class="process-panel" class:expanded>
  <button
    class="panel-header"
    on:click={toggle}
    aria-expanded={expanded}
    title={expanded ? 'Collapse process monitor' : 'Expand process monitor'}
  >
    <span class="chev">
      {#if expanded}
        <ChevronDown size={12} />
      {:else}
        <ChevronRight size={12} />
      {/if}
    </span>
    <Activity size={12} />
    <span class="panel-title">Processes</span>
    <span class="count-badges">
      {#if staleCount > 0}
        <span class="badge-stale" title="{staleCount} stale">{staleCount} stale</span>
      {/if}
      <span class="badge-total" title="{totalCount} tracked">{totalCount}</span>
    </span>
  </button>

  {#if expanded}
    <div class="panel-actions">
      <button
        class="mini-btn"
        class:spinning={loading}
        on:click|stopPropagation={refreshProcesses}
        disabled={loading}
        title="Refresh"
        aria-label="Refresh process list"
      >
        <RefreshCw size={11} />
      </button>
      {#if staleCount > 0}
        <button
          class="mini-btn danger"
          on:click|stopPropagation={requestKillAllStale}
          disabled={isKilling}
          title="Review and kill stale processes (requires confirmation)"
        >
          <Trash2 size={11} />
          <span>{isKilling ? 'Cleaning…' : `Clean ${staleCount}…`}</span>
        </button>
      {/if}
      {#if lastRefresh}
        <span class="refresh-ts">
          {lastRefresh.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}
        </span>
      {/if}
    </div>

    {#if killMessage}
      <div class="kill-note">{killMessage}</div>
    {/if}

    {#if confirmDialogOpen}
      <div class="confirm-overlay" role="dialog" aria-modal="true" aria-label="Confirm kill stale processes">
        <div class="confirm-panel">
          <div class="confirm-title">Kill stale processes?</div>
          <div class="confirm-warn">
            This will send SIGTERM to the processes listed below. Active claude / claude-mpm
            sessions are protected and will not be killed.
          </div>
          {#if confirmTargets.length === 0}
            <div class="empty">Nothing to kill.</div>
          {:else}
            <ul class="confirm-list">
              {#each confirmTargets as proc (proc.pid)}
                <li class="confirm-row">
                  <span class="confirm-pid">{proc.pid}</span>
                  <span class="confirm-name" title={proc.name}>{proc.name}</span>
                  {#if proc.session}
                    <span class="confirm-session">{proc.session}</span>
                  {:else}
                    <span class="proc-orphan">orphan</span>
                  {/if}
                  <span class="proc-age">{formatAge(proc.age_seconds)}</span>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="confirm-actions">
            <button class="mini-btn" on:click|stopPropagation={cancelKill}>Cancel</button>
            <button
              class="mini-btn danger"
              on:click|stopPropagation={confirmKillAllStale}
              disabled={isKilling || confirmTargets.length === 0}
            >
              <Trash2 size={11} />
              <span>Confirm kill</span>
            </button>
          </div>
        </div>
      </div>
    {/if}

    {#if processes.length === 0}
      <div class="empty">No commander processes running</div>
    {:else}
      <ul class="proc-list">
        {#each processes as proc (proc.pid)}
          <li class="proc-row" class:stale={proc.stale} title={proc.name}>
            <div class="proc-line">
              <span class="proc-status" class:stale={proc.stale}>
                {#if proc.stale}stale{:else}active{/if}
              </span>
              <span class="proc-name-compact">{proc.name}</span>
            </div>
            <div class="proc-detail">
              <span class="pid" title="PID">{proc.pid}</span>
              {#if proc.session}
                <span class="proc-session" title="tmux session">{proc.session}</span>
              {:else}
                <span class="proc-orphan">orphan</span>
              {/if}
              <span class="proc-age" title="age">{formatAge(proc.age_seconds)}</span>
              <span class="proc-mem" title="memory">{formatMemory(proc.memory_mb)}</span>
              {#if proc.stale}
                <button
                  class="kill-btn"
                  on:click|stopPropagation={() => killOne(proc.pid, proc.name)}
                  disabled={isKilling}
                  title="Kill stale process"
                  aria-label="Kill stale process {proc.pid}"
                >
                  <Trash2 size={10} />
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>

<style>
  .process-panel {
    border-top: 1px solid var(--border);
    background: var(--bg-primary);
    flex-shrink: 0;
    font-size: 0.75rem;
  }

  .panel-header {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.5rem 0.75rem;
    border: none;
    background: transparent;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
  }

  .panel-header:hover {
    background: var(--bg-surface);
  }

  .chev {
    display: inline-flex;
    align-items: center;
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .panel-title {
    font-weight: 600;
    font-size: 0.75rem;
    flex: 1;
  }

  .count-badges {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    flex-shrink: 0;
  }

  .badge-stale {
    font-size: 0.65rem;
    font-weight: 600;
    color: #b45309;
    background: rgba(245, 158, 11, 0.15);
    padding: 0.1rem 0.35rem;
    border-radius: 9999px;
    letter-spacing: 0.02em;
  }

  .badge-total {
    font-size: 0.65rem;
    color: var(--text-secondary);
    background: var(--bg-surface);
    padding: 0.1rem 0.35rem;
    border-radius: 9999px;
    font-family: monospace;
  }

  .panel-actions {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.35rem 0.75rem;
    border-top: 1px solid var(--border);
    background: var(--bg-secondary);
  }

  .mini-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: 0.25rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 0.7rem;
    font-weight: 500;
    transition: all 0.15s;
  }

  .mini-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--text-secondary);
    background: var(--bg-surface);
  }

  .mini-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .mini-btn.danger:hover:not(:disabled) {
    color: #dc2626;
    border-color: #dc2626;
    background: rgba(220, 38, 38, 0.08);
  }

  .mini-btn.spinning :global(svg) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin { to { transform: rotate(360deg); } }

  .refresh-ts {
    margin-left: auto;
    font-size: 0.65rem;
    color: var(--text-secondary);
    font-family: monospace;
  }

  .kill-note {
    padding: 0.35rem 0.75rem;
    font-size: 0.7rem;
    background: rgba(34, 197, 94, 0.08);
    color: #16a34a;
    border-top: 1px solid rgba(34, 197, 94, 0.15);
  }

  .empty {
    padding: 0.75rem;
    color: var(--text-secondary);
    text-align: center;
    font-size: 0.7rem;
    font-style: italic;
  }

  .proc-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 35vh;
    overflow-y: auto;
  }

  .proc-row {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding: 0.35rem 0.75rem;
    border-top: 1px solid var(--border);
    transition: background 0.1s;
  }

  .proc-row:hover {
    background: var(--bg-surface);
  }

  .proc-row.stale {
    background: rgba(245, 158, 11, 0.06);
    border-left: 2px solid #f59e0b;
  }

  .proc-line {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    min-width: 0;
  }

  .proc-status {
    font-size: 0.6rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.02em;
    padding: 0.08rem 0.3rem;
    border-radius: 0.2rem;
    background: rgba(34, 197, 94, 0.1);
    color: #16a34a;
    flex-shrink: 0;
  }

  .proc-status.stale {
    background: rgba(245, 158, 11, 0.15);
    color: #b45309;
  }

  .proc-name-compact {
    font-size: 0.7rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: monospace;
    flex: 1;
  }

  .proc-detail {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.65rem;
    color: var(--text-secondary);
    font-family: monospace;
  }

  .pid { color: var(--text-secondary); }

  .proc-session {
    color: var(--accent);
    background: rgba(99, 102, 241, 0.1);
    padding: 0.05rem 0.3rem;
    border-radius: 0.2rem;
  }

  .proc-orphan {
    font-style: italic;
    opacity: 0.7;
  }

  .proc-age, .proc-mem {
    color: var(--text-secondary);
  }

  .kill-btn {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border: 1px solid var(--border);
    border-radius: 0.2rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.1s;
    flex-shrink: 0;
  }

  .kill-btn:hover:not(:disabled) {
    color: white;
    background: #dc2626;
    border-color: #dc2626;
  }

  .kill-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  /* Confirmation dialog */
  .confirm-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9999;
  }

  .confirm-panel {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    max-width: 520px;
    width: 90%;
    max-height: 80vh;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    box-shadow: 0 10px 25px rgba(0, 0, 0, 0.3);
  }

  .confirm-title {
    font-weight: 600;
    font-size: 0.95rem;
    color: var(--text-primary);
  }

  .confirm-warn {
    font-size: 0.75rem;
    color: #b45309;
    background: rgba(245, 158, 11, 0.1);
    padding: 0.5rem;
    border-radius: 0.25rem;
    line-height: 1.4;
  }

  .confirm-list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    max-height: 40vh;
    border: 1px solid var(--border);
    border-radius: 0.25rem;
  }

  .confirm-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.35rem 0.5rem;
    font-size: 0.72rem;
    font-family: monospace;
    border-bottom: 1px solid var(--border);
  }

  .confirm-row:last-child { border-bottom: none; }

  .confirm-pid {
    color: var(--text-secondary);
    min-width: 48px;
  }

  .confirm-name {
    flex: 1;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .confirm-session {
    color: var(--accent);
    background: rgba(99, 102, 241, 0.1);
    padding: 0.05rem 0.3rem;
    border-radius: 0.2rem;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
