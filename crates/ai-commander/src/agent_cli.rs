//! Agent CLI command handlers.
//!
//! Implements handlers for agent-related CLI commands including memory operations,
//! chat, feedback, and system status.

#[cfg(feature = "agents")]
use std::io::{self, BufRead, Write};

use crate::cli::{AgentCommands, ContextCommands, FeedbackCommands, FeedbackTypeArg, MemoryCommands};

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
        AgentCommands::Detect {
            input,
            file,
            verbose,
        } => handle_detect(input.as_deref(), file.as_deref(), verbose),
        AgentCommands::Autonomous {
            task,
            max_iterations,
        } => rt.block_on(handle_autonomous(&task, max_iterations)),
        AgentCommands::Goals { task } => handle_goals(&task),
        AgentCommands::Context { command } => handle_context(command),
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

// =============================================================================
// Change Detection Commands
// =============================================================================

fn handle_detect(input: Option<&str>, file: Option<&std::path::Path>, verbose: bool) -> Result<()> {
    use commander_core::change_detector::{ChangeDetector, Significance};

    let text = if let Some(input) = input {
        input.to_string()
    } else if let Some(file) = file {
        std::fs::read_to_string(file)?
    } else {
        return Err("Provide --input or --file".into());
    };

    let mut detector = ChangeDetector::new();
    let event = detector.detect(&text);

    println!("Change Detection Result");
    println!("=======================");
    println!("Type: {:?}", event.change_type);
    println!("Significance: {:?}", event.significance);
    println!("Summary: {}", event.summary);

    if verbose && !event.diff_lines.is_empty() {
        println!("\nDiff Lines:");
        for line in &event.diff_lines {
            println!("  + {}", line);
        }
    }

    // Show if this would trigger LLM analysis
    let would_invoke_llm = event.significance >= Significance::Medium;
    println!(
        "\nWould invoke LLM: {}",
        if would_invoke_llm { "Yes" } else { "No" }
    );
    println!(
        "Would notify user: {}",
        if event.requires_notification() {
            "Yes"
        } else {
            "No"
        }
    );

    Ok(())
}

// =============================================================================
// Autonomous Mode Commands
// =============================================================================

#[cfg(feature = "agents")]
async fn handle_autonomous(task: &str, max_iterations: usize) -> Result<()> {
    use commander_agent::{CompletionDriver, Goal};

    println!("Autonomous Task Execution");
    println!("=========================\n");
    println!("Task: {}", task);
    println!("Max iterations: {}\n", max_iterations);

    // Parse goals from task (simplified extraction)
    let goals = extract_goals_from_task(task);

    let mut driver = CompletionDriver::with_max_iterations(max_iterations);
    for goal in goals {
        driver.add_goal(Goal::new(goal));
    }

    println!("Parsed Goals:");
    println!("{}", driver.format_progress());

    println!("\nNote: Full autonomous execution requires a running session.");
    println!("This command demonstrates goal parsing and driver setup.");

    // Show what the driver would do
    let decision = driver.should_continue();
    println!("\nInitial Decision: {:?}", decision);

    Ok(())
}

#[cfg(not(feature = "agents"))]
async fn handle_autonomous(_task: &str, _max_iterations: usize) -> Result<()> {
    eprintln!("Error: Autonomous mode requires the 'agents' feature.");
    eprintln!("Rebuild with: cargo build --features agents");
    std::process::exit(1);
}

fn handle_goals(task: &str) -> Result<()> {
    println!("Goal Extraction");
    println!("===============\n");
    println!("Task: {}\n", task);

    let goals = extract_goals_from_task(task);

    if goals.is_empty() {
        println!("No specific goals detected. Using task as single goal.");
        println!("\n1. {}", task);
    } else {
        println!("Extracted Goals:");
        for (i, goal) in goals.iter().enumerate() {
            println!("{}. {}", i + 1, goal);
        }
    }

    Ok(())
}

