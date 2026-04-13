# Research: Claude Max Plan Usage Tracking & Terminal-Based Browser Login

**Date:** 2026-04-12

---

## 1. Claude Max Plan Usage Tracking

### What `claude auth status` Exposes

Running `claude auth status` returns a JSON object that includes `subscriptionType`:

```json
{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "email": "...",
  "orgId": "...",
  "orgName": "...",
  "subscriptionType": "max"
}
```

This is the only plan/tier signal available from the CLI. No separate `claude usage` or billing command exists.

### CLI Flags for Budget

`--max-budget-usd <amount>` limits spending per `--print` invocation. There is no command that queries remaining budget or monthly usage.

### Stream-JSON Cost Data

The `--output-format stream-json` stream emits a `result` line with `cost_usd` per run:

```json
{"type":"result","subtype":"success","result":"...","cost_usd":0.003,"duration_ms":1200}
```

The parser in `crates/mpm-sdk/src/parser.rs` already captures this field in `AgentResult.cost_usd: Option<f64>`. There is no `usage_tokens`, `rate_limit`, or quota field in the stream.

### Rate Limit Headers

The Anthropic REST API returns `x-ratelimit-*` HTTP headers, but Claude Code runs as a subprocess — ai-commander never sees HTTP headers. No rate-limit header parsing exists in the codebase.

### Existing Usage Code in ai-commander

- `crates/mpm-sdk/src/types.rs`: `AgentResult.cost_usd` (per-run USD cost from stream)
- `crates/mpm-sdk/src/serve_client.rs`: `get_context()` hits `/api/v1/sessions/{id}/context` returning `SessionContext { tokens_used, tokens_total }` — this is a claude-mpm serve endpoint (not Anthropic's API)
- No quota, billing, or rate-limit tracking exists beyond these

### Verdict

The only programmatically accessible usage data is:
1. `subscriptionType: "max"` from `claude auth status --json` (or parse the JSON from `claude auth status`)
2. `cost_usd` per run from the stream-json `result` line (already parsed)
3. No token-level usage or quota available without direct Anthropic API key calls

---

## 2. Terminal-Based Browser Login

### `claude auth login` Options

```
claude auth login --claudeai    # Default: opens browser to claude.ai OAuth
claude auth login --console     # Anthropic Console (API key billing)
claude auth login --sso         # Force SSO flow
claude auth login --email <e>   # Pre-populate email in browser
```

All flows open a browser. No device-code / no-browser option exists.

### Does Claude Code Support OAuth Device Flow?

No. There is no `--device-flow`, `--no-browser`, or code-entry option. The login always requires a browser to be opened.

### Terminal Browser Options

| Tool | Feasibility | Notes |
|------|-------------|-------|
| `w3m` | Unlikely | Cannot execute JavaScript; OAuth requires JS |
| `lynx` | Unlikely | Same — no JS support |
| `links2` | Unlikely | Minimal JS; OAuth redirect chains will fail |
| `browsh` | Possible | Renders Firefox in terminal; full JS support |

`browsh` is the only realistic option since it wraps a full Firefox instance. However it requires Firefox installed and is complex to set up in headless server environments.

### Practical Alternatives

1. **`setup-token` command**: `claude setup-token` generates a long-lived auth token from a Claude subscription — this may be the best path for non-interactive/server scenarios. Requires one-time browser login, then the token can be stored.

2. **`ANTHROPIC_API_KEY` env var**: With `--bare` flag, Claude Code skips OAuth entirely and uses `ANTHROPIC_API_KEY` directly. No browser needed.

3. **Remote port forward**: On a headless server, `ssh -L 8080:localhost:8080` + open browser locally during `claude auth login` (the OAuth redirect goes to localhost).

### Existing Auth Code in ai-commander

- `crates/commander-core/src/config.rs`: manages `authorized_chats.json` for Telegram (not Claude auth)
- `crates/commander-core/src/onboarding.rs`: handles `openrouter_api_key` setup
- No Claude OAuth or login flow code exists — auth is entirely delegated to the `claude` subprocess

---

## Summary

| Question | Answer |
|----------|--------|
| `claude usage` command? | Does not exist |
| Plan tier available? | Yes, via `claude auth status` JSON (`subscriptionType`) |
| Cost per run? | Yes, `cost_usd` in stream-json result (already parsed) |
| Rate limit headers? | Not accessible (subprocess model) |
| Existing usage tracking? | `cost_usd` only; no quota/rate-limit code |
| Browser-free login? | Use `ANTHROPIC_API_KEY` + `--bare`, or `setup-token` once |
| OAuth device flow? | Not supported by Claude Code |
| Terminal browser? | `browsh` is only viable option (needs Firefox) |
