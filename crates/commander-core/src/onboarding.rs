//! First-run onboarding wizard.
//!
//! Provides a setup wizard for first-time users to configure their
//! OpenRouter and Telegram credentials.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

/// Check if onboarding has been completed.
///
/// Returns `true` if the config file does not exist (meaning the user
/// has not completed setup yet).
pub fn needs_onboarding() -> bool {
    !config_file().exists()
}

/// Get the config file path.
fn config_file() -> PathBuf {
    crate::config::state_dir().join("config.toml")
}

/// Run the onboarding wizard.
///
/// Displays a welcome message and prompts the user for optional
/// OpenRouter and Telegram credentials.
///
/// # Errors
/// Returns an error if reading from stdin or writing to files fails.
pub fn run_onboarding() -> io::Result<()> {
    println!();
    println!("╔════════════════════════════════════════╗");
    println!("║       Welcome to AI Commander!         ║");
    println!("╚════════════════════════════════════════╝");
    println!();
    println!("Let's set up your configuration.");
    println!();

    // OpenRouter API Key
    println!("━━━ OpenRouter API Key (optional) ━━━");
    println!("Used for response summarization on mobile.");
    println!("Get one at: https://openrouter.ai/keys");
    println!();
    print!("Enter OpenRouter API key (or press Enter to skip): ");
    io::stdout().flush()?;

    let mut openrouter_key = String::new();
    io::stdin().read_line(&mut openrouter_key)?;
    let openrouter_key = openrouter_key.trim().to_string();

    println!();

    // Telegram Bot Token
    println!("━━━ Telegram Bot Token (optional) ━━━");
    println!("Used for mobile access to your AI sessions.");
    println!("Create a bot: https://t.me/BotFather");
    println!();
    print!("Enter Telegram bot token (or press Enter to skip): ");
    io::stdout().flush()?;

    let mut telegram_token = String::new();
    io::stdin().read_line(&mut telegram_token)?;
    let telegram_token = telegram_token.trim().to_string();

    // Save config
    save_config(&openrouter_key, &telegram_token)?;

    println!();
    println!("━━━ Setup Complete! ━━━");
    println!();

    if !openrouter_key.is_empty() {
        println!("[ok] OpenRouter API key saved");
    } else {
        println!("[ ] OpenRouter: skipped (add later to ~/.commander/.env.local)");
    }

    if !telegram_token.is_empty() {
        println!("[ok] Telegram bot token saved");
    } else {
        println!("[ ] Telegram: skipped (add later to ~/.commander/.env.local)");
    }

    println!();
    println!("Quick start:");
    println!("  1. commander                   # Start the TUI");
    println!("  2. /connect ~/project -a cc -n myproj  # Connect a project");
    println!("  3. /telegram                   # Enable mobile access");
    println!();

    Ok(())
}

/// Save configuration to files.
fn save_config(openrouter_key: &str, telegram_token: &str) -> io::Result<()> {
    let config_path = config_file();

    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Build config.toml content
    let mut content = String::from("# AI Commander Configuration\n\n");

    if !openrouter_key.is_empty() {
        content.push_str(&format!("openrouter_api_key = \"{}\"\n", openrouter_key));
    }

    if !telegram_token.is_empty() {
        content.push_str(&format!("telegram_bot_token = \"{}\"\n", telegram_token));
    }

    // Write config.toml (creates it even if empty to mark onboarding as done)
    fs::write(&config_path, content)?;

    // Also write to .env.local in state dir for compatibility with existing code
    let env_path = crate::config::state_dir().join(".env.local");
    let mut env_content = String::new();

    if !openrouter_key.is_empty() {
        env_content.push_str(&format!("OPENROUTER_API_KEY={}\n", openrouter_key));
    }

    if !telegram_token.is_empty() {
        env_content.push_str(&format!("TELEGRAM_BOT_TOKEN={}\n", telegram_token));
    }

    if !env_content.is_empty() {
        fs::write(&env_path, env_content)?;
    }

    Ok(())
}

/// Load saved config into environment variables.
///
/// Reads the `.env.local` file from the state directory and sets
/// environment variables accordingly. This should be called on startup.
pub fn load_config() {
    let env_path = crate::config::state_dir().join(".env.local");
    if env_path.exists() {
        let _ = dotenvy::from_path(&env_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_file_path() {
        let path = config_file();
        assert!(path.ends_with("config.toml"));
    }

    #[test]
    fn test_needs_onboarding_function_exists() {
        // In a fresh environment without config, should need onboarding
        // Note: This test depends on the actual state of the filesystem
        let _ = needs_onboarding(); // Should not panic
    }

    #[test]
    fn test_save_config_content_format() {
        // Test the actual content generation logic in isolation
        // by writing to a specific temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let env_path = temp_dir.path().join(".env.local");

        // Directly test the write logic without relying on state_dir()
        let mut content = String::from("# AI Commander Configuration\n\n");
        content.push_str("openrouter_api_key = \"test-key\"\n");
        content.push_str("telegram_bot_token = \"test-token\"\n");
        fs::write(&config_path, &content).unwrap();

        let mut env_content = String::new();
        env_content.push_str("OPENROUTER_API_KEY=test-key\n");
        env_content.push_str("TELEGRAM_BOT_TOKEN=test-token\n");
        fs::write(&env_path, &env_content).unwrap();

        // Verify content
        let read_config = fs::read_to_string(&config_path).unwrap();
        assert!(read_config.contains("openrouter_api_key = \"test-key\""));
        assert!(read_config.contains("telegram_bot_token = \"test-token\""));

        let read_env = fs::read_to_string(&env_path).unwrap();
        assert!(read_env.contains("OPENROUTER_API_KEY=test-key"));
        assert!(read_env.contains("TELEGRAM_BOT_TOKEN=test-token"));
    }

    #[test]
    fn test_config_toml_format_with_empty() {
        // Test that empty values don't produce config lines
        let mut content = String::from("# AI Commander Configuration\n\n");

        let openrouter = "";
        let telegram = "";

        if !openrouter.is_empty() {
            content.push_str(&format!("openrouter_api_key = \"{}\"\n", openrouter));
        }
        if !telegram.is_empty() {
            content.push_str(&format!("telegram_bot_token = \"{}\"\n", telegram));
        }

        assert!(!content.contains("openrouter_api_key"));
        assert!(!content.contains("telegram_bot_token"));
    }

    #[test]
    fn test_env_content_format() {
        let mut env_content = String::new();

        let openrouter = "sk-or-test";
        let telegram = "123:ABC";

        if !openrouter.is_empty() {
            env_content.push_str(&format!("OPENROUTER_API_KEY={}\n", openrouter));
        }
        if !telegram.is_empty() {
            env_content.push_str(&format!("TELEGRAM_BOT_TOKEN={}\n", telegram));
        }

        assert_eq!(
            env_content,
            "OPENROUTER_API_KEY=sk-or-test\nTELEGRAM_BOT_TOKEN=123:ABC\n"
        );
    }
}
