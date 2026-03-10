# AI Commander Core Library Deployment Strategy

**Research Date:** 2026-02-23
**Researcher:** Claude Sonnet 4.5
**Status:** Comprehensive Analysis Complete

---

## Executive Summary

AI Commander is a multi-interface AI session manager with **15 crates** in a Rust workspace at version **0.3.0**. The project consists of 13 library crates and 2 binary crates, with no crates currently published to crates.io. **Recommendation: Phased deployment starting with foundational library crates, NOT full workspace publication.**

**Key Finding:** The project is NOT ready for stable 1.0.0 release, but is ready for 0.3.x patch releases focusing on core library stabilization.

---

## Current State Analysis

### 1. Project Structure

```
Workspace Root: ai-commander v0.3.0
├── Binary Crates (2):
│   ├── ai-commander (TUI/REPL interface)
│   └── commander-telegram (Telegram bot binary)
├── Library Crates (13):
│   ├── Foundational Layer (4):
│   │   ├── commander-models      (data types)
│   │   ├── commander-core        (business logic)
│   │   ├── commander-persistence (storage)
│   │   └── commander-events      (event system)
│   ├── Integration Layer (4):
│   │   ├── commander-adapters    (runtime adapters)
│   │   ├── commander-tmux        (session orchestration)
│   │   ├── commander-memory      (memory management)
│   │   └── commander-agent       (agent interface)
│   ├── Application Layer (3):
│   │   ├── commander-orchestrator (multi-agent)
│   │   ├── commander-work        (work queue)
│   │   └── commander-api         (REST API)
│   └── UI Layer (2):
│       ├── commander-gui         (Tauri/Svelte desktop)
│       └── [commander-telegram]  (also library functions)
```

### 2. Version Information

**Current Version:** `0.3.0` (workspace-level, all crates unified)

**Version History:**
- v0.1.1 - Initial release
- v0.2.0 - Feature additions
- v0.2.1, v0.2.2 - Patch releases
- v0.3.0 - Major feature release (TUI clickable links, Telegram inline keyboards, Forum Topics, GUI MVP)

**Version Strategy:** Workspace-level version pinning (all crates share same version)

**Semantic Versioning Status:**
- **0.x.x indicates:** Pre-stable API, breaking changes allowed
- **0.3.0 means:** Third minor iteration, NOT production-ready
- **Breaking changes expected:** Yes, API still evolving

### 3. Dependency Hierarchy

**Dependency Layers (bottom-up):**

```
Layer 0 (No internal deps):
  - commander-models

Layer 1 (depends on models only):
  - commander-persistence
  - commander-adapters

Layer 2 (depends on Layer 0-1):
  - commander-events (models + persistence)
  - commander-work (models + persistence)
  - commander-core (no commander- deps, only external)
  - commander-tmux (no commander- deps)

Layer 3 (depends on Layer 0-2):
  - commander-memory (core)
  - commander-agent (core, memory)
  - commander-runtime (adapters, models, tmux)

Layer 4 (depends on Layer 0-3):
  - commander-orchestrator (agent, core, memory)
  - commander-api (adapters, events, models, persistence, runtime, work)

Layer 5 (top-level binaries):
  - commander-telegram (adapters, core, models, orchestrator, persistence, tmux)
  - commander-gui (adapters, core, models, persistence, telegram, tmux)
  - ai-commander (ALL 10 crates)
```

**Circular Dependencies:** NONE identified ✅

**Critical Path:** `models -> persistence -> core -> agent -> orchestrator`

### 4. Publication Status

**Published to crates.io:** ❌ NONE

```bash
$ cargo search ai-commander
# No results
```

**Homebrew Formula:** ✅ YES
```bash
brew tap bobmatnyc/tools
brew install ai-commander
```

**Distribution Channels:**
- ✅ Homebrew (macOS binary distribution)
- ✅ Source (cargo install --path)
- ❌ crates.io (not published)
- ❌ Binary releases (no GitHub releases configured)

### 5. CI/CD Status

**GitHub Actions:** ❌ NOT CONFIGURED

```bash
$ ls .github/
# Directory does not exist
```

**Missing Automation:**
- No automated testing pipeline
- No automated releases
- No dependency updates (Dependabot)
- No security scanning
- No release process automation

