# God Class Refactoring Analysis

**Date:** 2026-02-04
**Project:** ai-commander
**Purpose:** Research and recommendations for refactoring 5 God Classes to under 800 lines each
**Status:** RESEARCH ONLY - No code changes

---

## Executive Summary

This analysis identifies method groupings and recommends Rust-idiomatic refactoring patterns for 5 oversized files:

| File | Current Lines | Methods | Target | Reduction Needed |
|------|---------------|---------|--------|------------------|
| session_agent.rs | 1617 | 49 | <800 | ~51% |
| user_agent.rs | 1267 | 33 | <800 | ~37% |
| eval.rs | 1055 | 27 | <800 | ~24% |
| change_detector.rs | 840 | 25 | <800 | ~5% |
| template.rs | 813 | 23 | <800 | ~2% |

---

## 1. session_agent.rs (1617 lines, 49 methods)

### 1.1 Method Inventory by Line Count

| Lines | Method/Section | Purpose |
|-------|----------------|---------|
| 1-58 | Module docs + imports + constants | Setup |
| 59-129 | `SessionState` struct + 12 methods | Session state management |
| 130-163 | `OutputAnalysis` struct + 3 methods | Analysis result type |
| 165-278 | `SessionAgent` struct + `new()` | Agent creation |
| 280-329 | `with_api_key()` | Alternative constructor |
| 331-355 | `default_config()` | Configuration |
| 357-459 | `builtin_tools()` | Tool definitions (~100 lines) |
| 461-524 | Accessor methods (12 methods) | Getters for state |
| 526-610 | `check_context()` | Context management |
| 612-677 | `estimate_context_tokens()`, `generate_pause_state()` | Context helpers |
| 679-760 | `process_output_change()` | Change detection integration |
| 762-899 | `analyze_output()`, `parse_analysis_response()`, `update_state()` | LLM analysis |
| 901-917 | `store_memory()` | Memory operations |
| 919-960 | `build_messages()` | Message building |
| 962-1000 | `execute_search_memories()` | Tool execution |
| 1002-1109 | Tool execution methods (4 tools) | Tool handlers |
| 1112-1261 | `impl Agent for SessionAgent` | Trait implementation |
| 1263-1282 | `format_search_results()` helper | Formatting |
| 1284-1616 | Tests (~330 lines) | Unit tests |

### 1.2 Logical Groupings by Responsibility

**Group A: State Management (129 lines)**
- `SessionState` struct
- All 12 `SessionState` methods
- State-related accessors

**Group B: Output Analysis (165 lines)**
- `OutputAnalysis` struct
- `analyze_output()`
- `parse_analysis_response()`
- `update_state()`

**Group C: Context Management (150 lines)**
- `check_context()`
- `estimate_context_tokens()`
- `generate_pause_state()`
- `ContextWindow` integration

**Group D: Change Detection Integration (80 lines)**
- `process_output_change()`
- `reset_change_detector()`
- Change detector accessors

**Group E: Tool Execution (250 lines)**
- `builtin_tools()` definitions
- All `execute_*` methods
- `format_search_results()`

**Group F: Agent Core (300 lines)**
- `SessionAgent` struct definition
- Constructors (`new`, `with_api_key`)
- `default_config()`
- `build_messages()`
- `impl Agent` trait

**Group G: Tests (330 lines)**
- MockMemoryStore
- All test functions

### 1.3 Recommended Rust Patterns

**Pattern 1: Module Decomposition**
```
session_agent/
  mod.rs           (~400 lines) - SessionAgent struct + Agent trait impl
  state.rs         (~150 lines) - SessionState + OutputAnalysis
  tools.rs         (~250 lines) - Tool definitions + execution
  context.rs       (~150 lines) - Context management methods
  analysis.rs      (~165 lines) - Output analysis logic
  tests/
    mod.rs         (~330 lines) - All tests
```

**Pattern 2: Trait Extraction for Tool Execution**
```rust
// tools.rs
pub trait ToolExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<ToolResult>;
}

pub struct SessionToolExecutor {
    memory: Arc<dyn MemoryStore>,
    embedder: EmbeddingGenerator,
    session_id: String,
}
```