/// Extract goals from a task description using simple heuristics.
fn extract_goals_from_task(task: &str) -> Vec<String> {
    let mut goals = Vec::new();

    // Look for numbered items: "1. ", "2. ", etc.
    let numbered_re = regex::Regex::new(r"(?m)^\s*\d+\.\s*(.+)$").ok();
    if let Some(re) = numbered_re {
        for cap in re.captures_iter(task) {
            if let Some(goal) = cap.get(1) {
                goals.push(goal.as_str().trim().to_string());
            }
        }
    }

    // Look for bullet points: "- ", "* "
    if goals.is_empty() {
        let bullet_re = regex::Regex::new(r"(?m)^\s*[-*]\s*(.+)$").ok();
        if let Some(re) = bullet_re {
            for cap in re.captures_iter(task) {
                if let Some(goal) = cap.get(1) {
                    goals.push(goal.as_str().trim().to_string());
                }
            }
        }
    }

    // Look for "and" separated items in common patterns
    if goals.is_empty() {
        // Pattern: "Implement X and Y and Z"
        let and_re = regex::Regex::new(r"(?i)^(?:implement|create|build|add|write)\s+(.+)$").ok();
        if let Some(re) = and_re {
            if let Some(cap) = re.captures(task) {
                if let Some(items) = cap.get(1) {
                    let items_str = items.as_str();
                    // Split by " and " or ", "
                    for item in items_str.split(" and ").flat_map(|s| s.split(", ")) {
                        let item = item.trim();
                        if !item.is_empty() {
                            goals.push(item.to_string());
                        }
                    }
                }
            }
        }
    }

    // If still no goals, use the whole task as a single goal
    if goals.is_empty() && !task.trim().is_empty() {
        goals.push(task.trim().to_string());
    }

    goals
}

// =============================================================================
// Context Management Commands
// =============================================================================

