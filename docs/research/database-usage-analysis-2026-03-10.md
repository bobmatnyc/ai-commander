# Database Usage Analysis: ai-commander

**Date**: 2026-03-10
**Scope**: All crates under `/Users/masa/Projects/ai-commander/crates/`
**Method**: Full Rust source inspection, Cargo.toml dependency audit, config/env file review

---

## Executive Summary

**Redis, MongoDB, and PostgreSQL are not used anywhere in this project.** Zero occurrences of these databases exist in the entire codebase, including source files, Cargo.toml manifests, environment variables, and config files.

The project uses two storage mechanisms:

1. **Atomic JSON flat-files** (the primary persistence layer) — for all structured state: projects, events, work items, runtime state, pairing data.
2. **Qdrant** (the only database dependency) — for vector/semantic memory storage in the `commander-memory` crate, with a `LocalStore` file-based fallback.

LanceDB is also **not used**. The directory path named `chroma/` is a legacy naming artifact from an earlier design; no ChromaDB dependency exists in Cargo.toml.

---

## 1. Redis Usage

**Result: Zero usage. Not a dependency. No env vars referencing Redis.**

No files in `crates/` contain the strings `redis`, `Redis`, or `REDIS`. No Cargo.toml lists a Redis crate (`redis`, `deadpool-redis`, `fred`, etc.). No `REDIS_URL` variable appears in `.env.local` or `.env.backup`.

**Verdict**: Redis is completely absent. No caching, pub/sub, sessions, or queue patterns via Redis exist.

---

## 2. MongoDB Usage

**Result: Zero usage. Not a dependency. No env vars referencing Mongo.**

No files in `crates/` contain the strings `mongo`, `Mongo`, `MONGO`, or `mongodb`. No Cargo.toml lists a MongoDB crate. No `MONGO_URI` variable exists in any env file.

**Verdict**: MongoDB is completely absent.

---

## 3. PostgreSQL Usage

**Result: Zero usage. Not a dependency. No migrations, no SQL, no ORM.**

No files contain `postgres`, `Postgres`, `sqlx`, `diesel`, or `sea-orm`. No Cargo.toml references any relational database crate. No migration files exist.

**Verdict**: PostgreSQL is completely absent.

---

## 4. What IS Used: Atomic JSON File Persistence

**Crate**: `commander-persistence`
**Location**: `/Users/masa/Projects/ai-commander/crates/commander-persistence/`
**Dependencies**: `serde`, `serde_json`, `tempfile`, `chrono` — no external database.

### What it stores

Three store types, each writing individual `.json` files atomically (write to temp, then rename):

| Store | Directory Layout | Data Stored |
|---|---|---|
| `StateStore` | `~/.ai-commander/state/projects/{proj-id}.json` | Project definitions, name, path, aliases, state, state_reason |
| `EventStore` | `~/.ai-commander/state/events/{proj-id}/{evt-id}.json` | Agent events: status updates, decisions, approvals |
| `WorkStore` | `~/.ai-commander/state/work/{proj-id}/{work-id}.json` | Work items with priority, state (queued/running/completed), result |

Additional flat JSON files managed by `commander-core/config.rs`:

| File | Purpose |
|---|---|
| `~/.ai-commander/state/projects.json` | Legacy/alternate projects index |
| `~/.ai-commander/state/pairings.json` | Telegram chat-ID to project-ID mappings |
| `~/.ai-commander/state/notifications.json` | Cross-channel notification queue |
| `~/.ai-commander/state/authorized_chats.json` | Authorized Telegram chat IDs |
| `~/.ai-commander/state/sessions/{id}` | Runtime session state per tmux session |
| `~/.ai-commander/config/config.toml` | User configuration |

### Why it was chosen

The code comments and architecture doc (`ROADMAP.md`) confirm this is a deliberate design choice for a local developer tool:
- "crash-safe persistence using atomic file operations (write to temp file, then rename)"
- No network dependency, zero setup required
- Suitable for single-machine, single-user operation