**Pattern 3: Composition for Analysis**
```rust
// analysis.rs
pub struct OutputAnalyzer {
    detector: ChangeDetector,
}

impl OutputAnalyzer {
    pub async fn analyze(&mut self, output: &str, config: &ModelConfig, client: &OpenRouterClient) -> Result<OutputAnalysis>;
    fn parse_response(&self, response: &str) -> OutputAnalysis;
}
```

### 1.4 Proposed Structure

| New File | Contents | Est. Lines |
|----------|----------|------------|
| `session_agent/mod.rs` | SessionAgent struct, Agent impl, constructors | ~400 |
| `session_agent/state.rs` | SessionState, OutputAnalysis structs | ~150 |
| `session_agent/tools.rs` | Tool definitions, ToolExecutor trait, handlers | ~250 |
| `session_agent/context.rs` | Context management methods | ~150 |
| `session_agent/analysis.rs` | OutputAnalyzer, LLM analysis | ~165 |
| `session_agent/tests.rs` | All tests | ~330 |

**Result:** Main file drops from 1617 to ~400 lines (75% reduction)

---

## 2. user_agent.rs (1267 lines, 33 methods)

### 2.1 Method Inventory by Line Count

| Lines | Method/Section | Purpose |
|-------|----------------|---------|
| 1-84 | Module docs + imports + DEFAULT_SYSTEM_PROMPT | Setup |
| 86-121 | `UserAgent` struct | Agent definition |
| 123-188 | Constructors (3 methods) | Agent creation |
| 190-272 | `default_tools()` | Tool definitions (~80 lines) |
| 274-317 | `build_messages()` | Message building |
| 319-455 | Tool execution methods (4 tools) | Tool handlers |
| 457-544 | `process_autonomous()` | Autonomous execution |
| 546-592 | `resume_autonomous()` | Resume after input |
| 594-640 | `parse_goals()` | Goal parsing |
| 642-699 | `execute_next_action()` | Action execution |
| 701-726 | `classify_error_as_blocker()` | Error classification |
| 728-792 | Blocker helper methods (4 methods) | Blocker handling |
| 794-807 | CompletionDriver accessors | Driver management |
| 809-848 | Memory operations + context accessors | State management |
| 850-965 | `impl Agent for UserAgent` | Trait implementation |
| 967-987 | `format_search_results()` helper | Formatting |
| 989-1267 | Tests (~280 lines) | Unit tests |

### 2.2 Logical Groupings by Responsibility

**Group A: Autonomous Execution (300 lines)**
- `process_autonomous()`
- `resume_autonomous()`
- `parse_goals()`
- `execute_next_action()`

**Group B: Blocker Management (120 lines)**
- `classify_error_as_blocker()`
- `extract_blocker_reason()`
- `classify_blocker_type()`
- `extract_options()`

**Group C: Tool Execution (220 lines)**
- `default_tools()`
- `execute_search_all_memories()`
- `execute_search_memories()`
- `execute_delegate_to_session()`
- `execute_get_session_status()`

**Group D: Agent Core (300 lines)**
- `UserAgent` struct
- Constructors
- `build_messages()`
- `impl Agent` trait
- Accessors

**Group E: Tests (280 lines)**
- MockMemoryStore
- All test functions

### 2.3 Recommended Rust Patterns

**Pattern 1: Module Decomposition**
```
user_agent/
  mod.rs           (~350 lines) - UserAgent struct + Agent trait impl
  autonomous.rs    (~300 lines) - Autonomous execution logic
  blockers.rs      (~120 lines) - Blocker classification
  tools.rs         (~150 lines) - Tool definitions + execution
  tests/
    mod.rs         (~280 lines) - All tests
```

**Pattern 2: Strategy Pattern for Autonomous Mode**
```rust
// autonomous.rs
pub struct AutonomousRunner<'a> {
    agent: &'a mut UserAgent,
    driver: CompletionDriver,
}

impl<'a> AutonomousRunner<'a> {
    pub async fn run(&mut self, initial_request: &str) -> Result<AutonomousResult>;
    pub async fn resume(&mut self, user_input: &str) -> Result<AutonomousResult>;
}
```