fn handle_context(command: ContextCommands) -> Result<()> {
    use commander_agent::{
        context_manager::model_contexts, ContextAction, ContextManager, ContextStrategy,
    };

    match command {
        ContextCommands::Status => {
            println!("Context Management Status");
            println!("=========================\n");

            println!("Default Model Contexts:");
            println!("  Claude 3.5 Sonnet: {} tokens", model_contexts::CLAUDE_3_5_SONNET);
            println!("  Claude 3 Haiku:    {} tokens", model_contexts::CLAUDE_3_HAIKU);
            println!("  Claude 3 Opus:     {} tokens", model_contexts::CLAUDE_3_OPUS);
            println!("  GPT-4 Turbo:       {} tokens", model_contexts::GPT_4_TURBO);
            println!("  Default:           {} tokens", model_contexts::DEFAULT);

            println!("\nThresholds (default):");
            println!("  Warning:  20% remaining");
            println!("  Critical: 10% remaining");

            println!("\nStrategies by Adapter:");
            println!("  claude-code: Compaction (summarize old messages)");
            println!("  mpm:         Pause/Resume (save state, resume later)");
            println!("  generic:     Warn and Continue");
        }

        ContextCommands::Check { usage } => {
            let usage = usage.min(100);
            let remaining = 100 - usage;

            println!("Context Check Simulation");
            println!("========================\n");
            println!("Simulated usage: {}%", usage);
            println!("Remaining: {}%\n", remaining);

            // Check against default thresholds
            let manager =
                ContextManager::new(ContextStrategy::Compaction, model_contexts::CLAUDE_3_5_SONNET);

            let tokens_used = (model_contexts::CLAUDE_3_5_SONNET as f32 * (usage as f32 / 100.0)) as usize;
            let mut test_manager = ContextManager::new(ContextStrategy::Compaction, model_contexts::CLAUDE_3_5_SONNET);
            let action = test_manager.update(tokens_used);

            println!("Thresholds:");
            println!("  Warning:  {}%", (manager.warning_threshold() * 100.0) as u32);
            println!("  Critical: {}%", (manager.critical_threshold() * 100.0) as u32);

            println!("\nAction that would be taken:");
            match action {
                ContextAction::Continue => {
                    println!("  Continue - Context is healthy");
                }
                ContextAction::Warn { remaining_percent } => {
                    println!(
                        "  Warn - {:.1}% remaining, approaching critical",
                        remaining_percent * 100.0
                    );
                }
                ContextAction::Critical { action } => {
                    println!("  Critical - Immediate action required:");
                    match action {
                        commander_agent::CriticalAction::Compact { messages_to_summarize } => {
                            println!("    -> Compact {} messages", messages_to_summarize);
                        }
                        commander_agent::CriticalAction::Pause { command, .. } => {
                            println!("    -> Execute pause command: {}", command);
                        }
                        commander_agent::CriticalAction::Alert { message } => {
                            println!("    -> Alert: {}", message);
                        }
                    }
                }
            }
        }

        ContextCommands::Strategy { adapter } => {
            use commander_agent::template::{AdapterType, TemplateRegistry};

            let adapter_type: AdapterType = adapter.parse().map_err(|e| {
                format!(
                    "Invalid adapter type '{}': {}. Valid types: claude-code, mpm, generic",
                    adapter, e
                )
            })?;

            let registry = TemplateRegistry::new();

            if let Some(template) = registry.get(&adapter_type) {
                println!("Context Strategy for {:?}", adapter_type);
                println!("================================\n");

                match &template.context_strategy {
                    Some(ContextStrategy::PauseResume {
                        pause_command,
                        resume_command,
                    }) => {
                        println!("Strategy: Pause/Resume");
                        println!("Pause Command:  {}", pause_command);
                        println!("Resume Command: {}", resume_command);
                        println!("\nBehavior:");
                        println!("  When context < 10%:");
                        println!("    1. Execute pause command to save state");
                        println!("    2. Summarize current work and remaining tasks");
                        println!("    3. Provide resume instructions");
                        println!("  When resuming:");
                        println!("    1. Execute resume command to load state");
                        println!("    2. Review saved context");
                        println!("    3. Continue from where left off");
                    }
                    Some(ContextStrategy::Compaction) => {
                        println!("Strategy: Compaction");
                        println!("\nBehavior:");
                        println!("  When context < 10%:");
                        println!("    1. Summarize older messages");
                        println!("    2. Keep recent messages in full detail");
                        println!("    3. Preserve key facts and decisions");
                        println!("    4. Continue without interruption");
                    }
                    Some(ContextStrategy::WarnAndContinue) => {
                        println!("Strategy: Warn and Continue");
                        println!("\nBehavior:");
                        println!("  When context < 10%:");
                        println!("    1. Display warning to user");
                        println!("    2. Suggest starting a new session");
                        println!("    3. Continue with reduced context");
                    }
                    None => {
                        println!("Strategy: None (default behavior)");
                        println!("\nNo specific context strategy configured.");
                    }
                }

                println!("\nMemory Categories:");
                for cat in &template.memory_categories {
                    println!("  - {}", cat);
                }
            } else {
                return Err(format!("No template found for adapter type: {}", adapter).into());
            }
        }
    }

    Ok(())
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

    #[test]
    fn test_extract_goals_numbered_list() {
        let task = "1. Create login form\n2. Add OAuth2\n3. Write tests";
        let goals = extract_goals_from_task(task);
        assert_eq!(goals.len(), 3);
        assert_eq!(goals[0], "Create login form");
        assert_eq!(goals[1], "Add OAuth2");
        assert_eq!(goals[2], "Write tests");
    }

    #[test]
    fn test_extract_goals_bullet_list() {
        let task = "- Add user model\n- Add authentication\n- Add tests";
        let goals = extract_goals_from_task(task);
        assert_eq!(goals.len(), 3);
        assert_eq!(goals[0], "Add user model");
    }

    #[test]
    fn test_extract_goals_implement_pattern() {
        let task = "Implement login and signup and logout";
        let goals = extract_goals_from_task(task);
        assert_eq!(goals.len(), 3);
        assert!(goals.contains(&"login".to_string()));
        assert!(goals.contains(&"signup".to_string()));
        assert!(goals.contains(&"logout".to_string()));
    }

    #[test]
    fn test_extract_goals_single_task() {
        let task = "Fix the authentication bug";
        let goals = extract_goals_from_task(task);
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0], "Fix the authentication bug");
    }

    #[test]
    fn test_extract_goals_empty() {
        let task = "";
        let goals = extract_goals_from_task(task);
        assert!(goals.is_empty());
    }
}
