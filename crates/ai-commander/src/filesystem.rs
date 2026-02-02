//! Filesystem operations for Commander.
//!
//! Provides natural language filesystem commands that Commander can execute directly.

use std::fs;
use std::path::{Path, PathBuf};

/// Result of a filesystem operation.
#[derive(Debug)]
pub struct FsResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Output message
    pub message: String,
    /// Additional details (file contents, listings, etc.)
    pub details: Option<String>,
}

impl FsResult {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            details: None,
        }
    }

    fn ok_with_details(message: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            details: Some(details.into()),
        }
    }

    fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            details: None,
        }
    }
}

/// Parsed filesystem command.
#[derive(Debug, PartialEq)]
pub enum FsCommand {
    /// List directory contents
    List { path: String, recursive: bool },
    /// Read file contents
    Read { path: String, lines: Option<(usize, usize)> },
    /// Write content to file
    Write { path: String, content: String, append: bool },
    /// Create a new file
    Create { path: String },
    /// Move/rename file or directory
    Move { from: String, to: String },
    /// Copy file or directory
    Copy { from: String, to: String },
    /// Delete file or directory
    Delete { path: String, force: bool },
    /// Create directory
    Mkdir { path: String, parents: bool },
    /// Search for files by pattern
    Search { pattern: String, path: Option<String> },
    /// Show file info (size, modified, etc.)
    Info { path: String },
    /// Show current working directory
    Pwd,
}