**Pattern 3: Blocker Classification as Standalone Module**
```rust
// blockers.rs
pub struct BlockerClassifier;

impl BlockerClassifier {
    pub fn from_error(error: &AgentError) -> Option<Blocker>;
    pub fn from_response(content: &str) -> Option<Blocker>;
    fn extract_reason(content: &str) -> String;
    fn classify_type(content: &str) -> BlockerType;
    fn extract_options(content: &str) -> Vec<String>;
}
```

### 2.4 Proposed Structure

| New File | Contents | Est. Lines |
|----------|----------|------------|
| `user_agent/mod.rs` | UserAgent struct, Agent impl, constructors | ~350 |
| `user_agent/autonomous.rs` | AutonomousRunner, goal parsing, action execution | ~300 |
| `user_agent/blockers.rs` | BlockerClassifier, blocker helpers | ~120 |
| `user_agent/tools.rs` | Tool definitions, execution handlers | ~150 |
| `user_agent/tests.rs` | All tests | ~280 |

**Result:** Main file drops from 1267 to ~350 lines (72% reduction)

---

## 3. eval.rs (1055 lines, 27 methods)

### 3.1 Method Inventory by Line Count

| Lines | Method/Section | Purpose |
|-------|----------------|---------|
| 1-47 | Module docs + imports | Setup |
| 48-99 | `Feedback` struct + 2 methods | Feedback type |
| 101-130 | `FeedbackType` enum + Display | Feedback classification |
| 132-285 | `FeedbackDetector` struct + 7 methods | Pattern-based detection |
| 287-409 | `FeedbackStore` struct + 8 methods | Persistence layer |
| 411-525 | `Improvement` + `ImprovementGenerator` | Suggestion generation |
| 527-548 | `FeedbackSummary` struct | Summary type |
| 550-707 | `AutoEval` struct + 7 methods | Integration point |
| 709-1055 | Tests (~346 lines) | Unit tests |

### 3.2 Logical Groupings by Responsibility

**Group A: Types (130 lines)**
- `Feedback` struct
- `FeedbackType` enum
- `FeedbackSummary` struct

**Group B: Detection (155 lines)**
- `FeedbackDetector` struct
- Pattern matching methods
- Retry detection
- False positive handling

**Group C: Storage (125 lines)**
- `FeedbackStore` struct
- Persistence methods (save/load)
- Query methods

**Group D: Improvement (115 lines)**
- `Improvement` struct
- `ImprovementGenerator` struct
- Analysis logic

**Group E: Integration (160 lines)**
- `AutoEval` struct
- `process_turn()`
- `record_timeout()`
- `summary()`

**Group F: Tests (346 lines)**
- All test functions

### 3.3 Recommended Rust Patterns

**Pattern 1: Module Decomposition**
```
eval/
  mod.rs           (~200 lines) - AutoEval integration + re-exports
  types.rs         (~130 lines) - Feedback, FeedbackType, FeedbackSummary
  detector.rs      (~155 lines) - FeedbackDetector
  store.rs         (~125 lines) - FeedbackStore
  improvement.rs   (~115 lines) - Improvement, ImprovementGenerator
  tests/
    mod.rs         (~346 lines) - All tests
```

**Pattern 2: Trait for Detection Strategy**
```rust
// detector.rs
pub trait FeedbackPattern {
    fn matches(&self, message: &str, context: &str) -> Option<FeedbackType>;
}

pub struct RegexPattern {
    pattern: Regex,
    feedback_type: FeedbackType,
}

impl FeedbackPattern for RegexPattern { ... }
```

**Pattern 3: Builder for AutoEval**
```rust
// mod.rs
pub struct AutoEvalBuilder {
    store_path: Option<PathBuf>,
    detector: Option<FeedbackDetector>,
}

impl AutoEvalBuilder {
    pub fn new() -> Self;
    pub fn with_store_path(self, path: PathBuf) -> Self;
    pub fn with_custom_patterns(self, patterns: Vec<Box<dyn FeedbackPattern>>) -> Self;
    pub fn build(self) -> Result<AutoEval>;
}
```

### 3.4 Proposed Structure

| New File | Contents | Est. Lines |
|----------|----------|------------|
| `eval/mod.rs` | AutoEval struct, integration, re-exports | ~200 |
| `eval/types.rs` | Feedback, FeedbackType, FeedbackSummary | ~130 |
| `eval/detector.rs` | FeedbackDetector, patterns | ~155 |
| `eval/store.rs` | FeedbackStore, persistence | ~125 |
| `eval/improvement.rs` | Improvement, ImprovementGenerator | ~115 |
| `eval/tests.rs` | All tests | ~346 |