**Test Status:**
```bash
$ cargo test --workspace
# Workspace compiles successfully
# 293 tests passing (as documented in CHANGELOG)
# 8 tests ignored (tmux integration tests require environment)
```

### 6. Release Process

**Current Process:** ❌ MANUAL, UNDOCUMENTED

**No evidence of:**
- Release checklist
- Automated changelog generation
- Version bumping automation
- Tag creation workflow
- Binary distribution automation

**Existing Documentation:**
- ✅ CHANGELOG.md (comprehensive, follows Keep a Changelog)
- ❌ RELEASING.md (does not exist)
- ✅ README.md (comprehensive installation/usage docs)
- ❌ CONTRIBUTING.md (does not exist)

---

## Target Deployment Channels

### 1. Library Crates (crates.io)

**Purpose:** Enable developers to build on AI Commander infrastructure

**Target Audience:**
- Rust developers building AI session managers
- Teams integrating AI workflows
- Plugin/extension developers

**Deployment Priority:**
```
HIGH PRIORITY (Core Infrastructure):
1. commander-models       # Foundation for all crates
2. commander-persistence  # Stable storage layer
3. commander-core         # Business logic
4. commander-events       # Event system

MEDIUM PRIORITY (Integrations):
5. commander-adapters     # Runtime adapter system
6. commander-tmux         # Tmux orchestration
7. commander-work         # Work queue
8. commander-memory       # Memory management

LOW PRIORITY (Application-Level):
9. commander-agent        # Agent interface
10. commander-orchestrator # Multi-agent coordination
11. commander-api         # REST API
12. commander-runtime     # Async runtime
```

**GUI/Telegram:** ❌ DO NOT PUBLISH (binaries, not libraries)

### 2. Binary Distribution

**Current Channels:**
- ✅ Homebrew (macOS) - ACTIVE
- ❌ cargo install ai-commander (requires crates.io publication)
- ❌ GitHub Releases (binaries) - NOT CONFIGURED
- ❌ Docker images - NOT AVAILABLE

**Target Channels:**
```
IMMEDIATE (Next Release):
- cargo install ai-commander (after crates.io publication)
- GitHub Releases (Linux/macOS/Windows binaries)

FUTURE (Post-1.0):
- Docker images (multi-arch)
- Nix packages
- Arch AUR
- Debian/Ubuntu packages
- Chocolatey (Windows)
```

### 3. End-User Distribution

**Current:**
- Homebrew users (macOS developers)
- Source users (cargo install --path)

**Target:**
- Cross-platform desktop users (GUI via GitHub Releases)
- Mobile users (Telegram bot, no app needed)
- CLI power users (cargo install)

---

## Stability Assessment

### What Should Be Published?

**YES - Ready for 0.3.x publication:**
```
✅ commander-models       # Stable data types, 0 breaking changes since 0.3.0
✅ commander-persistence  # Atomic JSON storage, stable API
✅ commander-core         # Config/filtering logic, stable
✅ commander-events       # Event pub/sub, stable API
```

**MAYBE - Needs API review before publication:**
```
⚠️ commander-adapters     # Adapter trait stable? Review breaking changes
⚠️ commander-tmux         # Tmux API stable? Test coverage sufficient?
⚠️ commander-work         # Work queue API finalized?
⚠️ commander-memory       # Memory management API stable?
```

**NO - Not ready for publication:**
```
❌ commander-agent        # Depends on orchestrator, high-level abstraction
❌ commander-orchestrator # Multi-agent coordination, complex/evolving
❌ commander-api          # REST API surface area still growing
❌ commander-runtime      # Async runtime patterns still settling
❌ commander-gui          # Binary, not library
❌ commander-telegram     # Binary, not library (though has lib functions)
```

### What's the Current Version Strategy?

**Observed Pattern:** Workspace-level versioning (all crates locked to same version)

**Pros:**
- Simple to manage
- Clear release cadence
- Easy for users (no version matrix)

**Cons:**
- All crates bumped together (even if unchanged)
- Breaking changes in one crate force version bump for all
- Harder to communicate stability per-crate

**Recommendation:** Continue workspace versioning for 0.x, consider independent versioning post-1.0

### Are Versions Consistent?

**✅ YES** - All crates use `version.workspace = true`

