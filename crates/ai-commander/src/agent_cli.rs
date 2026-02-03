//! Agent CLI command handlers.
//!
//! Implements handlers for agent-related CLI commands including memory operations,
//! chat, feedback, and system status.

#[cfg(feature = "agents")]
use std::io::{self, BufRead, Write};

use crate::cli::{AgentCommands, FeedbackCommands, FeedbackTypeArg, MemoryCommands};

/// Result type for agent CLI operations.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Execute an agent command.
pub fn execute(command: AgentCommands) -> Result<()> {
    // Create tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new()?;

    match command {
        AgentCommands::Memory { command } => rt.block_on(handle_memory(command)),
        AgentCommands::Chat {
            message,
            interactive,
        } => rt.block_on(handle_chat(message, interactive)),
        AgentCommands::Session { id, adapter, input } => {
            rt.block_on(handle_session(&id, &adapter, &input))
        }
        AgentCommands::Feedback { command } => rt.block_on(handle_feedback(command)),
        AgentCommands::Status => handle_status(),
        AgentCommands::Paths => handle_paths(),
        AgentCommands::Check => rt.block_on(handle_check()),
    }
}

// =============================================================================
// Memory Commands
// =============================================================================

async fn handle_memory(command: MemoryCommands) -> Result<()> {
    use commander_memory::{EmbeddingGenerator, LocalStore, Memory, MemoryStore};

    let store = LocalStore::new(commander_core::config::state_dir().join("memory")).await?;
    let embedder = EmbeddingGenerator::from_env();

    match command {
        MemoryCommands::Store { agent_id, content } => {
            let embedding = embedder.embed(&content).await?;
            let memory = Memory::new(&agent_id, &content, embedding);
            store.store(memory).await?;
            println!("Memory stored for agent: {}", agent_id);
        }

        MemoryCommands::Search {
            query,
            agent_id,
            limit,
        } => {
            let query_embedding = embedder.embed(&query).await?;
            let results = if let Some(aid) = agent_id {
                store.search(&query_embedding, &aid, limit).await?
            } else {
                store.search_all(&query_embedding, limit).await?
            };

            if results.is_empty() {
                println!("No memories found.");
            } else {
                println!("Found {} memories:\n", results.len());
                for (i, result) in results.iter().enumerate() {
                    println!(
                        "{}. [{}] {} (score: {:.3})",
                        i + 1,
                        result.memory.agent_id,
                        truncate(&result.memory.content, 70),
                        result.score
                    );
                }
            }
        }

        MemoryCommands::List { agent_id, limit } => {
            let memories = store.list(&agent_id, limit).await?;

            if memories.is_empty() {
                println!("No memories found for agent: {}", agent_id);
            } else {
                println!("Memories for agent '{}' ({} found):\n", agent_id, memories.len());
                for (i, memory) in memories.iter().enumerate() {
                    println!(
                        "{}. [{}] {}",
                        i + 1,
                        memory.created_at.format("%Y-%m-%d %H:%M"),
                        truncate(&memory.content, 60)
                    );
                }
            }
        }

        MemoryCommands::Clear { agent_id } => {
            let count = store.count(&agent_id).await?;
            store.clear_agent(&agent_id).await?;
            println!("Cleared {} memories for agent: {}", count, agent_id);
        }

        MemoryCommands::Stats => {
            // Get stats by listing all known agent IDs
            let all_memories = store.search_all(&vec![0.0; 64], 10000).await?;

            let mut agent_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for result in &all_memories {
                *agent_counts.entry(result.memory.agent_id.clone()).or_insert(0) += 1;
            }

            println!("Memory Statistics");
            println!("=================");
            println!("Total memories: {}", all_memories.len());
            println!("\nBy agent:");
            for (agent_id, count) in &agent_counts {
                println!("  {}: {} memories", agent_id, count);
            }
            println!("\nStorage: {}", commander_core::config::state_dir().join("memory").display());
        }
    }

    Ok(())
}

// =============================================================================
// Chat Commands
// =============================================================================