### Operations

- `save_project` / `load_project` / `list_project_ids` / `delete_project`
- `save_event` / `load_event` / `list_events` / `delete_event`
- `save_work` / `load_work` / `list_work` / `delete_work`
- All reads/writes go through `atomic_write_json` and `read_json` helpers in `atomic.rs`

---

## 5. What IS Used: Qdrant Vector Database

**Crate**: `commander-memory`
**Location**: `/Users/masa/Projects/ai-commander/crates/commander-memory/`
**Dependency**: `qdrant-client = "1"` (Cargo.toml)

### What it stores

A single Qdrant collection named `"memories"`, used for **agent semantic memory**.

Each memory document contains:
- `id` (UUID string)
- `agent_id` (string, used for agent isolation and access control)
- `content` (raw text of the memory)
- `embedding` (Vec<f32>, 1536 dimensions by default — OpenAI `text-embedding-3-small`)
- `metadata` (arbitrary JSON key-value pairs)
- `created_at` (RFC3339 timestamp)

### Why Qdrant was chosen

The docstring explicitly states:
> "QdrantStore: Qdrant vector database for production use"
> "For production use with larger collections, use the Qdrant backend."

It was chosen for:
- Semantic similarity search (cosine distance)
- Scale — the local file fallback is noted as "suitable for small collections (< 10,000 memories)"
- Agent isolation via payload filtering on `agent_id`

### Operations performed against Qdrant

| Operation | Qdrant API Call | Purpose |
|---|---|---|
| `store` | `upsert_points` | Save or update a memory |
| `search` | `search_points` with `agent_id` filter | Semantic search for one agent |
| `search_all` | `search_points` (no filter) | Semantic search across all agents |
| `delete` | `delete_points` by ID | Remove one memory |
| `get` | `get_points` by ID | Fetch single memory |
| `list` | `scroll` with `agent_id` filter | List all memories for an agent |
| `count` | `count` with `agent_id` filter | Count memories per agent |
| `clear_agent` | `delete_points` with `agent_id` filter | Wipe all memories for one agent |

### Configuration

- `QDRANT_URL` (default: `http://localhost:6334`)
- `QDRANT_API_KEY` (optional)
- `COMMANDER_DB_DIR` — overrides where LocalStore writes files

### LocalStore fallback

When Qdrant is unavailable, `LocalStore` provides the same `MemoryStore` trait using:
- An in-memory `HashMap<String, Memory>` (with `RwLock`)
- Atomic JSON serialization to `~/.ai-commander/db/chroma/memories.json`
- Brute-force cosine similarity (O(n) scan)

Note: The directory is named `chroma/` from a legacy design when ChromaDB was the intended backend. The actual dependency is Qdrant; ChromaDB is not a Cargo dependency.

### Access Control

Two access levels are enforced via `AccessControlledStore`:
- `AccessLevel::Own` — session agents can only read/write/delete their own memories
- `AccessLevel::All` — the user agent has cross-agent visibility

---

## 6. LanceDB Usage

**Result: Not used. Not referenced anywhere in source or dependencies.**

LanceDB does not appear in any Cargo.toml, `.rs` file, env file, or documentation. The `commander-memory` crate was designed with a `MemoryStore` trait that could accommodate additional backends, but no LanceDB implementation exists.

---

## 7. Data Flow Diagram

```
User / Telegram Bot / REST API
        |
        v
commander-runtime / commander-api
        |
        +---> commander-persistence (file I/O)
        |         |
        |         +-- StateStore --> ~/.ai-commander/state/projects/*.json
        |         +-- EventStore --> ~/.ai-commander/state/events/**/*.json
        |         +-- WorkStore  --> ~/.ai-commander/state/work/**/*.json
        |         +-- flat files -> pairings.json, notifications.json, etc.
        |
        +---> commander-memory (vector I/O)
                  |
                  +-- QdrantStore --> Qdrant @ localhost:6334
                  |                   collection: "memories"
                  |
                  +-- LocalStore  --> ~/.ai-commander/db/chroma/memories.json
                                      (fallback, < 10K memories)
```

