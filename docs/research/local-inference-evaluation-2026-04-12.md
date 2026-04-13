# Local Inference Engine Evaluation for ai-commander

**Date:** 2026-04-12
**Scope:** kuzu-memory, mcp-vector-search, local LLM availability on host machine

---

## Current Summarization Pipeline

`crates/commander-core/src/summarizer.rs` calls OpenRouter at `https://openrouter.ai/api/v1/chat/completions` with:
- **Tier-1 model:** `anthropic/claude-sonnet-4` (high-confidence structured summaries)
- **Tier-2 model:** `anthropic/claude-haiku-3.5` (fast/cheap mid-confidence summaries, env-overridable via `SUMMARIZER_TIER2_MODEL`)

`crates/commander-agent/src/config.rs` defines `Provider` enum: `OpenRouter` (default), `Anthropic`, `OpenAI`. No local provider variant exists.

---

## Project 1: kuzu-memory (`/Users/masa/Projects/kuzu-memory/`)

**What it is:** Embedded graph-based memory system for AI applications. Uses KuzuDB (embedded graph DB) + HNSW vector search + TF-IDF keyword scoring for memory storage and recall.

**Text generation capability:** None natively. The project is a storage/retrieval system only. It has two optional LLM integrations as thin wrappers:
1. `src/kuzu_memory/integrations/local_llm.py` — detects and routes to Ollama or LM Studio for chat completions (OpenAI-compatible API). The `detect_local_llm()` function probes ports 11434 (Ollama) and 1234 (LM Studio).
2. `src/kuzu_memory/recall/reranker.py` — optional Haiku reranking pass via `anthropic` SDK (opt-in, cloud only).

**Verdict:** Not an inference engine. It is a memory store. It can *call* Ollama if running, but adds no generation capability of its own.

---

## Project 2: mcp-vector-search (`/Users/masa/Projects/mcp-vector-search/`)

**What it is:** Semantic code search tool using LanceDB vector database + sentence-transformer embeddings + AST parsing. Has 17 MCP tools.

**Text generation capability:** Has an `LLMClient` (`src/mcp_vector_search/core/llm_client.py`) that supports OpenAI, OpenRouter, AWS Bedrock, and **Ollama** as providers. The `chat_local` CLI command uses Ollama for code Q&A with tool-calling via XML tags. Default local model: `gemma3:latest`.

**Verdict:** Not an inference engine itself. It wraps Ollama (and cloud APIs) for its chat feature. It is a code-search + retrieval tool, not a general summarization or routing layer.

---

## Local LLM Options on This Machine

Ollama is installed (`/Applications/Ollama.app`) and running. Available models:

| Model | Size |
|-------|------|
| `qwen2.5-coder:7b-instruct` | 4.7 GB |
| `mistral-small3.2:latest` | 15 GB |
| `mistral:latest` | 4.4 GB |
| `qwen2.5:72b` | 47 GB |
| `llama3.1:70b` | 43 GB |
| `codellama:70b` | 39 GB |
| `deepseek-v3.1:latest` | 404 GB (likely quantized) |
| `llama3.1:405b` | 243 GB |
| `hf.co/ilintar/IQuest-Coder-V1-40B-Instruct-GGUF` | 22 GB |

`mlx_lm` is not found on PATH. Apple Silicon MPS acceleration is available via PyTorch (used by mcp-vector-search), but no standalone MLX inference is installed.

---

## Key Answers

**1. Can kuzu-memory or mcp-vector-search do text generation?**
No. Both are retrieval/storage tools. They can call Ollama, but neither is an inference engine.

**2. What local LLM options are available?**
Ollama with multiple models running locally. Best candidates for ai-commander use cases:
- **Summarization:** `qwen2.5:72b` or `mistral-small3.2` (quality vs. latency trade-off). `qwen2.5-coder:7b-instruct` is fastest but likely weaker on general prose.
- **Routing/intent classification:** `qwen2.5-coder:7b-instruct` or `mistral:latest` — fast enough for pre-processing.

**3. What's the current summarization model?**
`anthropic/claude-sonnet-4` via OpenRouter (tier-1), `claude-haiku-3.5` (tier-2).

**4. Could we add a local inference layer using Ollama?**
Yes. Ollama exposes an OpenAI-compatible endpoint at `http://localhost:11434/v1/chat/completions`. The existing `summarizer.rs` HTTP client could be pointed at this with minimal changes. Key considerations:
- Ollama is a background app, not a system service — not reliable in server/daemon contexts.
- Latency: 70B+ models will be slower than OpenRouter for summarization.
- Best fit: a lightweight 7B model for the routing/pre-processing use case, not summarization quality replacement.

**5. Architecture recommendation**

```
User Input
    │
    ▼
[Pre-processor / Router]  ← NEW: commander-core crate, local Ollama 7B
    │                         Intent classification, command routing,
    │                         context extraction from message
    ▼
[Adapter Layer]           ← existing adapters (Telegram, TUI, API)
    │
    ▼
[Claude Code / Session]
    │
    ▼
[Summarizer]              ← OPTION: add Ollama fallback when no OpenRouter key
```

**Proposed crate placement:**
- Add a `LocalProvider` variant to `Provider` enum in `crates/commander-agent/src/config.rs`
- Add `Ollama` as a provider in `summarizer.rs` with the same HTTP client, pointing to `http://localhost:11434/v1/chat/completions`
- New `crates/commander-routing/` (or module within `commander-core`) for the pre-processor that classifies input before adapter dispatch

---

## Feasibility Assessment

| Use Case | Feasibility | Recommended Model | Notes |
|----------|-------------|-------------------|-------|
| Session summaries (local fallback) | High | `qwen2.5:72b` or `mistral-small3.2` | Quality gap vs. Sonnet; use as fallback when OpenRouter key absent |
| Chat/routing pre-processor | High | `qwen2.5-coder:7b` or `mistral:latest` | Fast enough, good for structured classification tasks |
| Replace OpenRouter entirely | Low | — | 70B models add latency; Sonnet quality hard to match locally |

**Recommendation:** Add Ollama as a **fallback** inference provider (env: `LOCAL_LLM_ENDPOINT=http://localhost:11434`) rather than primary. Use the kuzu-memory `detect_local_llm()` pattern for discovery. The routing/pre-processing layer is the stronger use case for local inference — it requires less quality and benefits from zero API cost and latency on short classification prompts.