#[cfg(feature = "agents")]
async fn handle_chat(message: Option<String>, interactive: bool) -> Result<()> {
    use commander_orchestrator::AgentOrchestrator;

    let mut orchestrator = AgentOrchestrator::new().await?;

    if interactive || message.is_none() {
        println!("Interactive chat mode. Type 'quit' or 'exit' to leave.\n");

        let stdin = io::stdin();
        loop {
            print!("You: ");
            io::stdout().flush()?;

            let mut input = String::new();
            stdin.lock().read_line(&mut input)?;
            let input = input.trim();

            if input.is_empty() {
                continue;
            }

            if input == "quit" || input == "exit" {
                println!("Goodbye!");
                break;
            }

            match orchestrator.process_user_input(input).await {
                Ok(response) => {
                    println!("\nAgent: {}\n", response);
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
    } else if let Some(msg) = message {
        let response = orchestrator.process_user_input(&msg).await?;
        println!("{}", response);
    }

    Ok(())
}

#[cfg(not(feature = "agents"))]
async fn handle_chat(_message: Option<String>, _interactive: bool) -> Result<()> {
    eprintln!("Error: Agent features are not enabled.");
    eprintln!("Rebuild with: cargo build --features agents");
    std::process::exit(1);
}

// =============================================================================
// Session Commands
// =============================================================================

#[cfg(feature = "agents")]
async fn handle_session(session_id: &str, adapter_type: &str, input: &str) -> Result<()> {
    use commander_orchestrator::AgentOrchestrator;

    let mut orchestrator = AgentOrchestrator::new().await?;
    let analysis = orchestrator
        .process_session_output(session_id, adapter_type, input)
        .await?;

    println!("Session Analysis");
    println!("================");
    println!("Session ID: {}", session_id);
    println!("Adapter: {}", adapter_type);
    println!("Detected completion: {}", analysis.detected_completion);
    println!(
        "Error detected: {}",
        analysis.error_detected.as_deref().unwrap_or("none")
    );

    if !analysis.summary.is_empty() {
        println!("\nSummary: {}", analysis.summary);
    }

    if !analysis.files_changed.is_empty() {
        println!("\nFiles changed:");
        for file in &analysis.files_changed {
            println!("  - {}", file);
        }
    }

    if analysis.waiting_for_input {
        println!("\nNote: Session requires user input");
    }

    Ok(())
}

#[cfg(not(feature = "agents"))]
async fn handle_session(_session_id: &str, _adapter_type: &str, _input: &str) -> Result<()> {
    eprintln!("Error: Agent features are not enabled.");
    eprintln!("Rebuild with: cargo build --features agents");
    std::process::exit(1);
}

// =============================================================================
// Feedback Commands
// =============================================================================

async fn handle_feedback(command: FeedbackCommands) -> Result<()> {
    use commander_agent::{AutoEval, Feedback, FeedbackType};

    let feedback_path = commander_core::config::state_dir().join("feedback");
    let mut auto_eval = AutoEval::new(feedback_path)?;

    match command {
        FeedbackCommands::Summary { agent_id } => {
            let aid = agent_id.as_deref().unwrap_or("user-agent");
            let summary = auto_eval.summary(aid);

            println!("Feedback Summary for '{}'", aid);
            println!("========================{}", "=".repeat(aid.len()));
            println!("Total: {}", summary.total);
            println!("Positive: {}", summary.positive);
            println!("Negative: {}", summary.negative);
            println!("Errors: {}", summary.errors);
            println!("Timeouts: {}", summary.timeouts);
            println!("Corrections: {}", summary.corrections);

            if !summary.most_common_issues.is_empty() {
                println!("\nCommon keywords in negative feedback:");
                for issue in &summary.most_common_issues {
                    println!("  - {}", issue);
                }
            }
        }

        FeedbackCommands::List { limit } => {
            let store = auto_eval.store();
            // Get feedback for all agents by checking common agent IDs
            let recent_user = store.get_recent("user-agent", limit).await;

            if recent_user.is_empty() {
                println!("No feedback entries found.");
            } else {
                println!("Recent Feedback ({} entries):\n", recent_user.len());
                for feedback in recent_user {
                    println!(
                        "[{}] {} - {} ({:.50}...)",
                        feedback.timestamp.format("%Y-%m-%d %H:%M"),
                        feedback.agent_id,
                        feedback.feedback_type,
                        feedback.user_input
                    );
                }
            }
        }

        FeedbackCommands::Add {
            agent_id,
            feedback_type,
            context,
            input,
            output,
        } => {
            let ft = match feedback_type {
                FeedbackTypeArg::Negative => FeedbackType::ExplicitNegative,
                FeedbackTypeArg::Retry => FeedbackType::ImplicitRetry,
                FeedbackTypeArg::Error => FeedbackType::Error,
                FeedbackTypeArg::Timeout => FeedbackType::Timeout,
                FeedbackTypeArg::Correction => FeedbackType::Correction,
                FeedbackTypeArg::Positive => FeedbackType::Positive,
            };

            let feedback = Feedback::new(&agent_id, ft, &context, &input, &output);
            auto_eval.store_mut().add(feedback).await?;
            println!("Feedback recorded for agent: {}", agent_id);
        }
    }

    Ok(())
}

// =============================================================================
// Status Commands
// =============================================================================

fn handle_status() -> Result<()> {
    println!("Agent System Status");
    println!("===================\n");

    // Storage paths
    let state_dir = commander_core::config::state_dir();
    let memory_dir = state_dir.join("memory");
    let feedback_dir = state_dir.join("feedback");

    println!("Storage Directories:");
    println!("  State: {}", state_dir.display());
    println!("  Memory: {}", memory_dir.display());
    println!("  Feedback: {}", feedback_dir.display());

    // Check directory existence
    println!("\nDirectory Status:");
    print_dir_status("State", &state_dir);
    print_dir_status("Memory", &memory_dir);
    print_dir_status("Feedback", &feedback_dir);

    // Check for data files
    println!("\nData Files:");
    print_file_status("Memories", &memory_dir.join("memories.json"));
    print_file_status("Feedback", &feedback_dir.join("feedback.json"));

    // Feature status
    println!("\nFeatures:");
    #[cfg(feature = "agents")]
    println!("  Agents: enabled");
    #[cfg(not(feature = "agents"))]
    println!("  Agents: disabled (rebuild with --features agents)");

    // API key status (don't show actual keys)
    println!("\nAPI Keys:");
    if std::env::var("OPENAI_API_KEY").is_ok() {
        println!("  OPENAI_API_KEY: set");
    } else if std::env::var("OPENROUTER_API_KEY").is_ok() {
        println!("  OPENROUTER_API_KEY: set");
    } else {
        println!("  No API keys found (will use hash-based embeddings)");
    }

    println!("\nAgent system ready.");
    Ok(())
}

fn handle_paths() -> Result<()> {
    use commander_core::config;

    println!("Commander Storage Paths");
    println!("=======================\n");

    println!("Base Directories:");
    println!("  State:    {}", config::state_dir().display());
    println!("  Database: {}", config::db_dir().display());
    println!("  Logs:     {}", config::logs_dir().display());
    println!("  Config:   {}", config::config_dir().display());
    println!("  Cache:    {}", config::cache_dir().display());

    println!("\nAgent-Specific:");
    println!("  Memory:   {}", config::state_dir().join("memory").display());
    println!("  Feedback: {}", config::state_dir().join("feedback").display());
    println!("  ChromaDB: {}", config::chroma_dir().display());

    println!("\nRuntime:");
    println!("  Sessions: {}", config::sessions_dir().display());
    println!("  Pairings: {}", config::pairing_file().display());

    println!("\nEnvironment Variables:");
    println!("  COMMANDER_STATE_DIR:  {}", std::env::var("COMMANDER_STATE_DIR").unwrap_or_else(|_| "(not set)".to_string()));
    println!("  COMMANDER_DB_DIR:     {}", std::env::var("COMMANDER_DB_DIR").unwrap_or_else(|_| "(not set)".to_string()));

    Ok(())
}

#[cfg(feature = "agents")]
async fn handle_check() -> Result<()> {
    use commander_memory::LocalStore;
    use commander_orchestrator::AgentOrchestrator;

    println!("Agent System Health Check");
    println!("=========================\n");

    // Check memory store
    print!("Memory store... ");
    match LocalStore::new(commander_core::config::state_dir().join("memory")).await {
        Ok(_) => println!("OK"),
        Err(e) => println!("FAILED: {}", e),
    }

    // Check orchestrator
    print!("Agent orchestrator... ");
    match AgentOrchestrator::new().await {
        Ok(_) => println!("OK"),
        Err(e) => println!("FAILED: {}", e),
    }

    // Check embedding generator
    print!("Embedding generator... ");
    let embedder = commander_memory::EmbeddingGenerator::from_env();
    match embedder.embed("test").await {
        Ok(v) => println!("OK (dimension: {})", v.len()),
        Err(e) => println!("FAILED: {}", e),
    }

    println!("\nAll checks completed.");
    Ok(())
}

#[cfg(not(feature = "agents"))]
async fn handle_check() -> Result<()> {
    use commander_memory::LocalStore;

    println!("Agent System Health Check (Limited)");
    println!("===================================\n");

    // Check memory store
    print!("Memory store... ");
    match LocalStore::new(commander_core::config::state_dir().join("memory")).await {
        Ok(_) => println!("OK"),
        Err(e) => println!("FAILED: {}", e),
    }

    // Check embedding generator
    print!("Embedding generator... ");
    let embedder = commander_memory::EmbeddingGenerator::from_env();
    match embedder.embed("test").await {
        Ok(v) => println!("OK (dimension: {})", v.len()),
        Err(e) => println!("FAILED: {}", e),
    }

    println!("\nNote: Full agent features disabled. Rebuild with --features agents.");
    Ok(())
}

// =============================================================================
// Helpers
// =============================================================================

/// Truncates a string to the given length, adding "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn print_dir_status(name: &str, path: &std::path::Path) {
    if path.exists() {
        if path.is_dir() {
            println!("  {}: OK", name);
        } else {
            println!("  {}: ERROR (not a directory)", name);
        }
    } else {
        println!("  {}: Not created yet", name);
    }
}

fn print_file_status(name: &str, path: &std::path::Path) {
    if path.exists() {
        if let Ok(metadata) = std::fs::metadata(path) {
            let size = metadata.len();
            println!("  {}: {} bytes", name, size);
        } else {
            println!("  {}: exists (size unknown)", name);
        }
    } else {
        println!("  {}: Not created yet", name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hi", 2), "hi");
    }
}
