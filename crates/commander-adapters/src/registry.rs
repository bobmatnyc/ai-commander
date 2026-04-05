//! Adapter registry for discovering and instantiating adapters.

use std::collections::HashMap;
use std::sync::Arc;

use crate::claude_code::ClaudeCodeAdapter;
use crate::event_driven::EventDrivenAdapter;
use crate::mpm::MpmAdapter;
use crate::mpm_sdk::MpmSdkAdapter;
use crate::shell::ShellAdapter;
use crate::traits::RuntimeAdapter;

/// Registry for runtime adapters.
///
/// The registry allows discovering available adapters and getting
/// instances by name. All adapters are stored as `Arc<dyn RuntimeAdapter>`
/// to allow sharing across threads.
///
/// # Example
///
/// ```
/// use commander_adapters::AdapterRegistry;
///
/// let registry = AdapterRegistry::new();
///
/// // List available adapters
/// for id in registry.list() {
///     println!("Available: {}", id);
/// }
///
/// // Get a specific adapter
/// if let Some(adapter) = registry.get("claude-code") {
///     let info = adapter.info();
///     println!("Using: {} ({})", info.name, info.description);
/// }
/// ```
pub struct AdapterRegistry {
    adapters: HashMap<String, Arc<dyn RuntimeAdapter>>,
    event_driven: HashMap<String, Arc<dyn EventDrivenAdapter>>,
}

impl AdapterRegistry {
    /// Creates a new registry with all built-in adapters.
    pub fn new() -> Self {
        let mut adapters: HashMap<String, Arc<dyn RuntimeAdapter>> = HashMap::new();

        // Register built-in adapters
        let claude_code = Arc::new(ClaudeCodeAdapter::new());
        adapters.insert(claude_code.info().id.clone(), claude_code);

        let mpm = Arc::new(MpmAdapter::new());
        adapters.insert(mpm.info().id.clone(), mpm);

        let shell = Arc::new(ShellAdapter::new());
        adapters.insert(shell.info().id.clone(), shell);

        let mut event_driven: HashMap<String, Arc<dyn EventDrivenAdapter>> = HashMap::new();
        let mpm_sdk = Arc::new(MpmSdkAdapter::new());
        event_driven.insert(mpm_sdk.info().id.clone(), mpm_sdk);

        Self {
            adapters,
            event_driven,
        }
    }

    /// Creates an empty registry.
    pub fn empty() -> Self {
        Self {
            adapters: HashMap::new(),
            event_driven: HashMap::new(),
        }
    }

    /// Registers a terminal-output `RuntimeAdapter`.
    pub fn register(&mut self, adapter: Arc<dyn RuntimeAdapter>) {
        let id = adapter.info().id.clone();
        self.adapters.insert(id, adapter);
    }

    /// Registers an event-driven adapter.
    pub fn register_event_driven(&mut self, adapter: Arc<dyn EventDrivenAdapter>) {
        let id = adapter.info().id.clone();
        self.event_driven.insert(id, adapter);
    }