```toml
[workspace.package]
version = "0.3.0"
```

All 15 crates share version 0.3.0.

### Is There a Workspace-Level Version?

**✅ YES** - Defined in root Cargo.toml

### Are Any Already Published?

**❌ NO** - Zero crates published to crates.io

### Release History Analysis

**Git Tags:**
```
v0.1.1 - Initial release
v0.2.0 - Feature additions
v0.2.1, v0.2.2 - Patch releases
v0.3.0 - Current (Major features: GUI, Telegram improvements, TUI enhancements)
```

**Changelog Insights:**
- v0.3.0: Significant feature additions (GUI MVP, Telegram Forum Topics, clickable links)
- v0.1.0: Initial modular architecture (9 crates, 293 tests)
- Versioning follows semver for 0.x (breaking changes allowed)

**Release Frequency:** Irregular (not on schedule, feature-driven)

**Release Process:** Manual (no automation detected)

---

## Recommended Deployment Strategy

### Phase 1: Foundation Layer (0.3.1 - Immediate)

**Goal:** Establish stable core libraries on crates.io

**Crates to Publish:**
```
1. commander-models v0.3.1
2. commander-persistence v0.3.1
3. commander-core v0.3.1
4. commander-events v0.3.1
```

**Prerequisites:**
- ✅ API review for each crate (ensure no planned breaking changes)
- ✅ Documentation audit (rustdoc for all public APIs)
- ✅ Add crate-level metadata (keywords, categories, repository, documentation links)
- ✅ Add LICENSE file to each crate
- ✅ Verify transitive dependencies are reasonable

**Publish Order (respects dependency graph):**
```bash
cargo publish -p commander-models
cargo publish -p commander-persistence
cargo publish -p commander-core
cargo publish -p commander-events
```

**Timeline:** 1-2 weeks (includes API review, documentation)

### Phase 2: Integration Layer (0.3.2-0.3.5)

**Goal:** Enable third-party integrations

**Crates to Publish (in order):**
```
5. commander-adapters v0.3.2
   - API Review: Adapter trait stability
   - Documentation: Adapter implementation guide
   - Examples: Custom adapter example

6. commander-tmux v0.3.3
   - API Review: Tmux orchestration API
   - Testing: Expand integration tests
   - Documentation: Tmux session management guide

7. commander-work v0.3.4
   - API Review: Work queue API finalized
   - Examples: Work item usage patterns

8. commander-memory v0.3.5
   - API Review: Memory management stable
   - Documentation: Memory configuration guide
```

**Prerequisites per Crate:**
- Dependency on previously published crates only
- API stability review
- Test coverage >80%
- Rustdoc for all public APIs
- At least one usage example

**Timeline:** 2-3 months (1 crate every 2-3 weeks, allows for API stabilization)

### Phase 3: Application Layer (0.4.0)

**Goal:** Provide high-level abstractions for AI agents

**Crates to Publish:**
```
9. commander-agent v0.4.0
   - Breaking changes expected (hence 0.4.0 bump)
   - Depends on stabilized core/memory/orchestrator

10. commander-orchestrator v0.4.0
    - Multi-agent coordination patterns
    - High-level abstraction, more evolution expected

11. commander-runtime v0.4.0
    - Async runtime patterns
    - Depends on adapters/tmux stabilization

12. commander-api v0.4.0
    - REST API surface area
    - OpenAPI spec generation
```

**Timeline:** 3-6 months (after integration layer stable)

**Note:** These crates are higher-level abstractions with more expected evolution. Recommend 0.4.0 version bump to signal potential breaking changes.

### Phase 4: Binary Distribution (Parallel Track)

**Goal:** Distribute end-user binaries

**Actions:**
1. **Publish ai-commander to crates.io (0.3.1)**
   - Requires Phase 1 completion (dependency crates published)
   - Enables `cargo install ai-commander`

2. **Setup GitHub Actions CI/CD**
   ```yaml
   .github/workflows/release.yml:
     - Run tests on push
     - Build binaries on tags (Linux/macOS/Windows)
     - Create GitHub Release with binaries
     - Update Homebrew formula automatically
   ```

3. **First GitHub Release (v0.3.1)**
   - Binary artifacts: ai-commander-{linux,macos,windows}-{arch}
   - Changelog auto-generated from commits
   - Homebrew formula update (via actions)

