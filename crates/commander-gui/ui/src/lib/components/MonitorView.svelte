<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '../transport';
  import { Trash2, RefreshCw, Activity } from 'lucide-svelte';

  interface ProcessInfo {
    pid: number;
    name: string;
    cpu: number;
    memory_mb: number;
    session: string | null;
    age_seconds: number;
    stale: boolean;
  }

  let processes: ProcessInfo[] = [];
  let loading = false;
  let lastRefresh: Date | null = null;
  let error: string | null = null;
  let interval: number;
  let isKilling = false;
  let killResult: { killed: Array<{pid: number, name: string}>, count: number, message: string } | null = null;
  let killResultTimeout: ReturnType<typeof setTimeout> | null = null;
  let destroyed = false;

  async function refreshProcesses() {
    if (loading || destroyed) return;
    loading = true;
    error = null;
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
      if (!destroyed) {
        processes = [];
        lastRefresh = new Date();
      }
    } finally {
      if (!destroyed) loading = false;
    }
  }

  async function killStale() {
    isKilling = true;
    killResult = null;
    try {
      const result = await invoke('kill_stale_processes');
      if (destroyed) return;
      killResult = result as any ?? { killed: [], count: 0, message: 'No response' };
      await refreshProcesses();
      // Auto-dismiss result after 5 seconds
      killResultTimeout = setTimeout(() => { killResult = null; }, 5000);
    } catch (err) {
      console.error('Failed to kill stale processes:', err);
      if (!destroyed) {
        killResult = { killed: [], count: 0, message: 'Failed to clean processes' };
        killResultTimeout = setTimeout(() => { killResult = null; }, 5000);
      }
    } finally {
      if (!destroyed) isKilling = false;
    }
  }

  function formatAge(seconds: number): string {
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  }

  function formatMemory(mb: number): string {
    if (mb < 1024) return `${mb.toFixed(0)} MB`;
    return `${(mb / 1024).toFixed(1)} GB`;
  }

  onMount(() => {
    refreshProcesses();
    interval = window.setInterval(refreshProcesses, 10000);
  });

  onDestroy(() => {
    destroyed = true;
    clearInterval(interval);
    if (killResultTimeout) clearTimeout(killResultTimeout);
  });
</script>