/// Parse a natural language command into a filesystem operation.
pub fn parse_command(input: &str, working_dir: &Path) -> Option<FsCommand> {
    let input = input.trim();
    let words: Vec<&str> = input.split_whitespace().collect();

    if words.is_empty() {
        return None;
    }

    let cmd = words[0].to_lowercase();

    // List/ls commands
    if matches!(cmd.as_str(), "ls" | "list" | "dir") {
        let path = words.get(1).map(|s| resolve_path(s, working_dir)).unwrap_or_else(|| ".".to_string());
        let recursive = words.iter().any(|w| w.to_lowercase() == "-r" || w.to_lowercase() == "--recursive" || w.to_lowercase() == "recursively");
        return Some(FsCommand::List { path, recursive });
    }

    // Cat/read commands
    if matches!(cmd.as_str(), "cat" | "read" | "show" | "view" | "type") {
        if words.len() > 1 {
            let path = resolve_path(words[1], working_dir);
            return Some(FsCommand::Read { path, lines: None });
        }
    }

    // Head command
    if cmd.as_str() == "head" {
        if words.len() > 1 {
            let path = resolve_path(words.last().unwrap(), working_dir);
            let n = words.iter()
                .find(|w| w.starts_with('-') && w[1..].parse::<usize>().is_ok())
                .and_then(|w| w[1..].parse::<usize>().ok())
                .unwrap_or(10);
            return Some(FsCommand::Read { path, lines: Some((0, n)) });
        }
    }

    // Tail command
    if cmd.as_str() == "tail" {
        if words.len() > 1 {
            let path = resolve_path(words.last().unwrap(), working_dir);
            // Tail needs special handling - we'll read all and take last N
            return Some(FsCommand::Read { path, lines: Some((usize::MAX, 10)) });
        }
    }

    // Write/echo commands
    if cmd.as_str() == "echo" && words.iter().any(|w| *w == ">" || *w == ">>") {
        let parts: Vec<&str> = input.splitn(2, '>').collect();
        if parts.len() == 2 {
            let content = parts[0].trim_start_matches("echo").trim().trim_matches('"').trim_matches('\'');
            let append = parts[1].starts_with('>');
            let path_str = parts[1].trim_start_matches('>').trim();
            let path = resolve_path(path_str, working_dir);
            return Some(FsCommand::Write {
                path,
                content: content.to_string(),
                append
            });
        }
    }

    // Touch/create commands
    if matches!(cmd.as_str(), "touch" | "create") {
        if words.len() > 1 {
            let path = resolve_path(words[1], working_dir);
            return Some(FsCommand::Create { path });
        }
    }

    // Move/rename commands
    if matches!(cmd.as_str(), "mv" | "move" | "rename") {
        if words.len() > 2 {
            let from = resolve_path(words[1], working_dir);
            let to = resolve_path(words[2], working_dir);
            return Some(FsCommand::Move { from, to });
        }
    }

    // Copy commands
    if matches!(cmd.as_str(), "cp" | "copy") {
        if words.len() > 2 {
            let from = resolve_path(words[1], working_dir);
            let to = resolve_path(words[2], working_dir);
            return Some(FsCommand::Copy { from, to });
        }
    }

    // Delete commands
    if matches!(cmd.as_str(), "rm" | "delete" | "remove" | "del") {
        if words.len() > 1 {
            let force = words.iter().any(|w| w.to_lowercase() == "-f" || w.to_lowercase() == "--force" || w.to_lowercase() == "-rf");
            let path = resolve_path(words.last().unwrap(), working_dir);
            return Some(FsCommand::Delete { path, force });
        }
    }

    // Mkdir commands
    if matches!(cmd.as_str(), "mkdir" | "makedir") {
        if words.len() > 1 {
            let parents = words.iter().any(|w| w.to_lowercase() == "-p" || w.to_lowercase() == "--parents");
            let path = resolve_path(words.last().unwrap(), working_dir);
            return Some(FsCommand::Mkdir { path, parents });
        }
    }

    // Search/find commands
    if matches!(cmd.as_str(), "find" | "search" | "glob") {
        if words.len() > 1 {
            let pattern = words[1].to_string();
            let search_path = words.get(2).map(|s| resolve_path(s, working_dir));
            return Some(FsCommand::Search { pattern, path: search_path });
        }
    }

    // File info commands
    if matches!(cmd.as_str(), "stat" | "info" | "file") {
        if words.len() > 1 {
            let path = resolve_path(words[1], working_dir);
            return Some(FsCommand::Info { path });
        }
    }

    // Pwd command
    if matches!(cmd.as_str(), "pwd" | "cwd" | "whereami") {
        return Some(FsCommand::Pwd);
    }

    // Natural language patterns
    if input.starts_with("list ") || input.starts_with("show files") || input.starts_with("what's in") {
        let path = extract_path_from_natural(&input, working_dir).unwrap_or_else(|| ".".to_string());
        return Some(FsCommand::List { path, recursive: false });
    }

    if input.starts_with("read ") || input.starts_with("show me ") || input.starts_with("what's in the file") {
        if let Some(path) = extract_path_from_natural(&input, working_dir) {
            return Some(FsCommand::Read { path, lines: None });
        }
    }

    if input.starts_with("create ") && (input.contains("file") || input.contains("directory") || input.contains("folder")) {
        if let Some(path) = extract_path_from_natural(&input, working_dir) {
            if input.contains("directory") || input.contains("folder") {
                return Some(FsCommand::Mkdir { path, parents: true });
            } else {
                return Some(FsCommand::Create { path });
            }
        }
    }

    if input.starts_with("delete ") || input.starts_with("remove ") {
        if let Some(path) = extract_path_from_natural(&input, working_dir) {
            return Some(FsCommand::Delete { path, force: false });
        }
    }

    if input.starts_with("find ") || input.starts_with("search for ") {
        let pattern = input
            .trim_start_matches("find ")
            .trim_start_matches("search for ")
            .trim_start_matches("files named ")
            .trim_start_matches("files matching ")
            .split_whitespace()
            .next()
            .unwrap_or("*")
            .to_string();
        return Some(FsCommand::Search { pattern, path: None });
    }

    None
}

/// Extract a path from natural language input.
fn extract_path_from_natural(input: &str, working_dir: &Path) -> Option<String> {
    // Look for quoted paths
    if let Some(start) = input.find('"') {
        if let Some(end) = input[start + 1..].find('"') {
            let path = &input[start + 1..start + 1 + end];
            return Some(resolve_path(path, working_dir));
        }
    }
    if let Some(start) = input.find('\'') {
        if let Some(end) = input[start + 1..].find('\'') {
            let path = &input[start + 1..start + 1 + end];
            return Some(resolve_path(path, working_dir));
        }
    }

    // Look for path-like strings
    for word in input.split_whitespace() {
        if word.contains('/') || word.contains('.') && !word.starts_with("file") {
            return Some(resolve_path(word, working_dir));
        }
    }

    None
}

