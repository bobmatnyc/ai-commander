# Hybrid NLP + LLM Summarization Research

**Date**: 2026-04-01
**Type**: Actionable Research
**Scope**: Replace pure-LLM summarizer with tiered hybrid approach

---

## Executive Summary

The current summarizer (`crates/commander-core/src/summarizer.rs`) makes an OpenRouter API call (defaulting to `anthropic/claude-sonnet-4`) for **every** summarization event. There are three call sites:

1. **Progressive summaries**: Every 500 characters of new output (`chars_since_last_summary >= 500`)
2. **Incremental summaries**: Every 50 new lines of output
3. **Final summary**: When Claude Code finishes and goes idle

For a typical Claude Code response that generates 2000-5000 characters of meaningful output, this means **4-10+ LLM calls per response**. With hundreds of responses per day across sessions, costs add up fast.

**Key finding**: The codebase already has substantial structured extraction infrastructure (`output_filter.rs`, `change_detector/`, `notification_parser.rs`) that extracts structured events from terminal output. This infrastructure handles ~60-80% of what the summarizer needs but is NOT used by the summarizer.

**Recommendation**: Implement a 3-tier summarization system that eliminates 70-90% of LLM calls by leveraging the existing pattern-matching infrastructure.

---

## Current Architecture Analysis

### Summarization Call Flow

```
Terminal Output (tmux capture)
    |
    v
output_filter::find_new_lines()    -- Extracts new lines, filters UI noise
    |
    v
session.add_response_lines()       -- Buffers lines
    |
    +-- Every 500 chars --> summarizer::summarize_incremental()  --> LLM API call
    +-- Every 50 lines  --> summarizer::summarize_incremental()  --> LLM API call
    +-- On completion   --> summarizer::summarize_with_fallback() --> LLM API call
```

### What Already Exists (Underutilized)

1. **`output_filter.rs`** - Already does:
   - ANSI stripping
   - UI noise filtering (spinners, box drawing, Claude branding, thinking indicators)
   - Claude readiness detection
   - Selector/prompt detection
   - Screen preview generation
   - Response cleaning

2. **`change_detector/`** - Already does:
   - Pattern-based classification of terminal output changes
   - Significance scoring (Critical/High/Medium/Low/Ignore)
   - Change type detection: Completion, Error, WaitingForInput, Progress
   - Test result parsing (`N tests passed/failed`)
   - Git operation detection
   - File change detection
   - Build progress detection
   - Human-readable summaries via `summarize_change()`

3. **`notification_parser.rs`** - Already does:
   - Session status extraction
   - Git status parsing
   - Model info parsing
   - Conversational text generation from structured data

### The Gap

The `summarizer.rs` does NOT use any of the above. It takes raw response text, sends it to an LLM, and gets back a natural language summary. The change detector and output filter operate in a parallel pipeline (used for notifications and session monitoring) but their structured output is never fed to the summarizer.

---

## Cost Analysis

### Current Cost Per Summary Call

| Component | Tokens | Notes |
|-----------|--------|-------|
| System prompt | ~120 tokens | Fixed per call |
| User prompt template | ~30 tokens | "User asked: ... Raw response: ..." |
| Input content | 125-1250 tokens | 500-5000 chars of terminal output |
| **Total input** | **~275-1400 tokens** | |
| Output | 50-150 tokens | Summary text |
| **Total per call** | **~325-1550 tokens** | |

### Cost Per Model (per 1M tokens)

| Model | Input | Output | Typical Call Cost |
|-------|-------|--------|-------------------|
| claude-sonnet-4 (default) | $3.00 | $15.00 | $0.0008-$0.004 |
| claude-haiku-3.5 | $0.80 | $4.00 | $0.0002-$0.001 |
| gemini-flash-2.0 | $0.10 | $0.40 | $0.00003-$0.0002 |

### Monthly Cost Estimate (Current System)

Assumptions:
- 200 responses/day (moderate usage across sessions)
- Average 6 LLM calls per response (2 progressive + 2 incremental + 1 final + 1 screen interpret)
- 200 * 6 = 1,200 LLM calls/day = 36,000 calls/month
- Average 800 tokens per call

| Model | Monthly Token Usage | Monthly Cost |
|-------|-------------------|--------------|
| claude-sonnet-4 | 28.8M tokens | **$86-$432** |
| claude-haiku-3.5 | 28.8M tokens | **$23-$115** |
| gemini-flash-2.0 | 28.8M tokens | **$3-$12** |

### With Hybrid Approach (Projected)

