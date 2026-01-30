//! Command handlers for CLI subcommands.

use std::path::Path;

use commander_adapters::AdapterRegistry;
use commander_models::{Project, ProjectState};
use commander_persistence::StateStore;
use tracing::{info, warn};

use crate::cli::{Commands, OutputFormat};

/// Result type for command operations.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Execute a CLI command.
pub fn execute(command: Commands, state_dir: &Path) -> Result<()> {
    let store = StateStore::new(state_dir);

    match command {
        Commands::Start { path, adapter, name } => {
            cmd_start(&store, &path, &adapter, name.as_deref())
        }
        Commands::Stop { project, force } => cmd_stop(&store, &project, force),
        Commands::List { running, format } => cmd_list(&store, running, format),
        Commands::Status { project, detailed } => cmd_status(&store, project.as_deref(), detailed),
        Commands::Send { project, message } => cmd_send(&store, &project, &message),
        Commands::Repl { project: _ } => {
            // REPL is handled separately in main
            Ok(())
        }
        Commands::Adapters => cmd_adapters(),
    }
}

fn cmd_start(store: &StateStore, path: &Path, adapter: &str, name: Option<&str>) -> Result<()> {
    // Verify adapter exists
    let registry = AdapterRegistry::new();
    let adapter_info = registry
        .get(adapter)
        .ok_or_else(|| format!("Unknown adapter: {}", adapter))?;

    // Create project
    let project_name = name
        .map(String::from)
        .or_else(|| path.file_name().and_then(|n| n.to_str()).map(String::from))
        .unwrap_or_else(|| "unnamed".to_string());

    let path_str = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let project = Project::new(path_str, project_name.clone());

    info!(
        project_id = %project.id,
        project_name = %project_name,
        adapter = %adapter,
        "Starting project"
    );

    // Save project
    store.save_project(&project)?;

    println!("Started project '{}' ({})", project_name, project.id);
    println!("  Path: {}", path.display());
    println!(
        "  Adapter: {} ({})",
        adapter_info.info().name,
        adapter
    );
    println!("\nNote: Actual runtime spawning will be implemented in Phase 7");

    Ok(())
}

fn cmd_stop(store: &StateStore, project_id: &str, force: bool) -> Result<()> {
    // Find project by ID or name
    let projects = store.load_all_projects()?;
    let project = projects
        .values()
        .find(|p| p.id.as_str() == project_id || p.name == project_id)
        .ok_or_else(|| format!("Project not found: {}", project_id))?;

    info!(
        project_id = %project.id,
        force = force,
        "Stopping project"
    );

    // Update state
    let mut project = project.clone();
    project.set_state(ProjectState::Paused, Some("Stopped by user".to_string()));
    store.save_project(&project)?;

    println!("Stopped project '{}' ({})", project.name, project.id);
    if force {
        println!("  (forced)");
    }
    println!("\nNote: Actual runtime termination will be implemented in Phase 7");

    Ok(())
}

fn cmd_list(store: &StateStore, running_only: bool, format: OutputFormat) -> Result<()> {
    let projects = store.load_all_projects()?;

    let filtered: Vec<_> = projects
        .values()
        .filter(|p| !running_only || p.state == ProjectState::Working)
        .collect();

    match format {
        OutputFormat::Table => {
            if filtered.is_empty() {
                println!("No projects found.");
                return Ok(());
            }

            println!(
                "{:<36}  {:<20}  {:<10}  PATH",
                "ID", "NAME", "STATE"
            );
            println!("{}", "-".repeat(80));
            for project in &filtered {
                println!(
                    "{:<36}  {:<20}  {:<10}  {}",
                    project.id,
                    truncate(&project.name, 20),
                    format!("{:?}", project.state),
                    truncate(&project.path, 30)
                );
            }
            println!("\n{} project(s)", filtered.len());
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&filtered)?;
            println!("{}", json);
        }
        OutputFormat::Brief => {
            for project in &filtered {
                println!("{}\t{}", project.id, project.name);
            }
        }
    }

    Ok(())
}

fn cmd_status(store: &StateStore, project_id: Option<&str>, detailed: bool) -> Result<()> {
    match project_id {
        Some(id) => {
            let projects = store.load_all_projects()?;
            let project = projects
                .values()
                .find(|p| p.id.as_str() == id || p.name == id)
                .ok_or_else(|| format!("Project not found: {}", id))?;

            print_project_status(project, detailed);
        }
        None => {
            let projects = store.load_all_projects()?;
            if projects.is_empty() {
                println!("No projects found.");
                return Ok(());
            }

            for project in projects.values() {
                print_project_status(project, detailed);
                println!();
            }
        }
    }

    Ok(())
}

fn print_project_status(project: &Project, detailed: bool) {
    println!("Project: {} ({})", project.name, project.id);
    println!("  State: {:?}", project.state);
    if let Some(reason) = &project.state_reason {
        println!("  Reason: {}", reason);
    }
    println!("  Path: {}", project.path);
    println!("  Created: {}", project.created_at);

    if detailed {
        println!("  Sessions: {}", project.sessions.len());
        println!("  Work Queue: {} items", project.work_queue.len());
        println!("  Pending Events: {}", project.pending_events.len());
        println!("  Thread Messages: {}", project.thread.len());
    }
}

fn cmd_send(_store: &StateStore, project_id: &str, message: &str) -> Result<()> {
    warn!("Send command not fully implemented yet");
    println!("Would send to '{}': {}", project_id, message);
    println!("\nNote: Message sending will be implemented in Phase 7");
    Ok(())
}

fn cmd_adapters() -> Result<()> {
    let registry = AdapterRegistry::new();

    println!("Available Runtime Adapters:");
    println!();

    for id in registry.list() {
        if let Some(adapter) = registry.get(id) {
            let info = adapter.info();
            println!("  {} - {}", info.id, info.name);
            println!("    {}", info.description);
            println!("    Command: {}", info.command);
            println!();
        }
    }

    Ok(())
}

/// Truncates a string to the given length, adding "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cmd_list_empty() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path());

        // Should not panic on empty list
        cmd_list(&store, false, OutputFormat::Brief).unwrap();
    }

    #[test]
    fn test_cmd_adapters() {
        // Should not panic
        cmd_adapters().unwrap();
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(truncate("hi", 2), "hi");
    }
}
