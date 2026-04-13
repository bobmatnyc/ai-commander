<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { X, Key, ExternalLink } from 'lucide-svelte';

  const dispatch = createEventDispatcher();

  let openrouterKey = '';
  let saving = false;
  let saveStatus = '';
  let loading = true;

  onMount(async () => {
    try {
      const config = await invoke('get_config') as { openrouter_api_key: string; telegram_bot_token: string };
      openrouterKey = config.openrouter_api_key;
    } catch (e) {
      console.error('Failed to load config:', e);
    } finally {
      loading = false;
    }
  });

  async function saveOpenRouterKey() {
    saving = true;
    saveStatus = '';
    try {
      await invoke('save_config', { key: 'OPENROUTER_API_KEY', value: openrouterKey });
      saveStatus = 'Saved ✓';
      setTimeout(() => saveStatus = '', 2000);
    } catch (e) {
      saveStatus = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }

  function close() { dispatch('close'); }
  function handleBackdrop(e: MouseEvent) { if (e.target === e.currentTarget) close(); }
  function handleKey(e: KeyboardEvent) { if (e.key === 'Escape') close(); }
</script>

<svelte:window on:keydown={handleKey} />

<div class="backdrop" on:click={handleBackdrop} role="presentation">
  <div class="modal">
    <div class="modal-header">
      <h2>Settings</h2>
      <button class="close-btn" on:click={close}><X size={18} /></button>
    </div>

    <div class="modal-body">
      {#if loading}
        <p class="loading">Loading...</p>
      {:else}
        <section class="settings-section">
          <h3><Key size={14} /> API Keys</h3>

          <div class="field">
            <label for="openrouter-key">OpenRouter API Key</label>
            <p class="field-hint">Used for response summarization in Telegram. Get one at <a href="https://openrouter.ai" target="_blank">openrouter.ai <ExternalLink size={11} /></a></p>
            <div class="input-row">
              <input
                id="openrouter-key"
                type="password"
                bind:value={openrouterKey}
                placeholder="sk-or-..."
                class="key-input"
              />
              <button class="save-btn" on:click={saveOpenRouterKey} disabled={saving}>
                {saving ? 'Saving...' : 'Save'}
              </button>
            </div>
            {#if saveStatus}
              <p class="save-status" class:error={saveStatus.startsWith('Error')}>{saveStatus}</p>
            {/if}
          </div>
        </section>

        <section class="settings-section">
          <h3>Storage</h3>
          <p class="field-hint">Config saved to: <code>~/.ai-commander/config/.env.local</code></p>
        </section>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: 480px;
    max-width: 90vw;
    max-height: 80vh;
    overflow-y: auto;
    box-shadow: 0 24px 48px rgba(0,0,0,0.4);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1.25rem 1.5rem;
    border-bottom: 1px solid var(--border);
  }

  .modal-header h2 {
    font-size: 1rem;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 0.25rem;
    border-radius: 4px;
    display: flex;
    align-items: center;
  }

  .close-btn:hover { color: var(--text-primary); background: var(--surface-hover); }

  .modal-body { padding: 1.5rem; display: flex; flex-direction: column; gap: 1.5rem; }

  .settings-section h3 {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 1rem;
  }

  .field { display: flex; flex-direction: column; gap: 0.5rem; }

  label {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--text-primary);
  }

  .field-hint {
    font-size: 0.75rem;
    color: var(--text-secondary);
    margin: 0;
  }

  .field-hint a { color: var(--accent); text-decoration: none; }
  .field-hint a:hover { text-decoration: underline; }

  .input-row { display: flex; gap: 0.5rem; }

  .key-input {
    flex: 1;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 0.8rem;
    outline: none;
  }

  .key-input:focus { border-color: var(--accent); }

  .save-btn {
    padding: 0.5rem 1rem;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 500;
    white-space: nowrap;
  }

  .save-btn:hover { background: var(--accent-hover); }
  .save-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .save-status { font-size: 0.75rem; color: var(--green); margin: 0; }
  .save-status.error { color: var(--red); }

  code {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-secondary);
    background: var(--bg);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
  }

  .loading { color: var(--text-secondary); font-size: 0.875rem; }
</style>