**Result:** Main file drops from 1055 to ~200 lines (81% reduction)

---

## 4. change_detector.rs (840 lines, 25 methods)

### 4.1 Method Inventory by Line Count

| Lines | Method/Section | Purpose |
|-------|----------------|---------|
| 1-21 | Module docs + imports | Setup |
| 22-43 | `Significance` enum | Significance levels |
| 45-68 | `ChangeType` enum | Change classifications |
| 70-103 | `ChangeEvent` struct + 3 methods | Event type |
| 105-452 | `ChangeDetector` struct + 15 methods | Main detector |
| 454-544 | `SmartPoller` struct + 6 methods | Adaptive polling |
| 546-559 | `ChangeNotification` struct | Notification type |
| 561-840 | Tests (~280 lines) | Unit tests |

### 4.2 Logical Groupings by Responsibility

**Group A: Types/Enums (80 lines)**
- `Significance` enum
- `ChangeType` enum
- `ChangeEvent` struct
- `ChangeNotification` struct

**Group B: Pattern Detection (225 lines)**
- Pattern definitions (`default_significant_patterns`, `default_ignore_patterns`)
- Pattern matching methods
- `classify_change()`
- `summarize_change()`

**Group C: Core Detection (120 lines)**
- `ChangeDetector` struct
- `detect()`
- `hash_output()`
- `clean_output()`
- `find_new_lines()`

**Group D: Smart Polling (90 lines)**
- `SmartPoller` struct
- Interval management

**Group E: Tests (280 lines)**
- All test functions

### 4.3 Recommended Rust Patterns

**Pattern 1: Module Decomposition (Minimal)**
```
change_detector/
  mod.rs           (~350 lines) - ChangeDetector, SmartPoller, exports
  types.rs         (~80 lines)  - Significance, ChangeType, ChangeEvent, ChangeNotification
  patterns.rs      (~130 lines) - Pattern definitions
  tests.rs         (~280 lines) - All tests
```

**Pattern 2: Pattern Registry**
```rust
// patterns.rs
pub struct PatternRegistry {
    significant: Vec<(Regex, ChangeType, Significance)>,
    ignore: Vec<Regex>,
}

impl PatternRegistry {
    pub fn default() -> Self;
    pub fn with_custom(custom: Vec<PatternDefinition>) -> Self;
}
```

### 4.4 Proposed Structure

| New File | Contents | Est. Lines |
|----------|----------|------------|
| `change_detector/mod.rs` | ChangeDetector, SmartPoller, core methods | ~350 |
| `change_detector/types.rs` | Enums, ChangeEvent, ChangeNotification | ~80 |
| `change_detector/patterns.rs` | Pattern definitions and registry | ~130 |
| `change_detector/tests.rs` | All tests | ~280 |

**Result:** Main file drops from 840 to ~350 lines (58% reduction)

---

## 5. template.rs (813 lines, 23 methods)

### 5.1 Method Inventory by Line Count

| Lines | Method/Section | Purpose |
|-------|----------------|---------|
| 1-34 | Module docs + imports | Setup |
| 35-71 | `AdapterType` enum + impls | Adapter classification |
| 73-198 | `AgentTemplate` struct + 10 methods | Template type |
| 200-316 | `TemplateRegistry` struct + 9 methods | Registry management |
| 318-388 | System prompt constants (3) | Claude Code, MPM, Generic prompts |
| 389-555 | Tool definition functions (3) | claude_code_tools, mpm_tools, generic_tools |
| 557-813 | Tests (~256 lines) | Unit tests |

### 5.2 Logical Groupings by Responsibility

**Group A: Types (165 lines)**
- `AdapterType` enum
- `AgentTemplate` struct
- Builder methods

**Group B: Registry (120 lines)**
- `TemplateRegistry` struct
- Load/save methods
- Registration

**Group C: Prompts (70 lines)**
- System prompt constants

**Group D: Tools (165 lines)**
- `claude_code_tools()`
- `mpm_tools()`
- `generic_tools()`

**Group E: Tests (256 lines)**
- All test functions

### 5.3 Recommended Rust Patterns