<div class="monitor-view">
  <div class="monitor-header">
    <h2 class="monitor-title">
      <Activity size={15} />
      Process Monitor
    </h2>
    <div class="monitor-actions">
      <button
        class="action-btn"
        class:spinning={loading}
        on:click={refreshProcesses}
        disabled={loading}
        title="Refresh"
        aria-label="Refresh process list"
      >
        <RefreshCw size={13} />
      </button>
      <button
        class="action-btn danger"
        on:click={killStale}
        disabled={isKilling}
        title="Kill stale processes"
        aria-label="Kill stale processes"
      >
        <Trash2 size={13} />
        <span>{isKilling ? 'Cleaning...' : 'Clean'}</span>
      </button>
    </div>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if killResult}
    <div class="kill-banner" class:success={killResult.count > 0}>
      {killResult.message}
      {#if killResult.killed?.length}
        — {killResult.killed.map(p => `${p.name} (${p.pid})`).join(', ')}
      {/if}
    </div>
  {/if}

  {#if processes.length === 0}
    <div class="empty-state">
      <Activity size={28} />
      <p class="empty-title">No processes tracked yet</p>
      <p class="empty-hint">
        Process monitoring shows Python, Node, and other child processes spawned by sessions.
      </p>
    </div>
  {:else}
    <div class="process-list">
      {#each processes as proc (proc.pid)}
        <div class="process-item" class:stale={proc.stale}>
          <div class="proc-main">
            <div class="proc-identity">
              <span class="proc-name">{proc.name}</span>
              <span class="proc-pid">PID {proc.pid}</span>
            </div>
            <div class="proc-stats">
              <span class="proc-stat" title="CPU usage">{(proc.cpu ?? 0).toFixed(1)}%</span>
              <span class="proc-stat" title="Memory usage">{formatMemory(proc.memory_mb ?? 0)}</span>
              <span class="proc-stat" title="Age">{formatAge(proc.age_seconds ?? 0)}</span>
            </div>
          </div>
          <div class="proc-meta">
            {#if proc.session}
              <span class="proc-session">{proc.session}</span>
            {:else}
              <span class="proc-orphan">orphan</span>
            {/if}
            {#if proc.stale}
              <span class="proc-stale-badge">stale</span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  {#if lastRefresh}
    <div class="refresh-time">
      Refreshed {lastRefresh.toLocaleTimeString()}
    </div>
  {/if}
</div>

<style>
  .monitor-view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background-color: var(--bg-secondary);
    overflow: hidden;
  }

  .monitor-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border);
    background-color: var(--bg-primary);
    flex-shrink: 0;
  }

  .monitor-title {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 1rem;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .monitor-actions {
    display: flex;
    align-items: center;
    gap: 0.375rem;
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 500;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
    flex-shrink: 0;
  }

  .action-btn:hover:not(:disabled) {
    background: var(--bg-surface);
    color: var(--text-primary);
    border-color: var(--text-secondary);
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-btn.danger:hover:not(:disabled) {
    color: #dc2626;
    border-color: #dc2626;
    background: rgba(220, 38, 38, 0.08);
  }

  .action-btn.spinning :global(svg) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to   { transform: rotate(360deg); }
  }

  .error-banner {
    background: rgba(220, 38, 38, 0.1);
    color: #dc2626;
    padding: 0.625rem 1rem;
    font-size: 0.8125rem;
    border-left: 3px solid #dc2626;
    flex-shrink: 0;
  }

  .kill-banner {
    background: rgba(99, 102, 241, 0.1);
    color: var(--text-secondary);
    padding: 0.625rem 1rem;
    font-size: 0.8125rem;
    border-left: 3px solid var(--accent);
    flex-shrink: 0;
  }

  .kill-banner.success {
    background: rgba(34, 197, 94, 0.1);
    color: #16a34a;
    border-left-color: #16a34a;
  }

  /* Empty state */
  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.625rem;
    padding: 2rem 1.5rem;
    text-align: center;
    color: var(--text-secondary);
  }

  .empty-state :global(svg) {
    opacity: 0.3;
    margin-bottom: 0.5rem;
  }

  .empty-title {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--text-primary);
    margin: 0;
  }

  .empty-hint {
    font-size: 0.75rem;
    color: var(--text-secondary);
    margin: 0;
    line-height: 1.5;
  }

  /* Process list */
  .process-list {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
  }

  .process-item {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    padding: 0.625rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    transition: border-color 0.15s;
  }

  .process-item.stale {
    border-left: 3px solid #f59e0b;
    background: rgba(245, 158, 11, 0.04);
  }

  .process-item:hover {
    border-color: var(--text-secondary);
  }

  .proc-main {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
  }

  .proc-identity {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  .proc-name {
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .proc-pid {
    font-size: 0.7rem;
    color: var(--text-secondary);
    font-family: monospace;
    flex-shrink: 0;
  }

  .proc-stats {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
  }

  .proc-stat {
    font-size: 0.7rem;
    color: var(--text-secondary);
    font-family: monospace;
    padding: 0.125rem 0.375rem;
    background: var(--bg-surface);
    border-radius: 0.25rem;
  }

  .proc-meta {
    display: flex;
    align-items: center;
    gap: 0.375rem;
  }

  .proc-session {
    font-size: 0.7rem;
    color: var(--accent);
    font-family: monospace;
    background: rgba(99, 102, 241, 0.1);
    padding: 0.1rem 0.375rem;
    border-radius: 0.25rem;
  }

  .proc-orphan {
    font-size: 0.7rem;
    color: var(--text-secondary);
    font-style: italic;
  }

  .proc-stale-badge {
    font-size: 0.65rem;
    font-weight: 600;
    color: #b45309;
    background: rgba(245, 158, 11, 0.12);
    padding: 0.1rem 0.375rem;
    border-radius: 0.25rem;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  /* Footer */
  .refresh-time {
    padding: 0.5rem 1rem;
    font-size: 0.7rem;
    color: var(--text-secondary);
    border-top: 1px solid var(--border);
    flex-shrink: 0;
    background-color: var(--bg-primary);
  }
</style>
