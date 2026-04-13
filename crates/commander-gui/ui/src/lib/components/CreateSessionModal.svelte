<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { X } from 'lucide-svelte';

  export let show = false;

  const dispatch = createEventDispatcher();

  interface ProjectDirectory {
    name: string;
    path: string;
    project_type: string;
  }

  interface AdapterOption {
    id: string;
    label: string;
  }

  const ADAPTERS: AdapterOption[] = [
    { id: 'claude-code', label: 'Claude Code' },
    { id: 'claude-mpm', label: 'Claude MPM' },
    { id: 'auggie', label: 'Auggie' },
    { id: 'codex', label: 'Codex' },
    { id: 'shell', label: 'Shell' },
  ];

  let directories: ProjectDirectory[] = [];
  let selectedDirectory: ProjectDirectory | null = null;
  let sessionName = '';
  let selectedAdapter = 'claude-mpm';
  let loading = false;
  let error = '';
  let filterText = '';

  $: filteredDirectories = directories.filter(dir => {
    if (!filterText) return true;
    const search = filterText.toLowerCase();
    return dir.path.toLowerCase().includes(search);
  });

  async function loadDirectories() {
    try {
      directories = await invoke('list_project_directories');
      error = '';
    } catch (err) {
      error = `Failed to load directories: ${err}`;
    }
  }

  $: if (show) {
    loadDirectories();
  }

  async function handleCreate() {
    if (!sessionName || !selectedDirectory) {
      error = 'Please select a directory and enter a session name';
      return;
    }

    loading = true;
    error = '';

    try {
      await invoke('create_session', {
        name: sessionName,
        directory: selectedDirectory.path,
        adapter: selectedAdapter,
      });

      dispatch('created');
      close();
    } catch (err) {
      error = `Failed to create session: ${err}`;
    } finally {
      loading = false;
    }
  }

  function close() {
    show = false;
    sessionName = '';
    selectedAdapter = 'claude-mpm';
    selectedDirectory = null;
    error = '';
    filterText = '';
  }

  function handleOverlayClick(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      close();
    }
  }
</script>

