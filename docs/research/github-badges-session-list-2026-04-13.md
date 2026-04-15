# GitHub Issue/PR Badges in Session List

**Date:** 2026-04-13
**Status:** Research complete
**Feature:** Poll each project's GitHub repo once per hour, show badges with open issue/PR counts in the session list.

---

## 1. Project Model -- No GitHub Repo Info

The `Project` struct in `crates/commander-models/src/project.rs` (line 189-259) has **no GitHub repo URL or remote info**. Relevant fields:

- `path: String` -- project directory path
- `name: String` -- project name
- `config: HashMap<String, serde_json::Value>` -- generic config map

The `path` field gives us the project directory, which is sufficient to infer the GitHub remote from `.git/config`.

**Decision:** Do NOT add a `github_repo` field to `Project`. Instead, derive `owner/repo` at runtime from the project's `path` by reading its git remote. This avoids schema changes and keeps the model clean.

## 2. Extracting owner/repo from Git Remotes

**Approach:** Shell out to `git -C <project.path> remote get-url origin` (or parse `.git/config`).

**URL formats to handle:**

| Format | Example | Extraction |
|--------|---------|------------|
| SSH | `git@github.com:owner/repo.git` | Split on `:`, strip `.git` |
| HTTPS | `https://github.com/owner/repo.git` | Parse URL path, strip `.git` |
| HTTPS (no .git) | `https://github.com/owner/repo` | Parse URL path |

**Rust implementation sketch:**

```rust
fn extract_github_owner_repo(remote_url: &str) -> Option<(String, String)> {
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = remote_url.strip_prefix("git@github.com:") {
        let path = rest.trim_end_matches(".git");
        let parts: Vec<&str> = path.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    // HTTPS: https://github.com/owner/repo.git
    if remote_url.contains("github.com/") {
        let after = remote_url.split("github.com/").nth(1)?;
        let path = after.trim_end_matches(".git").trim_end_matches('/');
        let parts: Vec<&str> = path.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}
```

**Best location:** A utility function in `commander-core` or a new small module. Use `tokio::process::Command` to run `git -C <path> remote get-url origin` asynchronously.

## 3. API Endpoint Design

**Current router:** `crates/commander-api/src/router.rs`

**AppState** (`crates/commander-api/src/state.rs`) holds:
- `projects: Arc<RwLock<HashMap<String, Project>>>` -- in-memory project store
- No GitHub-related caching

**Proposed additions:**

### New endpoint

```
GET /api/projects/{id}/github-stats
```

Response:
```json
{
  "owner": "bob",
  "repo": "ai-commander",
  "open_issues": 12,
  "open_prs": 3,
  "last_fetched": "2026-04-13T10:00:00Z"
}
```

### Bulk endpoint (for session list)

```
GET /api/github-stats
```

Response:
```json
{
  "stats": {
    "proj-abc123": { "owner": "bob", "repo": "ai-commander", "open_issues": 12, "open_prs": 3 },
    "proj-def456": null
  }
}
```

### Caching in AppState

Add to `AppState`:
```rust
pub github_stats: Arc<RwLock<HashMap<String, GitHubStats>>>,
```

Where `GitHubStats`:
```rust
struct GitHubStats {
    owner: String,
    repo: String,
    open_issues: u32,
    open_prs: u32,
    last_fetched: DateTime<Utc>,
}
```

### Background polling

Spawn a `tokio::spawn` task in `serve()` that runs every hour:
1. Iterate all projects
2. For each project with a GitHub remote, call `gh api repos/{owner}/{repo}` or use the REST API
3. Update the `github_stats` cache

**API call (using `gh` CLI):**
```bash
gh api repos/{owner}/{repo} --jq '{open_issues: .open_issues_count}'
gh api repos/{owner}/{repo}/pulls?state=open --jq 'length'
```

**Or direct HTTP (no `gh` dependency):**
```
GET https://api.github.com/repos/{owner}/{repo}
  -> .open_issues_count (includes PRs -- GitHub counts PRs as issues)

GET https://api.github.com/search/issues?q=repo:{owner}/{repo}+type:issue+state:open
  -> .total_count

GET https://api.github.com/search/issues?q=repo:{owner}/{repo}+type:pr+state:open
  -> .total_count
```

**Recommendation:** Use the GitHub REST API directly via `reqwest` (already in the workspace likely). This avoids requiring `gh` CLI on the server. No auth needed for public repos; for private repos, read `GITHUB_TOKEN` from env.

## 4. SessionList UI -- Badge Placement

The SessionList component is at `crates/commander-gui/ui/src/lib/components/SessionList.svelte` (565 lines).

**Key rendering location** (lines 231-238): each session row has:
```svelte
<button class="session-main" on:click={() => connect(session.name)}>
  <span class="status-dot" class:active={...}></span>
  <span class="session-name">{getDisplayName(session.name)}</span>
  <Activity size={16} ... />
</button>
```

**Badge placement:** Add badges between the session name and the Activity icon:

```svelte
<button class="session-main" on:click={() => connect(session.name)}>
  <span class="status-dot" class:active={...}></span>
  <span class="session-name">{getDisplayName(session.name)}</span>
  
  {#if githubStats[session.name]}
    <span class="github-badges">
      <span class="badge issue-badge" title="Open issues">
        {githubStats[session.name].open_issues}
      </span>
      <span class="badge pr-badge" title="Open PRs">
        {githubStats[session.name].open_prs}
      </span>
    </span>
  {/if}
  
  <Activity size={16} ... />
</button>
```

**For the web UI** (`crates/commander-gui/ui/dist-web/`), the same component is used, but the data source would be the REST API (`/api/github-stats`) instead of Tauri invoke.

**Note:** The Tauri desktop app uses `invoke('list_sessions')` while the web UI uses fetch-based API calls. The GitHub stats would need:
- A Tauri command (`get_github_stats`) for the desktop app
- The `/api/github-stats` endpoint for the web UI

## 5. Existing GitHub Tooling

**No `gh` CLI usage found** in the Rust codebase. The grep results show "github" references only in:
- `change_detector` (detecting GitHub-related changes in code)
- `notification_parser` (parsing notification content)
- `structured_summarizer` (summarizing changes)
- Various agent prompts (referencing GitHub conceptually)

None of these invoke `gh` or call the GitHub API. This would be a **new integration**.

## Summary of Recommendations

| Question | Answer |
|----------|--------|
| Project has GitHub info? | No. Derive from `git remote get-url origin` at runtime |
| Best extraction approach | `tokio::process::Command` running `git`, then parse URL |
| API endpoint | `GET /api/github-stats` (bulk) with hourly background polling |
| State caching | `Arc<RwLock<HashMap<String, GitHubStats>>>` in AppState |
| GitHub API access | Direct `reqwest` HTTP to `api.github.com` (no `gh` dependency) |
| UI badge location | Between session name and Activity icon in SessionList.svelte |
| Scope estimate | Small-medium: ~300-400 lines across 4-5 files |

### Implementation order

1. `commander-core`: Add `extract_github_owner_repo()` utility
2. `commander-api/src/state.rs`: Add `github_stats` cache to AppState
3. `commander-api`: Add background polling task + `/api/github-stats` endpoint
4. `commander-gui`: Add Tauri command wrapper + update SessionList.svelte with badges
5. Web UI: Wire up `/api/github-stats` fetch in the web client's session list
