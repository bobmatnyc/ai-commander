import { writable, derived } from 'svelte/store';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';

const STORAGE_KEY = 'commander:theme';

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined') return 'dark';
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function getSavedMode(): ThemeMode {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'light' || saved === 'dark' || saved === 'system') return saved;
  } catch {
    // ignore
  }
  return 'system';
}

function applyTheme(resolved: ResolvedTheme) {
  document.documentElement.setAttribute('data-theme', resolved);
}

// Resolve mode to actual light/dark
function resolveMode(mode: ThemeMode): ResolvedTheme {
  if (mode === 'system') return getSystemTheme();
  return mode;
}

// Initialize synchronously so there is no flash of wrong theme
const initialMode = getSavedMode();
const initialResolved = resolveMode(initialMode);
applyTheme(initialResolved);

export const themeMode = writable<ThemeMode>(initialMode);

export const resolvedTheme = derived(themeMode, ($mode) => resolveMode($mode));

// Apply to DOM whenever resolved theme changes
resolvedTheme.subscribe((resolved) => {
  if (typeof document !== 'undefined') {
    applyTheme(resolved);
  }
});

export function setTheme(mode: ThemeMode) {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // ignore
  }
  themeMode.set(mode);
}

// Listen for OS-level preference changes; only matters when mode === 'system'
if (typeof window !== 'undefined') {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    themeMode.update((current) => {
      if (current === 'system') {
        // Trigger re-derivation by returning the same value through a no-op update
        applyTheme(getSystemTheme());
      }
      return current;
    });
  });
}
