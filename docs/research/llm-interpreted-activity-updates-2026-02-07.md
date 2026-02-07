# LLM-Interpreted Activity Updates Architecture Research

**Date:** 2026-02-07
**Author:** Research Agent
**Status:** Actionable
**Related Issues:** #28, #29, #30, #31 (AgentOrchestrator Integration)

## Executive Summary

This document analyzes the architectural approach for replacing regex-based activity interpretation with LLM-based semantic understanding in ai-commander. The research recommends a **hybrid tiered architecture** that uses deterministic pattern matching as a fast-path filter, with LLM interpretation for semantically complex cases. This approach balances the user requirement for semantic understanding with latency, cost, and reliability constraints.

## Table of Contents

1. [Current Architecture Analysis](#1-current-architecture-analysis)
2. [LLM Interpretation Design](#2-llm-interpretation-design)
3. [Orchestrator Integration](#3-orchestrator-integration)
4. [Hybrid Architecture Recommendation](#4-hybrid-architecture-recommendation)
5. [Implementation Plan](#5-implementation-plan)
6. [Trade-offs and Risks](#6-trade-offs-and-risks)

---

## 1. Current Architecture Analysis

### 1.1 Activity Update Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CURRENT FLOW                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ┌──────────────┐     ┌──────────────────┐     ┌────────────────┐         │
│   │ tmux session │────>│ TmuxOrchestrator │────>│ capture_output │         │
│   │  (Claude)    │     │ (commander-tmux) │     │   (raw text)   │         │
│   └──────────────┘     └──────────────────┘     └───────┬────────┘         │
│                                                         │                   │
│                                                         v                   │
│   ┌──────────────────────────────────────────────────────────────────┐     │
│   │                    INTERPRETATION LAYER                          │     │
│   │  ┌─────────────────┐    ┌──────────────────────────────────────┐│     │
│   │  │ notification_   │    │ change_detector (commander-core)     ││     │
│   │  │ parser          │    │ - hash comparison                    ││     │
│   │  │ (regex-based)   │    │ - pattern classification             ││     │
│   │  │ - session name  │    │ - significance scoring               ││     │
│   │  │ - path/branch   │    │                                      ││     │
│   │  │ - model info    │    │ Patterns:                            ││     │
│   │  │ - context %     │    │ - completion: /completed|done|.../   ││     │
│   │  └────────┬────────┘    │ - error: /error|failed|exception/    ││     │
│   │           │             │ - waiting: /waiting for input/       ││     │
│   │           │             │ - progress: /test.*passed/           ││     │
│   │           │             └─────────────────┬────────────────────┘│     │
│   └───────────┼───────────────────────────────┼──────────────────────┘     │
│               │                               │                             │
│               v                               v                             │
│   ┌──────────────────────────────────────────────────────────────────┐     │
│   │                     ACTION DISPATCH                              │     │
│   │                                                                  │     │
│   │   TUI (sessions.rs)          Telegram (notifications.rs)        │     │
│   │   - check_session_status()   - notify_session_ready()           │     │
│   │   - scan_all_sessions()      - notify_sessions_waiting()        │     │
│   │                              - push_notification()              │     │
│   └──────────────────────────────────────────────────────────────────┘     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Key Components

| Component | Location | Purpose | Current Approach |
|-----------|----------|---------|------------------|
| `notification_parser` | `commander-core/src/notification_parser.rs` | Parse timer notification format | Regex patterns for structured fields |
| `ChangeDetector` | `commander-core/src/change_detector/mod.rs` | Detect meaningful changes in output | Hash + regex pattern matching |
| `patterns.rs` | `commander-core/src/change_detector/patterns.rs` | Define significant patterns | ~25 regex patterns for classification |
| `SessionAgent::analyze_output` | `commander-agent/src/session_agent/analysis.rs` | LLM-based output analysis | Already uses LLM for High significance |
| `TUI sessions.rs` | `ai-commander/src/tui/sessions.rs` | Session status monitoring | `is_claude_ready()` + preview extraction |

### 1.3 Current Interpretation Methods

**1. notification_parser.rs (Structured Regex)**
```rust
// Extracts structured data from formatted notifications
static SESSION_REGEX: Regex = r"(?:^|\s)@([a-zA-Z0-9_-]+)"
static PATH_REGEX: Regex = r"([^@\s]+)@([^:]+):([^\s(]+)"
static BRANCH_REGEX: Regex = r"\(([a-zA-Z0-9_/.-]{2,})([*?!+-]*)\)"
static MODEL_REGEX: Regex = r"\[([^|\]]+)\|([^|\]]+)\|([0-9]+)%\]"
```

**2. change_detector/patterns.rs (Pattern Classification)**
```rust
// Classifies change significance and type
(Regex::new(r"(?i)\b(completed?|finished|done|success(ful)?)\b"), ChangeType::Completion, Significance::High)
(Regex::new(r"(?i)\b(error|failed|failure|exception|panic|fatal)\b"), ChangeType::Error, Significance::High)
(Regex::new(r"(?i)(waiting for|awaiting|requires?) (input|response|confirmation)"), ChangeType::WaitingForInput, Significance::High)
```

**3. SessionAgent::analyze_output (LLM Analysis)**
```rust
// Already uses LLM for semantic understanding
let analysis_prompt = format!(
    "Analyze the following session output and extract:
    1. Whether a task was completed
    2. Whether the session is waiting for user input
    3. Any errors or warnings
    4. Files that were modified"
);
```

### 1.4 Key Findings

1. **LLM capability already exists** - `SessionAgent::analyze_output()` can interpret activity semantically
2. **Tiered approach in place** - `ChangeDetector` only escalates to LLM for High significance
3. **Multiple entry points** - TUI, Telegram, and REPL all need consistent interpretation
4. **Latency sensitivity** - Session status checks run every 5 seconds; full scans every 5 minutes

---

## 2. LLM Interpretation Design

### 2.1 Proposed Activity Interpretation Pipeline

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     PROPOSED LLM INTERPRETATION FLOW                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   ┌──────────────┐                                                          │
│   │ Raw Output   │                                                          │
│   │ from tmux    │                                                          │
│   └──────┬───────┘                                                          │
│          │                                                                  │
│          v                                                                  │
│   ┌──────────────────────────────────────────────────────────────────┐     │
│   │  TIER 1: FAST PATH (Deterministic)              < 1ms            │     │
│   │                                                                  │     │
│   │  Hash comparison → No change? → SKIP                             │     │
│   │                                                                  │     │
│   │  Noise filtering → Pure UI artifacts? → SKIP                     │     │
│   │                                                                  │     │
│   │  Pattern matching → High-confidence match? → DISPATCH            │     │
│   │    - "tests passed" → Progress notification                      │     │
│   │    - "Error:" prefix → Error notification                        │     │
│   │    - Claude prompt visible → Waiting for input                   │     │
│   └──────────────────────────────────────┬───────────────────────────┘     │
│                                          │                                  │
│                                          │ Ambiguous / Complex              │
│                                          v                                  │
│   ┌──────────────────────────────────────────────────────────────────┐     │
│   │  TIER 2: SEMANTIC INTERPRETATION (LLM)          500-2000ms       │     │
│   │                                                                  │     │
│   │  Batched context + focused prompt:                               │     │
│   │                                                                  │     │
│   │  "Analyze this session activity:                                 │     │
│   │   {last_n_lines}                                                 │     │
│   │                                                                  │     │
│   │   Determine:                                                     │     │
│   │   1. Activity type: [working|waiting|error|completed|idle]       │     │
│   │   2. Confidence: [high|medium|low]                               │     │
│   │   3. Summary: Brief description of current state                 │     │
│   │   4. Action needed: [none|notify_user|urgent_alert]              │     │
│   │                                                                  │     │
│   │   Respond in JSON format."                                       │     │
│   └──────────────────────────────────────┬───────────────────────────┘     │
│                                          │                                  │
│                                          v                                  │
│   ┌──────────────────────────────────────────────────────────────────┐     │
│   │  ACTION DISPATCH                                                 │     │
│   │                                                                  │     │
│   │  Based on LLM interpretation:                                    │     │
│   │  - Update session state in AgentOrchestrator                     │     │
│   │  - Push notification if action needed                            │     │
│   │  - Update TUI display                                            │     │
│   │  - Broadcast to Telegram if configured                           │     │
│   └──────────────────────────────────────────────────────────────────┘     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Prompt Structure for Activity Interpretation

```rust
/// System prompt for activity interpretation
pub const ACTIVITY_INTERPRETER_SYSTEM: &str = r#"
You are an AI session monitor that interprets terminal output from Claude Code sessions.
Your role is to understand what the AI is doing and determine if user attention is needed.

Key signals to look for:
- Waiting states: Prompts, questions, "waiting for input"
- Completion states: "Done", "Finished", task completion messages
- Error states: Error messages, stack traces, failures
- Progress states: Test results, build output, file operations
- Idle states: No meaningful activity, cursor blinking

Respond with structured JSON only.
"#;

/// User prompt template
pub const ACTIVITY_INTERPRET_PROMPT: &str = r#"
Analyze this session output and determine the current state:

```
{output}
```

Previous state: {previous_state}
Session: {session_name}

Respond with JSON:
{
  "activity_type": "working|waiting|error|completed|idle",
  "confidence": "high|medium|low",
  "summary": "Brief description",
  "requires_attention": true|false,
  "urgency": "none|low|high",
  "details": {
    "files_changed": [],
    "error_type": null,
    "next_action": null
  }
}
"#;
```

### 2.3 Latency Handling Strategies

| Strategy | Description | Use Case |
|----------|-------------|----------|
| **Batching** | Accumulate changes for 2-5 seconds before LLM call | Background monitoring |
| **Debouncing** | Skip interpretation if another is pending | Rapid output streams |
| **Caching** | Cache interpretation for identical output | Repeated patterns |
| **Async dispatch** | Don't block UI on LLM response | All cases |
| **Timeout fallback** | Revert to regex if LLM takes >3s | Reliability |

### 2.4 Cost Optimization

**Estimated LLM usage per session check:**
- Input tokens: ~500 (output sample + prompt)
- Output tokens: ~100 (JSON response)
- Cost per check: ~$0.001 (Claude Haiku) to ~$0.003 (Claude Sonnet)

**Optimization strategies:**
1. **Use cheaper model** - Haiku for routine interpretation, Sonnet for complex cases
2. **Aggressive filtering** - Only invoke LLM for truly ambiguous cases
3. **Output truncation** - Send last 50 lines maximum, not full scrollback
4. **Shared context** - Batch multiple sessions in single request where possible

---

## 3. Orchestrator Integration

### 3.1 Existing Infrastructure

The `AgentOrchestrator` in `commander-orchestrator/src/orchestrator.rs` already provides:

```rust
pub struct AgentOrchestrator {
    user_agent: UserAgent,              // For processing user input
    session_agents: HashMap<String, SessionAgent>,  // Per-session agents
    memory_store: Arc<dyn MemoryStore>, // Shared memory
    auto_eval: AutoEval,                // Feedback tracking
}

impl AgentOrchestrator {
    // Process user input through User Agent
    pub async fn process_user_input(&mut self, input: &str) -> Result<String>;

    // Process session output through Session Agent (ALREADY HAS LLM ANALYSIS)
    pub async fn process_session_output(
        &mut self,
        session_id: &str,
        adapter_type: &str,
        output: &str,
    ) -> Result<OutputAnalysis>;
}
```

### 3.2 `SessionAgent::analyze_output` (Existing LLM Integration)

```rust
// Already implemented in commander-agent/src/session_agent/analysis.rs
impl SessionAgent {
    pub async fn analyze_output(&mut self, output: &str) -> Result<OutputAnalysis> {
        let analysis_prompt = format!(
            "Analyze the following session output and extract:
            1. Whether a task was completed
            2. Whether the session is waiting for user input
            3. Any errors or warnings
            4. Files that were modified"
        );

        // Uses OpenRouter client for LLM call
        let response = self.client.chat(&self.config, messages, None).await?;
        // ... parse response into OutputAnalysis
    }
}
```

### 3.3 Integration Points with Issues #28-31

| Issue | Integration Point | LLM Interpretation Role |
|-------|-------------------|------------------------|
| #28 Auto-initialize AgentOrchestrator in TUI | `App::new()` | Orchestrator manages LLM clients |
| #29 Route TUI messages through orchestrator | `send_message()` | LLM interprets responses |
| #30 Add AgentOrchestrator to REPL | REPL main loop | Consistent interpretation |
| #31 Add AgentOrchestrator to Telegram bot | Bot handlers | LLM summarizes for Telegram |

### 3.4 Proposed Architecture Extension

```rust
// New trait for activity interpretation
pub trait ActivityInterpreter {
    /// Interpret raw output semantically
    async fn interpret_activity(
        &mut self,
        session_id: &str,
        output: &str,
        previous_state: Option<&ActivityState>,
    ) -> Result<ActivityInterpretation>;
}

// Extension to AgentOrchestrator
impl AgentOrchestrator {
    /// Interpret session activity with tiered approach
    pub async fn interpret_session_activity(
        &mut self,
        session_id: &str,
        output: &str,
    ) -> Result<ActivityInterpretation> {
        // Get or create session agent
        let agent = self.get_session_agent(session_id, "auto")?;

        // Tier 1: Check deterministic patterns first
        let change = agent.change_detector().detect(output);

        if change.significance < Significance::Medium {
            // Fast path: Use pattern-based interpretation
            return Ok(ActivityInterpretation::from_change_event(&change));
        }

        // Tier 2: LLM interpretation for complex cases
        let llm_analysis = agent.analyze_output(output).await?;

        Ok(ActivityInterpretation::from_analysis(&llm_analysis))
    }
}
```

---

## 4. Hybrid Architecture Recommendation

### 4.1 Recommended Approach: Tiered Hybrid Interpretation

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    RECOMMENDED ARCHITECTURE                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │                   ActivityInterpreterService                        │    │
│  │                                                                     │    │
│  │   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐   │    │
│  │   │ PatternMatcher  │  │ LLMInterpreter  │  │ ChangeDetector  │   │    │
│  │   │ (fast path)     │  │ (semantic)      │  │ (diff engine)   │   │    │
│  │   └────────┬────────┘  └────────┬────────┘  └────────┬────────┘   │    │
│  │            │                    │                    │            │    │
│  │            └────────────────────┼────────────────────┘            │    │
│  │                                 │                                 │    │
│  │                                 v                                 │    │
│  │                    ┌───────────────────────┐                      │    │
│  │                    │ ActivityInterpretation │                      │    │
│  │                    │                        │                      │    │
│  │                    │ - activity_type        │                      │    │
│  │                    │ - confidence           │                      │    │
│  │                    │ - summary              │                      │    │
│  │                    │ - requires_attention   │                      │    │
│  │                    │ - interpretation_method│                      │    │
│  │                    └───────────────────────┘                      │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │                   AgentOrchestrator                                 │    │
│  │                                                                     │    │
│  │   - Owns ActivityInterpreterService                                │    │
│  │   - Routes interpretation requests                                  │    │
│  │   - Manages session state                                          │    │
│  │   - Dispatches notifications                                       │    │
│  │                                                                     │    │
│  │   interpret_session_activity(session_id, output) -> Result<...>   │    │
│  └────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │     TUI     │  │    REPL     │  │  Telegram   │  │    API      │       │
│  │             │  │             │  │             │  │             │       │
│  │  Uses orchestrator for all activity interpretation              │       │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Decision Flow

```
Input: Raw session output
        │
        v
┌───────────────────────────────────────┐
│ 1. Hash comparison                    │
│    Changed? ────NO───> Return(None)   │
└───────────────┬───────────────────────┘
                │ YES
                v
┌───────────────────────────────────────┐
│ 2. Noise filtering                    │
│    Pure UI noise? ──YES──> Return(None)│
└───────────────┬───────────────────────┘
                │ NO
                v
┌───────────────────────────────────────┐
│ 3. Pattern matching (regex)           │
│    High-confidence match? ────────────┼──YES──> Return(PatternResult)
│                                       │
│    Patterns:                          │
│    - Error indicators (Critical)      │
│    - Waiting indicators (High)        │
│    - Completion indicators (High)     │
│    - Test results (Medium)            │
└───────────────┬───────────────────────┘
                │ NO (ambiguous)
                v
┌───────────────────────────────────────┐
│ 4. LLM Interpretation                 │
│                                       │
│    - Truncate output to last 50 lines │
│    - Include previous state context   │
│    - Use focused prompt               │
│    - Parse JSON response              │
│                                       │
│    Return(LLMResult)                  │
└───────────────────────────────────────┘
```

### 4.3 Keep Regex for These Cases

| Case | Reason | Confidence |
|------|--------|------------|
| Error prefix patterns | `"Error:"`, `"FATAL:"` are unambiguous | 100% |
| Claude prompt visible | Well-defined UI pattern | 100% |
| Test result summaries | `"42 tests passed"` is structured | 100% |
| Git commit messages | Clear indicators | 95% |
| Build completion | `"Build succeeded"` | 95% |

### 4.4 Use LLM for These Cases

| Case | Reason | Example |
|------|--------|---------|
| Complex error context | Needs understanding of what failed | Stack trace analysis |
| Ambiguous completion | "Done" could be partial | AI saying "Done with step 1" |
| Implicit waiting | No explicit prompt, but context shows waiting | Cursor at empty line |
| Progress interpretation | Understanding what percentage done | Multi-step task progress |
| Natural language status | AI describing its state | "I'm working on..." |

---

## 5. Implementation Plan

### Phase 1: Foundation (Issues #28-29)

**Duration:** 1-2 days
**Dependencies:** None

1. **Auto-initialize AgentOrchestrator in TUI (#28)**
   - Add `orchestrator: Option<AgentOrchestrator>` initialization in `App::new()`
   - Handle async/sync boundary with tokio runtime
   - Add fallback if initialization fails

2. **Route TUI messages through orchestrator (#29)**
   - Modify `send_message()` to use orchestrator
   - Implement fallback to direct adapter

### Phase 2: Activity Interpretation Service

**Duration:** 2-3 days
**Dependencies:** Phase 1

1. **Create `ActivityInterpreterService` in commander-agent**
   ```rust
   pub struct ActivityInterpreterService {
       pattern_matcher: PatternMatcher,
       llm_client: OpenRouterClient,
       config: InterpretationConfig,
   }

   impl ActivityInterpreterService {
       pub async fn interpret(&mut self, output: &str, context: &InterpretContext)
           -> Result<ActivityInterpretation>;
   }
   ```

2. **Add to AgentOrchestrator**
   ```rust
   impl AgentOrchestrator {
       pub async fn interpret_session_activity(
           &mut self,
           session_id: &str,
           output: &str,
       ) -> Result<ActivityInterpretation>;
   }
   ```

3. **Integrate in TUI session monitoring**
   - Replace direct `is_claude_ready()` calls with orchestrator
   - Update `check_session_status()` to use LLM interpretation
   - Update `scan_all_sessions()` to use LLM interpretation

### Phase 3: Multi-Channel Integration (Issues #30-31)

**Duration:** 2-3 days
**Dependencies:** Phase 2

1. **Add AgentOrchestrator to REPL (#30)**
   - Initialize orchestrator in REPL main
   - Route input/output through orchestrator

2. **Add AgentOrchestrator to Telegram bot (#31)**
   - Initialize orchestrator in bot state
   - Use LLM interpretation for notification summaries
   - Format semantic summaries for Telegram

### Phase 4: Optimization and Tuning

**Duration:** 1-2 days
**Dependencies:** Phase 3

1. **Implement batching and debouncing**
2. **Add caching for repeated patterns**
3. **Tune LLM prompt for accuracy**
4. **Add monitoring/metrics for interpretation performance**

### Total Estimated Scope

| Phase | Effort | Lines of Code |
|-------|--------|---------------|
| Phase 1 | 1-2 days | ~200 LOC |
| Phase 2 | 2-3 days | ~400 LOC |
| Phase 3 | 2-3 days | ~300 LOC |
| Phase 4 | 1-2 days | ~150 LOC |
| **Total** | **6-10 days** | **~1050 LOC** |

---

## 6. Trade-offs and Risks

### 6.1 Advantages of Hybrid Approach

| Advantage | Description |
|-----------|-------------|
| **Semantic understanding** | LLM can interpret natural language status updates |
| **Reduced false positives** | Context-aware interpretation reduces noise |
| **Consistent behavior** | Same interpretation logic across TUI/REPL/Telegram |
| **Future flexibility** | Easy to adjust interpretation via prompts |
| **Latency control** | Fast path handles 80%+ of cases without LLM |

### 6.2 Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **LLM latency** | Delayed notifications | Tier 1 fast path + async dispatch |
| **LLM cost** | API usage fees | Aggressive filtering, use Haiku |
| **LLM reliability** | API failures | Fallback to regex on timeout |
| **Hallucination** | Wrong interpretation | Structured output + validation |
| **Complexity** | Maintenance burden | Clear tiered architecture |

### 6.3 Alternative Approaches Considered

**1. Pure LLM Approach (Rejected)**
- All interpretation via LLM
- Rejected due to: Latency (500ms+ per check), cost, reliability concerns

**2. Pure Regex Enhancement (Rejected)**
- Expand regex patterns to cover more cases
- Rejected due to: User requirement for semantic understanding, diminishing returns on pattern coverage

**3. ML Classification Model (Deferred)**
- Train custom classifier on activity types
- Deferred: Requires training data collection, more infrastructure

---

## 7. Conclusion and Recommendation

### Recommended Path Forward

1. **Implement hybrid tiered architecture** as described in Section 4
2. **Start with Phase 1** (#28, #29) to establish orchestrator foundation
3. **Add activity interpretation service** in Phase 2
4. **Extend to all channels** in Phase 3
5. **Optimize based on real-world usage** in Phase 4

### Key Design Decisions

1. **Keep regex as fast path** - Handles 80%+ of cases with <1ms latency
2. **Use LLM for ambiguous cases** - Semantic understanding where patterns fail
3. **Centralize in AgentOrchestrator** - Single source of truth for interpretation
4. **Use Claude Haiku for cost efficiency** - ~$0.001 per interpretation
5. **Async dispatch always** - Never block UI on LLM response

### Success Criteria

- [ ] All activity updates interpreted semantically (LLM path available)
- [ ] 80%+ of cases handled by fast path (no added latency)
- [ ] <2 second end-to-end latency for LLM interpretations
- [ ] <$10/month LLM cost at typical usage
- [ ] Consistent interpretation across TUI, REPL, Telegram

---

## Appendix A: Code References

| File | Purpose |
|------|---------|
| `crates/commander-core/src/notification_parser.rs` | Current regex parsing |
| `crates/commander-core/src/change_detector/mod.rs` | Change detection |
| `crates/commander-core/src/change_detector/patterns.rs` | Regex patterns |
| `crates/commander-agent/src/session_agent/analysis.rs` | Existing LLM analysis |
| `crates/commander-orchestrator/src/orchestrator.rs` | AgentOrchestrator |
| `crates/ai-commander/src/tui/sessions.rs` | TUI session monitoring |
| `crates/commander-telegram/src/notifications.rs` | Telegram notifications |

## Appendix B: Related Issues

- #28 - Auto-initialize AgentOrchestrator in TUI
- #29 - Route TUI messages through orchestrator
- #30 - Add AgentOrchestrator to REPL
- #31 - Add AgentOrchestrator to Telegram bot
- #35 - Timer notifications passed through literally (CLOSED)

---

*Research completed 2026-02-07*