/// Resolve a path relative to working directory.
fn resolve_path(path: &str, working_dir: &Path) -> String {
    let expanded = shellexpand::tilde(path).to_string();
    let path = Path::new(&expanded);

    if path.is_absolute() {
        expanded
    } else {
        working_dir.join(path).to_string_lossy().to_string()
    }
}

/// Execute a filesystem command.
pub fn execute(cmd: &FsCommand, working_dir: &Path) -> FsResult {
    match cmd {
        FsCommand::Pwd => {
            FsResult::ok(working_dir.to_string_lossy().to_string())
        }

        FsCommand::List { path, recursive } => {
            let target = Path::new(path);
            if !target.exists() {
                return FsResult::err(format!("Path not found: {}", path));
            }

            if *recursive {
                list_recursive(target, 0)
            } else {
                list_directory(target)
            }
        }

        FsCommand::Read { path, lines } => {
            let target = Path::new(path);
            if !target.exists() {
                return FsResult::err(format!("File not found: {}", path));
            }
            if !target.is_file() {
                return FsResult::err(format!("Not a file: {}", path));
            }

            match fs::read_to_string(target) {
                Ok(content) => {
                    let output = match lines {
                        Some((0, n)) => {
                            // Head: first N lines
                            content.lines().take(*n).collect::<Vec<_>>().join("\n")
                        }
                        Some((usize::MAX, n)) => {
                            // Tail: last N lines
                            let all_lines: Vec<_> = content.lines().collect();
                            let start = all_lines.len().saturating_sub(*n);
                            all_lines[start..].join("\n")
                        }
                        Some((start, end)) => {
                            content.lines().skip(*start).take(*end).collect::<Vec<_>>().join("\n")
                        }
                        None => content,
                    };
                    FsResult::ok_with_details(format!("Contents of {}", path), output)
                }
                Err(e) => FsResult::err(format!("Failed to read {}: {}", path, e)),
            }
        }

        FsCommand::Write { path, content, append } => {
            let target = Path::new(path);

            // Ensure parent directory exists
            if let Some(parent) = target.parent() {
                if !parent.exists() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        return FsResult::err(format!("Failed to create directory: {}", e));
                    }
                }
            }

            let result = if *append {
                use std::io::Write;
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(target)
                    .and_then(|mut f| writeln!(f, "{}", content))
            } else {
                fs::write(target, content).map(|_| ())
            };

            match result {
                Ok(_) => FsResult::ok(format!("Wrote to {}", path)),
                Err(e) => FsResult::err(format!("Failed to write {}: {}", path, e)),
            }
        }

        FsCommand::Create { path } => {
            let target = Path::new(path);

            if target.exists() {
                return FsResult::ok(format!("File already exists: {}", path));
            }

            // Ensure parent directory exists
            if let Some(parent) = target.parent() {
                if !parent.exists() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        return FsResult::err(format!("Failed to create directory: {}", e));
                    }
                }
            }

            match fs::File::create(target) {
                Ok(_) => FsResult::ok(format!("Created {}", path)),
                Err(e) => FsResult::err(format!("Failed to create {}: {}", path, e)),
            }
        }

        FsCommand::Move { from, to } => {
            let source = Path::new(from);
            let dest = Path::new(to);

            if !source.exists() {
                return FsResult::err(format!("Source not found: {}", from));
            }

            match fs::rename(source, dest) {
                Ok(_) => FsResult::ok(format!("Moved {} → {}", from, to)),
                Err(e) => FsResult::err(format!("Failed to move: {}", e)),
            }
        }

        FsCommand::Copy { from, to } => {
            let source = Path::new(from);
            let dest = Path::new(to);

            if !source.exists() {
                return FsResult::err(format!("Source not found: {}", from));
            }

            if source.is_dir() {
                match copy_dir_recursive(source, dest) {
                    Ok(_) => FsResult::ok(format!("Copied {} → {}", from, to)),
                    Err(e) => FsResult::err(format!("Failed to copy: {}", e)),
                }
            } else {
                match fs::copy(source, dest) {
                    Ok(_) => FsResult::ok(format!("Copied {} → {}", from, to)),
                    Err(e) => FsResult::err(format!("Failed to copy: {}", e)),
                }
            }
        }

        FsCommand::Delete { path, force } => {
            let target = Path::new(path);

            if !target.exists() {
                return if *force {
                    FsResult::ok(format!("Already deleted: {}", path))
                } else {
                    FsResult::err(format!("Not found: {}", path))
                };
            }

            let result = if target.is_dir() {
                if *force {
                    fs::remove_dir_all(target)
                } else {
                    fs::remove_dir(target)
                }
            } else {
                fs::remove_file(target)
            };

            match result {
                Ok(_) => FsResult::ok(format!("Deleted {}", path)),
                Err(e) => FsResult::err(format!("Failed to delete {}: {}", path, e)),
            }
        }

        FsCommand::Mkdir { path, parents } => {
            let target = Path::new(path);

            if target.exists() {
                return FsResult::ok(format!("Directory already exists: {}", path));
            }

            let result = if *parents {
                fs::create_dir_all(target)
            } else {
                fs::create_dir(target)
            };

            match result {
                Ok(_) => FsResult::ok(format!("Created directory {}", path)),
                Err(e) => FsResult::err(format!("Failed to create directory: {}", e)),
            }
        }

        FsCommand::Search { pattern, path } => {
            let search_path = path.as_ref()
                .map(|p| PathBuf::from(p))
                .unwrap_or_else(|| working_dir.to_path_buf());

            let matches = search_files(&search_path, pattern);

            if matches.is_empty() {
                FsResult::ok(format!("No files matching '{}' found", pattern))
            } else {
                let listing = matches.join("\n");
                FsResult::ok_with_details(
                    format!("Found {} files matching '{}'", matches.len(), pattern),
                    listing
                )
            }
        }

        FsCommand::Info { path } => {
            let target = Path::new(path);

            if !target.exists() {
                return FsResult::err(format!("Not found: {}", path));
            }

            match fs::metadata(target) {
                Ok(meta) => {
                    let file_type = if meta.is_dir() { "directory" } else if meta.is_file() { "file" } else { "other" };
                    let size = meta.len();
                    let modified = meta.modified()
                        .map(|t| format!("{:?}", t))
                        .unwrap_or_else(|_| "unknown".to_string());

                    let info = format!(
                        "Type: {}\nSize: {} bytes\nModified: {}",
                        file_type, size, modified
                    );
                    FsResult::ok_with_details(path.to_string(), info)
                }
                Err(e) => FsResult::err(format!("Failed to get info: {}", e)),
            }
        }
    }
}