The two storage systems are completely independent. Qdrant/LocalStore does not cache or front persistence files. Persistence files do not feed Qdrant. There is no cross-database interaction.

---

## 8. Embedding Pipeline

```
Text content
    |
    v
EmbeddingGenerator (commander-memory/embedding.rs)
    |
    +-- OPENAI_API_KEY set?     --> OpenAI text-embedding-3-small (1536 dims)
    +-- OPENROUTER_API_KEY set? --> OpenRouter text-embedding-3-small (1536 dims)
    +-- neither                 --> hash-based deterministic (test/dev only)
    |
    v
Vec<f32> embedding
    |
    v
MemoryStore::store() --> Qdrant or LocalStore
```

---

## 9. LanceDB Consolidation Feasibility Assessment

### What could LanceDB replace?

#### Option A: Replace Qdrant/LocalStore with LanceDB

**Feasibility: High**

LanceDB is an embedded vector database (no separate server process) that stores data as Apache Arrow/Lance files on disk. It would eliminate the need for a running Qdrant server.

Alignment with current design:
- The `MemoryStore` trait in `store.rs` already abstracts the backend — adding a `LanceDbStore` would follow the same pattern as `QdrantStore` and `LocalStore`
- LanceDB supports cosine similarity search, matching the current `Distance::Cosine` config in Qdrant
- LanceDB is embedded, which matches the project's "zero external dependencies" philosophy for the local-first file persistence layer
- The `crates/commander-memory/src/lib.rs` doc comment already mentions the path is `~/.ai-commander/db/chroma/` — renaming it for a new backend is trivial

Trade-offs:
- LanceDB Rust bindings (`lancedb` crate) are younger than `qdrant-client`; API stability should be verified
- LanceDB has a disk-based index that is fast but slightly different from Qdrant's HNSW indexing for very large collections
- For < 10,000 memories (the stated scale at which LocalStore is adequate), LanceDB offers no compelling advantage over LocalStore itself
- For production-scale use, LanceDB would be a reasonable embedded alternative to avoid running Qdrant as a separate service

#### Option B: Replace atomic JSON files with LanceDB

**Feasibility: Low / Not appropriate**

The persistence layer (`commander-persistence`) stores structured relational data (projects, events, work items) with lookups by ID and list operations. LanceDB is optimized for vector similarity search, not key-value retrieval or relational queries. Replacing atomic JSON files with LanceDB would:
- Add complexity for no semantic search benefit
- Require embedding non-vectorizable data (project names, event titles) which is conceptually wrong
- Lose the current human-readable, crash-inspectable JSON format that is intentional in the design

**Verdict for Option B**: Not recommended.

### Summary Table

| Database | Current Status | Consolidation to LanceDB |
|---|---|---|
| Redis | Not used | N/A |
| MongoDB | Not used | N/A |
| PostgreSQL | Not used | N/A |
| ChromaDB | Named in paths only, not a dependency | N/A |
| Qdrant | Active (production vector store) | Feasible — implement `LanceDbStore` behind `MemoryStore` trait |
| LocalStore (JSON) | Active (dev/small-scale vector fallback) | Could be replaced by LanceDB embedded for consistency |
| Atomic JSON files | Active (all structured persistence) | Not appropriate — different use case |
| LanceDB | Not used | Could be added as a third `MemoryStore` backend |

### Recommendation

If the goal is to eliminate the Qdrant server process dependency, implementing a `LanceDbStore` as a third `MemoryStore` backend is the cleanest path. The trait boundary already exists; the implementation would mirror `QdrantStore` in structure. The `LocalStore` could then be deprecated or retained only for testing.

If Qdrant as an external service is not a practical problem, there is no immediate consolidation need — the current two-tier design (LocalStore for dev, QdrantStore for production) is well-structured and extensible.