If Tier 1 (structured extraction) handles 75% of summaries:
- LLM calls drop from 36,000 to 9,000/month
- Remaining calls use Haiku or Flash (cheaper models)
- **Projected savings: 85-95% cost reduction**

---

## Proposed Tiered Architecture

### Tier 1: Structured Extraction (Instant, Free)

**Handles: ~75% of all summarization calls**

Extend the existing `change_detector` to produce template-based summaries directly. The change detector already classifies output into types (Completion, Error, Progress, WaitingForInput) and extracts key information.

**Implementation approach**: Create a `StructuredSummarizer` that:

1. Runs `change_detector::classify_change()` on the response buffer
2. Extracts structured events using regex patterns (most already exist)
3. Constructs a template-based summary

**Patterns to extract (ordered by frequency)**:

| Pattern | Regex/Detection | Template |
|---------|----------------|----------|
| File edits | `(Created\|Modified\|Wrote\|Updated) (.+)` | "Edited {N} files: {list}" |
| Test results | `(\d+) (tests? )?(passed\|failed)` | "Tests: {passed} passed, {failed} failed" |
| Git operations | `(committed\|pushed\|merged) (.+)` | "Git: {operation}" |
| Build output | `(Compiling\|Building\|Finished) (.+)` | "Build: {status}" |
| Error/panic | `(error\|panic\|failed): (.+)` | "Error: {message}" |
| Search/read | `(Searched\|Reading\|Found) (.+)` | "Analyzed: {N} files" |
| Tool calls | `(Bash\|Read\|Edit\|Write\|Grep\|Glob)` | "Used tools: {list}" |
| Completion | Session goes idle after activity | "Task completed" |
| Simple response | Short output (<200 chars, no tools) | Pass through as-is |

**Pseudo-Rust implementation**:

```rust
pub struct StructuredSummary {
    pub files_edited: Vec<String>,
    pub tests: Option<TestResult>,
    pub git_ops: Vec<String>,
    pub errors: Vec<String>,
    pub tools_used: HashSet<String>,
    pub key_lines: Vec<String>,  // Non-noise, non-tool lines
}

impl StructuredSummary {
    pub fn to_message(&self) -> String {
        let mut parts = Vec::new();

        if !self.files_edited.is_empty() {
            parts.push(format!("Edited {} file(s): {}",
                self.files_edited.len(),
                self.files_edited.join(", ")));
        }

        if let Some(ref tests) = self.tests {
            parts.push(format!("Tests: {} passed, {} failed",
                tests.passed, tests.failed));
        }

        // ... more template construction

        if parts.is_empty() && !self.key_lines.is_empty() {
            // Fall through to Tier 2 or use key lines directly
            return self.key_lines[..3.min(self.key_lines.len())].join(". ");
        }

        parts.join("\n")
    }

    pub fn confidence(&self) -> f32 {
        // How confident are we that this summary is sufficient?
        // High confidence = skip LLM call
        let has_structure = !self.files_edited.is_empty()
            || self.tests.is_some()
            || !self.git_ops.is_empty()
            || !self.errors.is_empty();

        if has_structure { 0.9 }
        else if self.key_lines.len() <= 3 { 0.7 }
        else { 0.3 }  // Complex output, probably needs LLM
    }
}
```

### Tier 2: Fast/Cheap Model (Fast, Cheap)

**Handles: ~20% of calls (moderate complexity)**

When Tier 1 confidence is below threshold (0.6), use a fast, cheap model:
- `anthropic/claude-haiku-3.5` via OpenRouter ($0.80/$4.00 per 1M)
- Or `google/gemini-flash-2.0` ($0.10/$0.40 per 1M)

Reduce input tokens by:
- Pre-filtering with Tier 1 (send only `key_lines`, not full output)
- Including the partial structured summary as context
- Using shorter system prompts

**Modified API call**:
```rust
// Instead of sending raw output, send pre-processed context
let user_prompt = format!(
    "User asked: {query}\n\n\
     Structured facts:\n{structured_summary}\n\n\
     Remaining unstructured output ({n} lines):\n{key_lines}\n\n\
     Provide a brief conversational summary:",
    query = query,
    structured_summary = tier1_summary,
    n = key_lines.len(),
    key_lines = key_lines.join("\n")
);
```

This typically reduces input from ~1000 tokens to ~300 tokens, cutting cost by 70% even when an LLM call is needed.

### Tier 3: Full Model (Slower, Expensive)

**Handles: ~5% of calls (novel/complex output)**

Reserved for:
- Very long, unstructured output where Tier 1 has low confidence
- Multi-step reasoning explanations
- Error diagnosis with complex context
- Explicit user request for detailed summary