{#if show}
  <div class="modal-overlay" on:click={handleOverlayClick} on:keydown={(e) => e.key === 'Escape' && close()} role="presentation">
    <div class="modal-content" on:click|stopPropagation on:keydown|stopPropagation role="dialog" aria-modal="true" aria-labelledby="modal-title">
      <div class="modal-header">
        <h2 id="modal-title">Create New Session</h2>
        <button class="close-btn" on:click={close}>
          <X size={20} />
        </button>
      </div>

      <div class="modal-body">
        <div class="form-group">
          <label for="session-name">Session Name</label>
          <input
            id="session-name"
            type="text"
            bind:value={sessionName}
            placeholder="my-session"
            class="input"
          />
        </div>

        <div class="form-group">
          <label for="adapter-select">Adapter</label>
          <select
            id="adapter-select"
            bind:value={selectedAdapter}
            class="input select"
          >
            {#each ADAPTERS as adapter}
              <option value={adapter.id}>{adapter.label}</option>
            {/each}
          </select>
        </div>

        <div class="form-group">
          <label for="directory-filter">Project Path</label>
          <div class="filter-wrapper">
            <span class="filter-icon" aria-hidden="true">&#x1F50D;</span>
            <input
              id="directory-filter"
              type="text"
              bind:value={filterText}
              placeholder="Filter projects..."
              class="input filter-input"
            />
          </div>
          {#if directories.length > 0}
            <p class="filter-count">
              Showing {filteredDirectories.length} of {directories.length} project{directories.length === 1 ? '' : 's'}
            </p>
          {/if}
          <div class="directory-list" id="directory-list" role="listbox" aria-label="Project directories">
            {#each filteredDirectories as dir}
              <button
                class="directory-item"
                class:selected={selectedDirectory?.path === dir.path}
                on:click={() => { selectedDirectory = dir; if (!sessionName) sessionName = dir.name; }}
                role="option"
                aria-selected={selectedDirectory?.path === dir.path}
              >
                <div class="dir-info">
                  <span class="dir-name">{dir.name}</span>
                  <span class="dir-type">{dir.project_type}</span>
                </div>
                <span class="dir-path">{dir.path}</span>
              </button>
            {/each}

            {#if directories.length === 0}
              <p class="no-dirs">No project directories found</p>
            {:else if filteredDirectories.length === 0}
              <p class="no-dirs">No projects match &ldquo;{filterText}&rdquo;</p>
            {/if}
          </div>
        </div>

        {#if error}
          <div class="error-message">{error}</div>
        {/if}
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" on:click={close} disabled={loading}>
          Cancel
        </button>
        <button
          class="btn btn-primary"
          on:click={handleCreate}
          disabled={loading || !sessionName || !selectedDirectory}
        >
          {loading ? 'Creating...' : 'Create Session'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-content {
    background: white;
    border-radius: 8px;
    width: 90%;
    max-width: 600px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.5rem;
    border-bottom: 1px solid #e5e7eb;
  }

  .modal-header h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
  }

  .close-btn {
    background: none;
    border: none;
    cursor: pointer;
    padding: 0.25rem;
    display: flex;
    align-items: center;
    color: #6b7280;
  }

  .close-btn:hover {
    color: #1f2937;
  }

  .modal-body {
    padding: 1.5rem;
    overflow-y: auto;
  }

  .form-group {
    margin-bottom: 1.5rem;
  }

  .form-group label {
    display: block;
    margin-bottom: 0.5rem;
    font-weight: 500;
    font-size: 0.875rem;
    color: #374151;
  }

  .input {
    width: 100%;
    padding: 0.5rem 0.75rem;
    border: 1px solid #d1d5db;
    border-radius: 6px;
    font-size: 0.875rem;
    box-sizing: border-box;
  }

  .input:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
  }

  .select {
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%236b7280' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 0.75rem center;
    padding-right: 2.5rem;
    cursor: pointer;
  }

  .filter-wrapper {
    position: relative;
    margin-bottom: 0.375rem;
  }

  .filter-icon {
    position: absolute;
    left: 0.625rem;
    top: 50%;
    transform: translateY(-50%);
    font-size: 0.75rem;
    pointer-events: none;
    opacity: 0.5;
  }

  .filter-input {
    padding-left: 1.875rem;
    padding-top: 0.375rem;
    padding-bottom: 0.375rem;
    font-size: 0.8125rem;
    border-color: #e5e7eb;
    background: #f9fafb;
  }

  .filter-input:focus {
    background: white;
    border-color: #3b82f6;
  }

  .filter-count {
    margin: 0 0 0.375rem;
    font-size: 0.75rem;
    color: #9ca3af;
  }

  .directory-list {
    border: 1px solid #d1d5db;
    border-radius: 6px;
    max-height: 220px;
    overflow-y: auto;
  }

  .directory-item {
    width: 100%;
    padding: 0.75rem;
    border: none;
    border-bottom: 1px solid #e5e7eb;
    background: white;
    cursor: pointer;
    text-align: left;
    transition: background 0.2s;
  }

  .directory-item:hover {
    background: #f9fafb;
  }

  .directory-item.selected {
    background: #dbeafe;
    border-left: 3px solid #3b82f6;
  }

  .directory-item:last-child {
    border-bottom: none;
  }

  .dir-info {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.25rem;
  }

  .dir-name {
    font-weight: 500;
    font-size: 0.875rem;
    color: #1f2937;
  }

  .dir-type {
    font-size: 0.75rem;
    padding: 0.125rem 0.5rem;
    background: #f3f4f6;
    border-radius: 9999px;
    color: #6b7280;
  }

  .dir-path {
    font-size: 0.75rem;
    color: #6b7280;
  }

  .no-dirs {
    padding: 2rem;
    text-align: center;
    color: #6b7280;
    font-size: 0.875rem;
  }

  .error-message {
    padding: 0.75rem;
    background: #fee2e2;
    color: #991b1b;
    border-radius: 6px;
    margin-top: 1rem;
    font-size: 0.875rem;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.75rem;
    padding: 1.5rem;
    border-top: 1px solid #e5e7eb;
  }

  .btn {
    padding: 0.5rem 1rem;
    border-radius: 6px;
    border: none;
    cursor: pointer;
    font-weight: 500;
    font-size: 0.875rem;
    transition: all 0.2s;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    background: #f3f4f6;
    color: #374151;
  }

  .btn-secondary:hover:not(:disabled) {
    background: #e5e7eb;
  }

  .btn-primary {
    background: #3b82f6;
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: #2563eb;
  }
</style>
