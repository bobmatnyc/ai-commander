<script lang="ts">
  import { Sun, Moon, Monitor } from 'lucide-svelte';
  import { themeMode, setTheme } from '../stores/theme';
  import type { ThemeMode } from '../stores/theme';

  const modes: ThemeMode[] = ['system', 'light', 'dark'];

  function cycle() {
    const current = $themeMode;
    const idx = modes.indexOf(current);
    const next = modes[(idx + 1) % modes.length];
    setTheme(next);
  }

  function select(mode: ThemeMode) {
    setTheme(mode);
  }
</script>

<div class="theme-toggle" role="group" aria-label="Theme selector">
  <button
    class="toggle-btn"
    class:active={$themeMode === 'light'}
    on:click={() => select('light')}
    title="Light theme"
    aria-pressed={$themeMode === 'light'}
  >
    <Sun size={13} />
  </button>
  <button
    class="toggle-btn"
    class:active={$themeMode === 'system'}
    on:click={() => select('system')}
    title="System theme"
    aria-pressed={$themeMode === 'system'}
  >
    <Monitor size={13} />
  </button>
  <button
    class="toggle-btn"
    class:active={$themeMode === 'dark'}
    on:click={() => select('dark')}
    title="Dark theme"
    aria-pressed={$themeMode === 'dark'}
  >
    <Moon size={13} />
  </button>
</div>

<style>
  .theme-toggle {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    border-radius: 7px;
    border: 1px solid var(--border);
    background: var(--bg);
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }

  .toggle-btn:hover {
    color: var(--text-primary);
    background: var(--surface-hover);
  }

  .toggle-btn.active {
    background: var(--accent);
    color: white;
  }
</style>