Use the current `claude-sonnet-4` model, but only when actually needed.

### Decision Flow

```
New output arrives
    |
    v
Run Tier 1: StructuredSummarizer
    |
    +-- confidence >= 0.7 --> Use Tier 1 summary (FREE)
    |
    +-- confidence 0.4-0.7 --> Tier 2: Haiku/Flash (CHEAP)
    |       Send structured context + key lines
    |       Reduced input tokens (~300 vs ~1000)
    |
    +-- confidence < 0.4 --> Tier 3: Sonnet (EXPENSIVE)
            Full raw output (current behavior)
            Only for truly complex/novel output
```

---

## Rust Ecosystem Analysis

### Option A: Pure Regex/Pattern Approach (Recommended)

**No new dependencies needed.** The codebase already uses `regex` extensively.

Extend `change_detector/patterns.rs` with more extraction patterns and add a `StructuredSummarizer` module. This is:
- Zero binary size increase
- Zero new dependencies
- Fully deterministic
- Sub-millisecond execution
- Already proven in the codebase

### Option B: rust-bert for Local NLP

**Not recommended for this use case.**

- `rust-bert` requires `libtorch` (~1.5-3GB download, ~500MB in binary)
- Startup time: 5-15 seconds to load model
- Memory: 500MB-2GB for even small models
- Extractive summarization models (BERT/DistilBERT) achieve ~2-3x compression
- For terminal output (not natural language prose), keyword extraction is not much better than regex

**Verdict**: Massive overhead for marginal benefit over regex patterns. Terminal output is structured enough that NLP models offer little advantage over pattern matching.

### Option C: ONNX Runtime for Local Inference

**Not recommended for this use case.**

- `ort` crate adds ~50-100MB to binary (ONNX runtime shared libraries)
- Could run small summarization models locally (e.g., DistilBART)
- But terminal output is not natural language prose -- standard NLU models struggle with it
- Would need custom fine-tuning for Claude Code output patterns
- Latency: 100-500ms per inference on CPU

**Verdict**: Over-engineered for the problem. Custom fine-tuning on terminal output is a significant project.

### Option D: tokenizers + Simple Scoring (Possible Enhancement)

The `tokenizers` crate (by Hugging Face) is lightweight (~5MB) and could be used for:
- Accurate token counting (vs. current `len() / 4` approximation)
- TF-IDF-like sentence scoring for extractive summarization
- Better text segmentation

**Verdict**: Nice-to-have for accurate token counting, but not essential. The `chars / 4` heuristic works well enough for thresholding.

### Option E: whatlang / lingua for Language Detection

**Not relevant.** Terminal output is overwhelmingly English with code/paths/commands. Language detection adds no value here.

---

## Specific Techniques for Terminal Output

### Already Implemented (output_filter.rs)

- [x] ANSI stripping (`strip_ansi_basic`)
- [x] Spinner/progress indicator detection
- [x] Box drawing character filtering
- [x] Claude Code branding removal
- [x] Thinking indicator filtering
- [x] MCP tool noise filtering
- [x] Status bar filtering
- [x] Clean response extraction

### Already Implemented (change_detector/)

- [x] Test result parsing
- [x] Error/exception detection
- [x] Completion detection
- [x] File change detection
- [x] Git operation detection
- [x] Build progress detection
- [x] Input-waiting detection
- [x] Significance scoring

### Needs Implementation (for Tier 1)

- [ ] **Tool use detection and counting**: Parse `(Bash)`, `(Read)`, `(Edit)`, `(Write)`, `(Grep)` tool invocations and count them
- [ ] **File list extraction**: From tool calls, extract which files were read/edited/created
- [ ] **Diff summarization**: For git diffs, extract `+N/-M lines` and affected files
- [ ] **Progress collapsing**: Detect repeated progress patterns (e.g., "Compiling crate 1/10 ... 10/10") and collapse to "Compiled 10 crates"
- [ ] **Error message extraction**: Pull the most relevant error line from stack traces
- [ ] **Template-based summary construction**: Compose extracted events into natural language

### Estimated Extraction Accuracy by Output Type

| Output Type | Frequency | Tier 1 Confidence | Example |
|-------------|-----------|-------------------|---------|
| File read/search | 30% | 0.9 | "Searched 5 files, read 3" |
| Code edit | 25% | 0.9 | "Edited 2 files: main.rs, lib.rs" |
| Test run | 15% | 0.95 | "Tests: 42 passed, 0 failed" |
| Build | 10% | 0.85 | "Build succeeded" |
| Git ops | 5% | 0.9 | "Committed changes, pushed to main" |
| Error/debug | 10% | 0.6 | Needs LLM for context |
| Explanatory text | 5% | 0.3 | Needs LLM |