/// List directory contents.
fn list_directory(path: &Path) -> FsResult {
    match fs::read_dir(path) {
        Ok(entries) => {
            let mut items: Vec<String> = Vec::new();

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let meta = entry.metadata();

                let prefix = if meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                    "📁 "
                } else {
                    "📄 "
                };

                let size = meta.as_ref()
                    .map(|m| if m.is_file() { format!(" ({} bytes)", m.len()) } else { String::new() })
                    .unwrap_or_default();

                items.push(format!("{}{}{}", prefix, name, size));
            }

            items.sort();

            if items.is_empty() {
                FsResult::ok("Directory is empty")
            } else {
                FsResult::ok_with_details(
                    format!("{} items in {}", items.len(), path.display()),
                    items.join("\n")
                )
            }
        }
        Err(e) => FsResult::err(format!("Failed to list {}: {}", path.display(), e)),
    }
}

/// List directory recursively.
fn list_recursive(path: &Path, depth: usize) -> FsResult {
    let mut items = Vec::new();
    list_recursive_inner(path, depth, &mut items);

    if items.is_empty() {
        FsResult::ok("No items found")
    } else {
        FsResult::ok_with_details(
            format!("{} items found", items.len()),
            items.join("\n")
        )
    }
}

fn list_recursive_inner(path: &Path, depth: usize, items: &mut Vec<String>) {
    let indent = "  ".repeat(depth);

    if let Ok(entries) = fs::read_dir(path) {
        let mut sorted: Vec<_> = entries.flatten().collect();
        sorted.sort_by_key(|e| e.file_name());

        for entry in sorted {
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files
            if name.starts_with('.') {
                continue;
            }

            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    items.push(format!("{}📁 {}/", indent, name));
                    list_recursive_inner(&entry.path(), depth + 1, items);
                } else {
                    items.push(format!("{}📄 {} ({} bytes)", indent, name, meta.len()));
                }
            }
        }
    }
}