4. **commander-telegram binary distribution**
   - Same process as ai-commander
   - Separate binaries or single unified binary?
   - Consider: Should telegram be subcommand of ai-commander?

**Timeline:** Parallel with Phase 1 (can proceed once ci-commander depends on published crates)

### Phase 5: Ecosystem Growth (Post-0.5.0)

**Goal:** Enable third-party ecosystem

**Actions:**
- Publish examples repository
- Create adapter template/generator
- Documentation site (docs.rs + custom site)
- Plugin marketplace/registry
- Community contributions guide

**Timeline:** 6-12 months

---

## Prerequisites for Stable (1.0.0) Release

### API Stability

**Current Status:** ❌ NOT READY

**Blockers:**
1. **Breaking changes expected** in orchestrator/agent layer
2. **Adapter trait** needs finalization (extensibility vs stability)
3. **Event system** needs production validation
4. **Work queue** API patterns need field testing
5. **Memory management** API stabilization

**Requirements for 1.0.0:**
- 6+ months of 0.x releases with no breaking changes
- Real-world usage feedback
- Public API documented and frozen
- Deprecation policy established

### Testing

**Current Status:** ⚠️ PARTIAL

**Current State:**
- ✅ 293 tests passing
- ⚠️ 8 tests ignored (tmux integration)
- ❌ No coverage metrics
- ❌ No integration test suite
- ❌ No end-to-end tests

**Requirements for 1.0.0:**
- ✅ Unit test coverage >80%
- ✅ Integration tests for all adapters
- ✅ End-to-end workflow tests
- ✅ Tmux integration tests in CI
- ✅ Performance benchmarks
- ✅ Stress testing (long-running sessions)

### Documentation

**Current Status:** ⚠️ PARTIAL

**Exists:**
- ✅ README.md (comprehensive)
- ✅ CHANGELOG.md (detailed)
- ✅ Architecture diagrams
- ✅ GUI documentation (docs/GUI.md)

**Missing:**
- ❌ API documentation (rustdoc incomplete)
- ❌ Contribution guidelines
- ❌ Releasing guidelines
- ❌ Security policy
- ❌ Adapter development guide
- ❌ Integration examples

**Requirements for 1.0.0:**
- ✅ Rustdoc for ALL public APIs
- ✅ Usage examples for each crate
- ✅ Migration guides between versions
- ✅ Troubleshooting guide
- ✅ Performance tuning guide

### Production Validation

**Current Status:** ❌ NOT VALIDATED

**Needed:**
- Production deployment case studies
- Performance benchmarks under load
- Security audit
- Dependency audit
- Edge case handling validation
- Error recovery patterns validated

**Requirements for 1.0.0:**
- 6+ months of production usage
- At least 3 organizations using in production
- Security audit completed
- Performance benchmarks published
- Known issues documented

### Deprecation Policy

**Current Status:** ❌ NOT DEFINED

**Requirements for 1.0.0:**
- Deprecation timeline (e.g., 2 minor versions before removal)
- Migration path documentation
- Deprecation warnings in code
- Changelog deprecation section

---

## Step-by-Step Release Plan

### Immediate Actions (Week 1-2)

**1. Pre-Publication Audit**

```bash
# For each Phase 1 crate (models, persistence, core, events):

# 1. API Review
- Review public API surface
- Identify any planned breaking changes
- Document stability guarantees
- Check for accidental pub exports

# 2. Documentation Audit
- Ensure all public items have rustdoc
- Add crate-level documentation (lib.rs)
- Add usage examples
- Document error cases

# 3. Metadata Addition (Cargo.toml)
[package]
name = "commander-models"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "Core data models for AI Commander"
repository = "https://github.com/bobmatnyc/ai-commander"
documentation = "https://docs.rs/commander-models"
homepage = "https://github.com/bobmatnyc/ai-commander"
keywords = ["ai", "session-manager", "data-models"]
categories = ["data-structures", "development-tools"]
readme = "README.md"  # Add crate-specific README

# 4. Licensing
- Ensure LICENSE file exists in workspace root
- Add license headers to source files (optional but recommended)

# 5. Dependency Review
- Audit dependency versions (any yanked crates?)
- Review transitive dependencies
- Ensure no dev-dependencies leak into [dependencies]
```

