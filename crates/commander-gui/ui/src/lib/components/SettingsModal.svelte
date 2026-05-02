<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';
  import { invoke } from '../transport';
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

  /**
   * Why: When the modal is rendered as a descendant of components that have
   * created a stacking context (e.g. mobile <aside> with position:absolute +
   * z-index, or any ancestor with transform/filter/backdrop-filter), the
   * modal's z-index becomes scoped to that local context and can render BELOW
   * sibling elements like the sidebar overlay. Moving the rendered node to
   * document.body sidesteps the issue entirely — the modal lives at the top
   * level of the DOM and its z-index is global.
   * What: Svelte action that detaches the node from its current parent and
   * appends it to document.body on mount, then removes it on destroy.
   * Test: Open the settings modal on a ≤768px viewport — assert the backdrop
   * covers the sidebar (which has z-index:10) and clicks on the modal land on
   * the modal, not on session rows underneath.
   */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        if (node.parentNode) {
          node.parentNode.removeChild(node);
        }
      },
    };
  }
</script>

<svelte:window on:keydown={handleKey} />

<div class="backdrop" on:click={handleBackdrop} role="presentation" use:portal>
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
    /* Sit above all other UI: header (z:50), dropdowns (z:100), mobile aside
     * (z:10), CommandPalette (z:2000), ProcessMonitorPanel confirm (z:9999). */
    z-index: 10000;
  }

  .modal {
    background: var(--bg-primary, #1e1e2e);
    border: 1px solid var(--border, #45475a);
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

  .close-btn:hover { color: var(--text-primary); background: var(--bg-surface, #313244); }

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
    background: var(--bg-secondary, #181825);
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

  .save-btn:hover { background: #4f46e5; }
  .save-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .save-status { font-size: 0.75rem; color: #a6e3a1; margin: 0; }
  .save-status.error { color: #f38ba8; }

  code {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-secondary);
    background: var(--bg-secondary, #181825);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
  }

  .loading { color: var(--text-secondary); font-size: 0.875rem; }
</style>