/// Copy directory recursively.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Search for files matching a pattern.
fn search_files(base: &Path, pattern: &str) -> Vec<String> {
    let mut matches = Vec::new();
    search_files_inner(base, pattern, &mut matches);
    matches
}

fn search_files_inner(path: &Path, pattern: &str, matches: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden
            if name.starts_with('.') {
                continue;
            }

            // Check if name matches pattern (simple glob)
            if matches_glob(&name, pattern) {
                matches.push(entry.path().to_string_lossy().to_string());
            }

            // Recurse into directories
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    search_files_inner(&entry.path(), pattern, matches);
                }
            }
        }
    }
}

/// Simple glob matching (* and ? wildcards).
fn matches_glob(name: &str, pattern: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();

    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') && !pattern.contains('?') {
        return name.contains(&pattern);
    }

    // Convert glob to simple regex-like matching
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        // No wildcards, just ?
        if pattern.len() != name.len() {
            return false;
        }
        return pattern.chars().zip(name.chars()).all(|(p, n)| p == '?' || p == n);
    }

    // Handle * wildcards
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if let Some(found) = name[pos..].find(part) {
            if i == 0 && found != 0 {
                // First part must match at start
                return false;
            }
            pos += found + part.len();
        } else {
            return false;
        }
    }

    // Last part must match at end if pattern doesn't end with *
    if !pattern.ends_with('*') && !parts.last().unwrap_or(&"").is_empty() {
        return name.ends_with(parts.last().unwrap());
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir() -> PathBuf {
        PathBuf::from("/tmp/test")
    }

    #[test]
    fn test_parse_ls() {
        let cmd = parse_command("ls", &test_dir());
        assert!(matches!(cmd, Some(FsCommand::List { path, recursive: false }) if path == "."));

        let cmd = parse_command("ls src", &test_dir());
        assert!(matches!(cmd, Some(FsCommand::List { path, .. }) if path.contains("src")));
    }

    #[test]
    fn test_parse_cat() {
        let cmd = parse_command("cat README.md", &test_dir());
        assert!(matches!(cmd, Some(FsCommand::Read { path, .. }) if path.contains("README")));
    }

    #[test]
    fn test_parse_mkdir() {
        let cmd = parse_command("mkdir -p src/new", &test_dir());
        assert!(matches!(cmd, Some(FsCommand::Mkdir { parents: true, .. })));
    }

    #[test]
    fn test_parse_mv() {
        let cmd = parse_command("mv old.txt new.txt", &test_dir());
        assert!(matches!(cmd, Some(FsCommand::Move { .. })));
    }

    #[test]
    fn test_parse_find() {
        let cmd = parse_command("find *.rs", &test_dir());
        assert!(matches!(cmd, Some(FsCommand::Search { pattern, .. }) if pattern == "*.rs"));
    }

    #[test]
    fn test_matches_glob() {
        assert!(matches_glob("test.rs", "*.rs"));
        assert!(matches_glob("test.rs", "test.*"));
        assert!(matches_glob("test.rs", "*"));
        assert!(matches_glob("test.rs", "test.rs"));
        assert!(!matches_glob("test.rs", "*.py"));
        assert!(matches_glob("README.md", "readme*"));
    }

    #[test]
    fn test_parse_pwd() {
        let cmd = parse_command("pwd", &test_dir());
        assert!(matches!(cmd, Some(FsCommand::Pwd)));
    }
}