**2. Create Release Checklist**

```markdown
# Release Checklist Template (docs/RELEASING.md)

## Pre-Release
- [ ] All tests passing (`cargo test --workspace`)
- [ ] No clippy warnings (`cargo clippy --workspace`)
- [ ] Documentation builds (`cargo doc --workspace --no-deps`)
- [ ] CHANGELOG.md updated
- [ ] Version bumped in Cargo.toml
- [ ] API review completed (for new crates)

## Publication
- [ ] Publish crates in dependency order
- [ ] Verify publication on crates.io
- [ ] Create git tag (vX.Y.Z)
- [ ] Push tag to GitHub

## Post-Release
- [ ] Create GitHub Release with changelog
- [ ] Update Homebrew formula (if binary release)
- [ ] Announce in community channels
- [ ] Update documentation site
```

**3. Setup CI/CD Pipeline**

```yaml
# .github/workflows/ci.yml (create this)
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo fmt --check

  # Add tmux tests (Linux/macOS only)
  tmux-integration:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: sudo apt-get install tmux
      - run: cargo test -p commander-tmux -- --ignored
```

```yaml
# .github/workflows/release.yml (create this)
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  create-release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/create-gh-release-action@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}

  upload-binaries:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
          - os: macos-latest
            target: x86_64-apple-darwin
          - os: macos-latest
            target: aarch64-apple-darwin
          - os: windows-latest
            target: x86_64-pc-windows-msvc
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/upload-rust-binary-action@v1
        with:
          bin: ai-commander
          target: ${{ matrix.target }}
          tar: unix
          zip: windows
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### Phase 1 Execution (Week 3-4)

**1. Version Bump to 0.3.1**

```bash
# Update workspace version
# Cargo.toml
[workspace.package]
version = "0.3.1"  # Bump patch version

# Update CHANGELOG.md
## [0.3.1] - 2026-03-01

### Added
- Published core library crates to crates.io:
  - commander-models
  - commander-persistence
  - commander-core
  - commander-events

### Changed
- Enhanced documentation for all public APIs
- Added crate-level READMEs

### Fixed
- (list any bugfixes)
```

**2. Publish Crates (In Order)**

```bash
# IMPORTANT: Publish in dependency order!

# 1. Foundation (no internal dependencies)
cargo publish -p commander-models
# Wait for crates.io indexing (~5-10 minutes)

# 2. Layer 1 (depends on models)
cargo publish -p commander-persistence
# Wait for indexing

# 3. Layer 2 (depends on layers 0-1)
cargo publish -p commander-core
cargo publish -p commander-events
# Wait between each

# Verification
cargo search commander-models  # Should show published crate
```

**3. Create Git Tag and Release**

```bash
# Create annotated tag
git tag -a v0.3.1 -m "Release v0.3.1: Core library crates published to crates.io"

# Push tag
git push origin v0.3.1

# GitHub Actions will automatically:
# - Create GitHub Release
# - Build and upload binaries
# - Generate changelog
```

**4. Update Documentation**

```bash
# Update README.md to reflect published crates
## Installation

### Library Crates (Rust Developers)

```toml
# Cargo.toml
[dependencies]
commander-models = "0.3.1"
commander-persistence = "0.3.1"
commander-core = "0.3.1"
commander-events = "0.3.1"
```

### Binary (End Users)

```bash
# via cargo (NEW!)
cargo install ai-commander

# via Homebrew (macOS)
brew install bobmatnyc/tools/ai-commander

# via GitHub Releases
# Download binary from: https://github.com/bobmatnyc/ai-commander/releases/latest
```
```

**5. Verify Publication**

```bash
# Test installation from published crates
cd /tmp
cargo new test-commander
cd test-commander

# Add dependency
echo '[dependencies]
commander-models = "0.3.1"' >> Cargo.toml

# Test build
cargo build
# Should download from crates.io and build successfully

# Test binary installation
cargo install ai-commander
# Should install successfully