**Pattern 1: Module Decomposition (Minimal)**
```
template/
  mod.rs           (~300 lines) - AgentTemplate, TemplateRegistry, re-exports
  adapter_type.rs  (~40 lines)  - AdapterType enum
  prompts.rs       (~70 lines)  - System prompt constants
  tools.rs         (~165 lines) - Tool definition functions
  tests.rs         (~256 lines) - All tests
```

**Pattern 2: Factory Pattern for Templates**
```rust
// mod.rs
pub trait TemplateFactory {
    fn create_template(&self) -> AgentTemplate;
}

pub struct ClaudeCodeFactory;
pub struct MpmFactory;
pub struct GenericFactory;

impl TemplateFactory for ClaudeCodeFactory {
    fn create_template(&self) -> AgentTemplate {
        AgentTemplate::claude_code()
    }
}
```

### 5.4 Proposed Structure

| New File | Contents | Est. Lines |
|----------|----------|------------|
| `template/mod.rs` | AgentTemplate, TemplateRegistry | ~300 |
| `template/adapter_type.rs` | AdapterType enum + impls | ~40 |
| `template/prompts.rs` | System prompt constants | ~70 |
| `template/tools.rs` | Tool definition functions | ~165 |
| `template/tests.rs` | All tests | ~256 |

**Result:** Main file drops from 813 to ~300 lines (63% reduction)

---

## Summary of Recommended Actions

### Priority Order (by reduction needed)

1. **session_agent.rs** (51% reduction) - HIGHEST PRIORITY
   - Extract: state.rs, tools.rs, context.rs, analysis.rs
   - Pattern: Module decomposition + Trait extraction

2. **user_agent.rs** (37% reduction) - HIGH PRIORITY
   - Extract: autonomous.rs, blockers.rs, tools.rs
   - Pattern: Strategy pattern for autonomous mode

3. **eval.rs** (24% reduction) - MEDIUM PRIORITY
   - Extract: types.rs, detector.rs, store.rs, improvement.rs
   - Pattern: Module decomposition + Builder pattern

4. **change_detector.rs** (5% reduction) - LOW PRIORITY
   - Extract: types.rs, patterns.rs
   - Pattern: Pattern registry

5. **template.rs** (2% reduction) - LOW PRIORITY
   - Extract: adapter_type.rs, prompts.rs, tools.rs
   - Pattern: Factory pattern (optional)

### Rust-Idiomatic Patterns Applied

| Pattern | Files Using It | Benefit |
|---------|---------------|---------|
| Module Decomposition | All 5 | Primary organization strategy |
| Trait Extraction | session_agent, user_agent | Dependency injection, testability |
| Builder Pattern | eval | Flexible configuration |
| Strategy Pattern | user_agent | Swappable autonomous behavior |
| Factory Pattern | template | Template creation abstraction |
| Composition | session_agent, user_agent | Smaller, focused structs |

### Estimated Final Line Counts

| File | Before | After (main) | Reduction |
|------|--------|--------------|-----------|
| session_agent/mod.rs | 1617 | ~400 | 75% |
| user_agent/mod.rs | 1267 | ~350 | 72% |
| eval/mod.rs | 1055 | ~200 | 81% |
| change_detector/mod.rs | 840 | ~350 | 58% |
| template/mod.rs | 813 | ~300 | 63% |

All main files will be well under the 800-line target after refactoring.

---

## Implementation Notes

### Test Placement Strategy

For all files, tests should be moved to a `tests/` subdirectory or `tests.rs` file:
- Unit tests stay with the module they test
- Integration tests can be in a `tests/` subdirectory
- Mock implementations (like `MockMemoryStore`) should be in a shared `test_utils.rs`

### Re-export Strategy

Each `mod.rs` should re-export public items for backward compatibility:
```rust
// session_agent/mod.rs
mod state;
mod tools;
mod context;
mod analysis;

pub use state::{SessionState, OutputAnalysis};
pub use tools::SessionToolExecutor;
// ... etc
```

### Migration Path

1. Create the new directory structure
2. Move code to new modules (no behavior changes)
3. Update imports throughout codebase
4. Run tests to verify no regressions
5. Refactor for patterns (trait extraction, etc.) in separate PR

---

*Generated by Research Agent - 2026-02-04*
