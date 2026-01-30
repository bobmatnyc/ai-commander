//! Adapter registry for discovering and instantiating adapters.

use std::collections::HashMap;
use std::sync::Arc;

use crate::claude_code::ClaudeCodeAdapter;
use crate::mpm::MpmAdapter;
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

        Self { adapters }
    }

    /// Creates an empty registry.
    pub fn empty() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Registers an adapter.
    pub fn register(&mut self, adapter: Arc<dyn RuntimeAdapter>) {
        let id = adapter.info().id.clone();
        self.adapters.insert(id, adapter);
    }

    /// Gets an adapter by ID.
    pub fn get(&self, id: &str) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapters.get(id).cloned()
    }

    /// Lists all registered adapter IDs.
    pub fn list(&self) -> Vec<&str> {
        self.adapters.keys().map(|s| s.as_str()).collect()
    }

    /// Returns the number of registered adapters.
    pub fn len(&self) -> usize {
        self.adapters.len()
    }

    /// Returns true if no adapters are registered.
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
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
    ///
    /// Returns `None` if the alias is unknown.
    pub fn resolve(&self, alias: &str) -> Option<&'static str> {
        match alias {
            "cc" | "claude-code" => Some("claude-code"),
            "mpm" => Some("mpm"),
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
        assert!(registry.len() >= 2); // claude-code and mpm
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

        // Test unknown alias
        assert_eq!(registry.resolve("unknown"), None);
    }
}