**Weighted average Tier 1 confidence: ~0.82** -- meaning ~82% of calls can skip the LLM entirely.

---

## Implementation Plan

### Phase 1: Structured Summarizer Module (Effort: Small, Impact: High)

**Goal**: Add `structured_summarizer.rs` to `commander-core` that produces template summaries.

1. Create `StructuredSummary` struct with extracted events
2. Add extraction patterns for tool use, file lists, progress collapsing
3. Implement `to_message()` for template-based summary generation
4. Implement `confidence()` scoring
5. Unit tests with real Claude Code output samples

**Files to modify**:
- New: `crates/commander-core/src/structured_summarizer.rs`
- Modified: `crates/commander-core/src/lib.rs` (re-export)
- Modified: `crates/commander-core/Cargo.toml` (no new deps needed)

**Estimated effort**: 2-3 days

### Phase 2: Integrate with Summarizer (Effort: Small, Impact: High)

**Goal**: Make `summarize_with_fallback` and `summarize_incremental` use tiered approach.

1. Add `summarize_structured()` function that tries Tier 1 first
2. Fall through to Tier 2 (Haiku) if confidence < 0.7
3. Fall through to Tier 3 (Sonnet) if confidence < 0.4
4. Add config for tier thresholds and model selection
5. Add metrics/logging for tier hit rates

**Files to modify**:
- Modified: `crates/commander-core/src/summarizer.rs`
- Modified: `crates/commander-telegram/src/state.rs` (call new functions)

**Estimated effort**: 1-2 days

### Phase 3: Optimize Progressive Summaries (Effort: Small, Impact: Medium)

**Goal**: Reduce progressive summary frequency using smarter triggers.

Current: Summary every 500 chars OR every 50 lines (whichever comes first).

Proposed:
- Only send progressive summary if `change_detector` detects Medium+ significance
- Skip progressive summaries for simple file reads/searches (Low significance)
- Batch incremental summaries: wait for 2-3 significant events before summarizing
- Use Tier 1 for all progressive summaries (they are ephemeral status updates)

**Estimated effort**: 1 day

### Phase 4: Model Tier Configuration (Effort: Small, Impact: Medium)

**Goal**: Allow configuring which model is used for each tier.

```
SUMMARIZER_TIER2_MODEL=anthropic/claude-haiku-3.5
SUMMARIZER_TIER3_MODEL=anthropic/claude-sonnet-4
SUMMARIZER_CONFIDENCE_THRESHOLD=0.7
```

**Estimated effort**: 0.5 days

---

## Impact/Effort Matrix

| Initiative | Impact | Effort | Priority |
|-----------|--------|--------|----------|
| Phase 1: Structured summarizer | HIGH (eliminates 75%+ LLM calls) | Small (2-3 days) | **P0** |
| Phase 2: Tiered integration | HIGH (remaining calls cheaper) | Small (1-2 days) | **P0** |
| Phase 3: Smarter triggers | MEDIUM (fewer total calls) | Small (1 day) | P1 |
| Phase 4: Model config | MEDIUM (cost control) | Small (0.5 day) | P1 |
| rust-bert / ONNX | LOW (marginal benefit) | LARGE (weeks) | **Skip** |
| Custom fine-tuning | LOW-MEDIUM | VERY LARGE (months) | **Skip** |

---

## Projected Savings (Conservative Estimate)

| Metric | Current | After Phase 1+2 | Improvement |
|--------|---------|-----------------|-------------|
| LLM calls/day | 1,200 | 300 | 75% reduction |
| Avg tokens/call | 800 | 400 | 50% reduction (pre-filtered input) |
| Model cost tier | Sonnet ($3/$15) | Mix: 75% free, 20% Haiku, 5% Sonnet | ~90% cost reduction |
| Monthly cost (Sonnet) | $86-$432 | $5-$25 | **90-95% savings** |
| Latency (progressive) | 1-3s (API roundtrip) | <1ms (template) | **1000x faster** |
| Latency (final) | 1-3s | <1ms (75%) or 1-3s (25%) | **75% instant** |

---

## Key Insight

The most important finding is that **the codebase already solves 80% of this problem** through `change_detector/` and `output_filter.rs`. The summarizer simply does not use these modules. Connecting them -- having the summarizer check structured extraction first before falling back to LLM -- is a small change with enormous impact.

The NLP/ML approaches (rust-bert, ONNX, local models) are unnecessary for this specific problem because terminal output from Claude Code is highly structured and predictable, making regex-based extraction more appropriate than natural language understanding.