# Verify version
ai-commander --version
# Should show: ai-commander 0.3.1
```

### Ongoing (Post-Phase 1)

**1. Monitor for Issues**

- Watch crates.io download counts
- Monitor GitHub issues for bugs
- Track community feedback (Discord, Reddit, etc.)
- Review API usage patterns

**2. Iterate Based on Feedback**

- Patch releases (0.3.2, 0.3.3, etc.) for bugfixes
- Minor releases (0.4.0) for new features
- API adjustments in response to usage patterns

**3. Prepare for Phase 2**

- Begin API review for integration layer crates
- Expand test coverage
- Improve documentation
- Gather production usage feedback

---

## Risks and Mitigation

### Risk 1: API Breaking Changes Required

**Probability:** HIGH (0.x version indicates unstable API)

**Impact:** Ecosystem fragmentation, user frustration

**Mitigation:**
- Clearly communicate 0.x = unstable in docs
- Provide migration guides for each breaking change
- Use semver strictly (0.4.0 for breaking changes)
- Deprecate before removing (when possible)
- Maintain CHANGELOG.md with breaking changes section

### Risk 2: Dependency Conflicts

**Probability:** MEDIUM (complex dependency graph)

**Impact:** Users unable to integrate crates

**Mitigation:**
- Audit dependencies before publication
- Use conservative dependency version ranges
- Test with minimal dependency versions
- Document known conflicts
- Consider vendoring problematic dependencies

### Risk 3: Publication Irreversibility

**Probability:** LOW (but high impact)

**Impact:** Published crate versions are permanent (cannot delete)

**Mitigation:**
- Thorough pre-publication testing
- Use cargo publish --dry-run first
- Yank if critical bugs found (doesn't delete, but warns users)
- Have rollback communication plan

### Risk 4: Maintenance Burden

**Probability:** HIGH (15 crates = significant surface area)

**Impact:** Slow response to issues, user frustration

**Mitigation:**
- Start with 4 crates (Phase 1), validate maintenance load
- Automate testing, releases, documentation
- Establish contribution guidelines
- Consider crate consolidation if maintenance becomes unsustainable
- Set clear SLA expectations (0.x = best-effort)

### Risk 5: Premature 1.0.0 Declaration

**Probability:** MEDIUM (pressure to declare stability)

**Impact:** API locked too early, forced to break semver promises

**Mitigation:**
- Resist pressure to declare 1.0.0 prematurely
- Clearly document 1.0.0 criteria (6+ months stable API)
- Communicate "production-ready" does not require 1.0.0
- Use 0.x versions until confident API is frozen
- Gather real-world usage data before 1.0.0

---

## Timeline Summary

```
Month 1 (March 2026):
├─ Week 1-2: Pre-publication audit, CI/CD setup
├─ Week 3-4: Phase 1 publication (models, persistence, core, events)
└─ Week 4: Verify installation, monitor issues

Month 2-4 (April-June 2026):
├─ Phase 2 preparation: API reviews for integration layer
├─ Patch releases (0.3.2-0.3.5) as needed for Phase 1 crates
└─ Begin integration layer publication (adapters, tmux, work, memory)

Month 5-7 (July-September 2026):
├─ Phase 3 preparation: API stabilization for application layer
├─ Version bump to 0.4.0 (breaking changes)
└─ Publication of agent, orchestrator, runtime, api

Month 8-12 (October 2026 - February 2027):
├─ Production validation period
├─ Patch releases for stability
├─ Ecosystem growth (plugins, examples, documentation site)
└─ Evaluate 1.0.0 readiness (if 6+ months stable)