    /// Gets an adapter by ID.
    pub fn get(&self, id: &str) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapters.get(id).cloned()
    }

    /// Gets an event-driven adapter by ID.
    pub fn get_event_driven(&self, id: &str) -> Option<Arc<dyn EventDrivenAdapter>> {
        self.event_driven.get(id).cloned()
    }

    /// Returns true if an event-driven adapter is registered under this id.
    pub fn is_event_driven(&self, id: &str) -> bool {
        self.event_driven.contains_key(id)
    }

    /// Lists all registered adapter IDs (both terminal and event-driven).
    pub fn list(&self) -> Vec<&str> {
        self.adapters
            .keys()
            .chain(self.event_driven.keys())
            .map(|s| s.as_str())
            .collect()
    }

    /// Returns the number of registered adapters (terminal + event-driven).
    pub fn len(&self) -> usize {
        self.adapters.len() + self.event_driven.len()
    }

    /// Returns true if no adapters are registered.
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty() && self.event_driven.is_empty()
    }

    /// Gets the default adapter (claude-code).
    pub fn default_adapter(&self) -> Option<Arc<dyn RuntimeAdapter>> {
        self.get("claude-code")
    }

    /// Resolves a tool alias to its canonical adapter ID.
    ///
    /// Supports the following aliases:
    /// - `cc` -> `claude-code`
    /// - `mpm` -> `mpm` (already canonical)
    /// - `claude-code` -> `claude-code` (already canonical)
    /// - `shell`, `sh`, `bash`, `zsh` -> `shell`
    ///
    /// Returns `None` if the alias is unknown.
    pub fn resolve(&self, alias: &str) -> Option<&'static str> {
        match alias {
            "cc" | "claude-code" => Some("claude-code"),
            "mpm" => Some("mpm"),
            "mpm-sdk" => Some("mpm-sdk"),
            "shell" | "sh" | "bash" | "zsh" => Some("shell"),
            _ => {
                // Check if it's a registered adapter ID
                if self.adapters.contains_key(alias) {
                    // For dynamic adapters, we return the static mapping if known
                    // Since we can't return a reference to the input, we just use known IDs
                    None
                } else {
                    None
                }
            }
        }
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let registry = AdapterRegistry::new();
        assert!(!registry.is_empty());
        // claude-code, mpm, shell (terminal) + mpm-sdk (event-driven)
        assert!(registry.len() >= 4);
    }

    #[test]
    fn test_registry_get() {
        let registry = AdapterRegistry::new();

        let adapter = registry.get("claude-code");
        assert!(adapter.is_some());
        assert_eq!(adapter.unwrap().info().id, "claude-code");
    }

    #[test]
    fn test_registry_list() {
        let registry = AdapterRegistry::new();
        let list = registry.list();

        assert!(list.contains(&"claude-code"));
        assert!(list.contains(&"mpm"));
        assert!(list.contains(&"shell"));
        assert!(list.contains(&"mpm-sdk"));
    }

    #[test]
    fn test_registry_register() {
        let mut registry = AdapterRegistry::empty();
        assert!(registry.is_empty());

        registry.register(Arc::new(ClaudeCodeAdapter::new()));
        assert_eq!(registry.len(), 1);
        assert!(registry.get("claude-code").is_some());
    }

    #[test]
    fn test_default_adapter() {
        let registry = AdapterRegistry::new();
        let default = registry.default_adapter();

        assert!(default.is_some());
        assert_eq!(default.unwrap().info().id, "claude-code");
    }

    #[test]
    fn test_adapter_is_send_sync() {
        // This test verifies that adapters can be shared across threads
        let registry = AdapterRegistry::new();
        let adapter = registry.get("claude-code").unwrap();

        // Spawn a thread and use the adapter
        let handle = std::thread::spawn(move || adapter.info().name.clone());

        let name = handle.join().unwrap();
        assert_eq!(name, "Claude Code");
    }

    #[test]
    fn test_resolve_aliases() {
        let registry = AdapterRegistry::new();

        // Test known aliases
        assert_eq!(registry.resolve("cc"), Some("claude-code"));
        assert_eq!(registry.resolve("claude-code"), Some("claude-code"));
        assert_eq!(registry.resolve("mpm"), Some("mpm"));
        assert_eq!(registry.resolve("mpm-sdk"), Some("mpm-sdk"));

        // Test shell aliases
        assert_eq!(registry.resolve("shell"), Some("shell"));
        assert_eq!(registry.resolve("sh"), Some("shell"));
        assert_eq!(registry.resolve("bash"), Some("shell"));
        assert_eq!(registry.resolve("zsh"), Some("shell"));

        // Test unknown alias
        assert_eq!(registry.resolve("unknown"), None);
    }

    #[test]
    fn test_shell_adapter() {
        let registry = AdapterRegistry::new();

        let adapter = registry.get("shell");
        assert!(adapter.is_some());
        assert_eq!(adapter.unwrap().info().id, "shell");
    }

    #[test]
    fn test_event_driven_registration() {
        let registry = AdapterRegistry::new();

        // mpm-sdk is registered as an event-driven adapter.
        assert!(registry.get_event_driven("mpm-sdk").is_some());
        assert_eq!(
            registry.get_event_driven("mpm-sdk").unwrap().info().id,
            "mpm-sdk"
        );
    }

    #[test]
    fn test_is_event_driven() {
        let registry = AdapterRegistry::new();

        assert!(registry.is_event_driven("mpm-sdk"));
        assert!(!registry.is_event_driven("claude-code"));
        assert!(!registry.is_event_driven("mpm"));
        assert!(!registry.is_event_driven("shell"));
        assert!(!registry.is_event_driven("unknown"));
    }

    #[test]
    fn test_event_driven_not_in_terminal_map() {
        let registry = AdapterRegistry::new();

        // mpm-sdk should NOT be retrievable via the terminal `get` — it lives
        // in the event-driven map only.
        assert!(registry.get("mpm-sdk").is_none());
    }
}