Year 2+ (March 2027+):
├─ 1.0.0 release (if criteria met)
├─ Long-term support (LTS) policy
└─ Ecosystem maturity
```

---

## Open Questions

### 1. Should commander-telegram remain a binary-only crate?

**Current:** `commander-telegram` has both binary (bot) and library functions (daemon management)

**Options:**
- **A) Keep as-is:** Binary with library functions (current state)
- **B) Split:** `commander-telegram` (lib) + `telegram-bot` (bin)
- **C) Absorb:** Move daemon code to `commander-core`, keep only binary

**Recommendation:** **Option A** (keep as-is) for 0.3.x, revisit for 0.4.0

**Rationale:**
- Splitting requires significant refactoring
- Current structure works for existing users
- Can revisit after gathering usage patterns

### 2. Should GUI be distributed separately?

**Current:** GUI is a crate in workspace but distributed via GitHub Releases

**Options:**
- **A) Separate repository:** Move GUI to own repo with releases
- **B) Keep in workspace:** Maintain as part of monorepo
- **C) Submodule:** GUI as git submodule of main repo

**Recommendation:** **Option B** (keep in workspace) for now

**Rationale:**
- Simplifies development (shared crates)
- GUI is tightly coupled to core libraries
- Separate releases are possible within monorepo

### 3. What's the target audience for published libraries?

**Options:**
- **A) Rust developers only:** Assume Rust expertise
- **B) AI researchers/engineers:** Prioritize high-level abstractions
- **C) Plugin developers:** Focus on extensibility

**Recommendation:** **All three, prioritized:**
1. Plugin developers (Phase 1-2: adapters, events, models)
2. AI engineers (Phase 3: orchestrator, agent, runtime)
3. Rust developers (General Rust ecosystem)

**Rationale:**
- Adapters/plugins are lowest barrier to entry
- High-level abstractions benefit AI engineers
- General Rust ecosystem benefits from mature 1.0.0 crates

### 4. Should version numbers be unified or independent?

**Current:** Workspace-level versioning (all crates share version)

**Future Options:**
- **A) Continue unified:** Simple, clear, easier to manage
- **B) Independent per crate:** More flexible, better signals stability
- **C) Hybrid:** Core crates unified, others independent

**Recommendation:** **Option A (unified)** for 0.x, **Option C (hybrid)** for 1.0+

**Rationale:**
- Unified versioning simplifies 0.x iteration
- Post-1.0.0, independent versioning signals stability better
- Hybrid allows core stability + feature evolution

---

## Success Metrics

### Phase 1 Success Criteria

- ✅ 4 crates published to crates.io
- ✅ `cargo install ai-commander` works
- ✅ GitHub Actions CI/CD operational
- ✅ At least 1 GitHub Release with binaries
- ✅ Documentation updated (README, crate READMEs)
- ✅ No critical bugs in published crates (0 yanked versions)

### Phase 2 Success Criteria

- ✅ 8 total crates published (Phase 1 + 4 integration layer)
- ✅ At least 1 community-contributed adapter
- ✅ Test coverage >70% for published crates
- ✅ Rustdoc completeness >90%
- ✅ Download count >100 per crate on crates.io

### Phase 3 Success Criteria

- ✅ 12 total crates published (all library crates)
- ✅ At least 3 community projects using published crates
- ✅ API stability (no breaking changes for 3+ months)
- ✅ Production usage validation (at least 1 organization)
- ✅ Documentation site launched

### 1.0.0 Success Criteria

- ✅ 6+ months of stable API (no breaking changes)
- ✅ 3+ organizations using in production
- ✅ Security audit completed
- ✅ Test coverage >80%
- ✅ Rustdoc completeness 100%
- ✅ Performance benchmarks published
- ✅ Deprecation policy documented
- ✅ Migration guides for all versions
- ✅ Community governance model established

---

## Conclusion

AI Commander is a well-structured Rust project with 15 crates, currently at version 0.3.0 and NOT YET published to crates.io. The project is **NOT ready for 1.0.0 release** but is **ready for phased 0.3.x library publication** starting with the foundational layer.

**Recommended Approach:**
1. **Immediate:** Publish 4 core crates (models, persistence, core, events) as 0.3.1
2. **Short-term:** Establish CI/CD, GitHub Releases, and monitoring
3. **Medium-term:** Publish integration layer (adapters, tmux, work, memory) as 0.3.x patches
4. **Long-term:** Stabilize application layer (agent, orchestrator, api) as 0.4.0
5. **Future:** Validate in production for 6+ months before 1.0.0

**Key Insight:** Phased publication reduces risk, allows API iteration, and builds ecosystem gradually. Starting with 4 crates is manageable and tests publication infrastructure before committing to full workspace distribution.

**Next Steps:**
1. Conduct API review for Phase 1 crates (models, persistence, core, events)
2. Setup GitHub Actions CI/CD pipelines
3. Create RELEASING.md documentation
4. Publish Phase 1 crates as v0.3.1
5. Monitor usage and iterate based on feedback

---

**Research Complete.**
Saved: `docs/research/core-library-deployment-strategy-2026-02-23.md`
