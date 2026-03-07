# Functional Core / Imperative Shell Analysis

**Date:** 2026-03-07
**Overall Assessment:** A- (Strong adherence with minor opportunities for improvement)

## Executive Summary

The pr-tracker-rust codebase demonstrates **strong adherence** to the functional core / imperative shell architecture. The separation between pure business logic and I/O operations is clear and well-maintained, with 88% of tests concentrated in pure modules. However, there are a few specific instances where business logic could be further extracted from I/O layers.

## Architecture Overview

### Functional Core (Pure Modules) - ~2,456 lines, 123 tests

Pure modules contain zero I/O dependencies and comprehensive test coverage:

| Module | Lines | Tests | Test Density | Purpose |
|--------|-------|-------|--------------|---------|
| `src/scoring.rs` | 460 | 23 | 5.0% | PR importance scoring algorithm |
| `src/models.rs` | 382 | 12 | 3.1% | Domain models + business logic methods |
| `src/core.rs` | 342 | 6 | 1.8% | Sync diffing algorithm |
| `src/tui/state.rs` | 361 | 22 | 6.1% | TUI state utilities |
| `src/tui/pr_list/state.rs` | 336 | 20 | 6.0% | PR list view state |
| `src/tui/authors/state.rs` | 306 | 20 | 6.5% | Authors view state |
| `src/tui/widgets.rs` | 269 | 20 | 7.4% | UI rendering utilities |

**Average test density:** 5.0%

### Imperative Shell (Impure Modules) - ~2,539 lines, 16 tests

I/O modules handle database, HTTP, and orchestration:

| Module | Lines | Tests | Test Density | Purpose |
|--------|-------|-------|--------------|---------|
| `src/db.rs` | 616 | 0 | 0% | SQLite database operations |
| `src/github/mod.rs` | 413 | 0 | 0% | GitHub API HTTP client |
| `src/github/graphql.rs` | 382 | 4 | 1.0% | GraphQL query definitions |
| `src/service.rs` | 366 | 5 | 1.4% | API ↔ domain transformations |
| `src/cli_app.rs` | 347 | 0 | 0% | CLI command handlers |
| `src/sync.rs` | 293 | 5 | 1.7% | Sync orchestration |
| `src/tui/tasks.rs` | 122 | 2 | 1.6% | Background task spawning |

**Average test density:** 0.6%

### Key Metrics

- **Code distribution:** 41% pure, 42% impure, 17% binaries/infrastructure
- **Test distribution:** 88% in pure modules, 12% in impure modules
- **Pure vs impure test coverage:** **8× better** in pure modules (5.0% vs 0.6%)
- **Total tests:** 139 (all passing)

## Strengths

### ✅ 1. Complete I/O Isolation in Core Logic

The functional core is completely free of I/O dependencies:

```rust
// src/core.rs - Time injected as parameter
pub fn process_pull_request_sync_results(
    previous_prs: &[PullRequest],
    fresh_prs: &[PullRequest],
    now: DateTime<Utc>,  // ✅ Time dependency injected
) -> SyncDiff {
    // Pure diffing logic
}
```

### ✅ 2. Pure Business Logic Methods

Domain models contain only pure transformations:

```rust
// src/models.rs - All methods are pure
impl PullRequest {
    pub fn is_acknowledged(&self) -> bool { /* pure */ }
    pub fn all_changes(&self) -> Vec<ChangeKind> { /* pure */ }
    pub fn should_notify_on_changes(&self) -> bool { /* pure */ }
    pub fn user_is_involved(&self, username: &str) -> bool { /* pure */ }
}
```

### ✅ 3. Excellent Test Coverage in Pure Modules

Pure modules have comprehensive, focused tests:

```rust
// src/scoring.rs - 23 tests for complex scoring logic
#[test]
fn author_scores_higher_than_non_author() { /* ... */ }

#[test]
fn draft_pr_scores_lower_than_ready() { /* ... */ }

#[test]
fn ci_failure_adds_bonus_score() { /* ... */ }
```

### ✅ 4. Clear Repository Pattern

Database operations are cleanly abstracted:

```rust
// src/db.rs - Pure data access, no business logic
impl DatabaseRepository {
    pub async fn save_pr(&self, pr: &PullRequest) -> Result<()>
    pub async fn load_prs(&self) -> Result<Vec<PullRequest>>
    // All methods are thin I/O wrappers
}
```

### ✅ 5. Pure Transformations in Service Layer

API-to-domain conversions are extracted as testable functions:

```rust
// src/service.rs - Pure helper functions (tested)
fn map_ci_status(status: Option<String>) -> CiStatus { /* pure */ }
fn map_approval_status(state: Option<String>) -> ApprovalStatus { /* pure */ }
fn filter_new_prs(prs: Vec<PullRequest>, existing: &[PullRequest]) -> Vec<PullRequest> { /* pure */ }
```

## Weaknesses & Improvement Opportunities

### ⚠️ 1. Time Coupling in TUI Event Handler (High Priority)

**Location:** `src/tui/pr_list/events.rs:68`

**Issue:** Business logic (setting acknowledgment time) mixed with I/O:

```rust
// Current implementation - Mixed concerns
KeyCode::Char('a') => {
    // ... selection logic ...
    let mut pr = shared.prs[pr_index].clone();
    pr.last_acknowledged_at = Some(Utc::now());  // ❌ Direct time coupling
    repo.save_pr(&pr).await?;  // ❌ Business logic + I/O together
    shared.prs[pr_index] = pr;
}
```

**Recommendation:** Extract to pure function in `core.rs` or `models.rs`:

```rust
// Proposed: Pure acknowledgment logic
pub fn acknowledge_pr_at(pr: &PullRequest, time: DateTime<Utc>) -> PullRequest {
    let mut acknowledged = pr.clone();
    acknowledged.last_acknowledged_at = Some(time);
    acknowledged
}

// Event handler becomes orchestration-only
KeyCode::Char('a') => {
    // ... selection logic ...
    let updated_pr = acknowledge_pr_at(&shared.prs[pr_index], Utc::now());
    repo.save_pr(&updated_pr).await?;
    shared.prs[pr_index] = updated_pr;
}
```

**Benefits:**
- Acknowledgment logic becomes testable
- Time can be mocked in tests
- Clear separation of concerns
- Consistent with existing architecture patterns

**Test Example:**
```rust
#[test]
fn acknowledge_pr_at_sets_timestamp() {
    let pr = test_pr();
    let time = Utc.with_ymd_and_hms(2026, 3, 7, 12, 0, 0).unwrap();
    let acknowledged = acknowledge_pr_at(&pr, time);
    assert_eq!(acknowledged.last_acknowledged_at, Some(time));
}
```

### ⚠️ 2. Environment-Based Cutoff Calculation (Medium Priority)

**Location:** `src/sync.rs:17-28`

**Issue:** Configuration reading mixed with time calculation:

```rust
// Current implementation - Environment + time coupling
fn pr_age_cutoff() -> Option<DateTime<Utc>> {
    let days: i64 = std::env::var("PR_TRACKER_MAX_PR_AGE_DAYS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_MAX_PR_AGE_DAYS);

    if days <= 0 {
        return None;
    }

    Some(Utc::now() - chrono::Duration::days(days))  // ❌ Time coupling
}
```

**Recommendation:** Split into pure calculation + configuration reading:

```rust
// Pure calculation (in core.rs or models.rs)
pub fn compute_age_cutoff(
    days: i64,
    current_time: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if days <= 0 {
        return None;
    }
    Some(current_time - chrono::Duration::days(days))
}

// Environment reading (stays in sync.rs)
fn load_max_pr_age_days() -> i64 {
    std::env::var("PR_TRACKER_MAX_PR_AGE_DAYS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_MAX_PR_AGE_DAYS)
}

// Usage
fn pr_age_cutoff() -> Option<DateTime<Utc>> {
    let days = load_max_pr_age_days();
    compute_age_cutoff(days, Utc::now())
}
```

**Benefits:**
- Pure cutoff calculation becomes testable
- Easy to verify edge cases (days = 0, negative values)
- Can test different time scenarios

**Similar pattern exists:** `compute_discovery_cutoff` in `sync.rs:169` already follows this pattern and is tested (5 tests).

### 💡 3. Service Layer Decomposition (Low Priority)

**Location:** `src/service.rs`

**Current State:** Module mixes async I/O operations with pure transformations.

**Pure functions already extracted:**
- `map_ci_status()` (line 117)
- `map_approval_status()` (line 134)
- `map_comments_from_pr()` (line 178)
- `filter_new_prs()` (line 248)

**Recommendation:** Create `src/transformations.rs` module for all pure API-to-domain conversions, leaving `service.rs` as pure orchestration.

**Benefits:**
- Clearer module boundaries
- Easier to find transformation logic
- More isolated testing
- Reusable transformations

**Not urgent:** Current organization is acceptable, but this would improve discoverability.

## Comparison to Architecture Documentation

The `AGENTS.md` file states:

> **Pure (core) modules** — synchronous, zero I/O, heavily tested:
> - `models.rs`, `core.rs`, `scoring.rs`
> - `tui/state.rs`, `tui/widgets.rs`, `tui/navigation.rs`
> - `tui/pr_list/state.rs`, `tui/authors/state.rs`

**Status:** ✅ **Accurate** - All listed modules are indeed pure with zero I/O.

> **Impure (shell) modules** — perform I/O, no unit tests (tested via integration):
> - `db.rs`, `github/`, `sync.rs`, `service.rs`, `cli_app.rs`
> - `tui/app.rs`, `tui/*/events.rs`, `tui/*/render.rs`, `tui/tasks.rs`

**Status:** ✅ **Accurate** - All listed modules perform I/O and have minimal/no unit tests.

> When adding logic, put it in a pure module and test it there. Shell modules should be thin wrappers that call pure functions and perform I/O.

**Status:** ⚠️ **Mostly followed** - One violation in event handler (acknowledgment logic).

## Recommendations Summary

### Immediate Actions (High Priority)

1. **Extract `acknowledge_pr_at()` function**
   - Move acknowledgment logic from event handler to pure module
   - Add tests for acknowledgment behavior
   - **Impact:** Improves testability, maintains architectural consistency
   - **Effort:** 30 minutes
   - **Files:** `src/core.rs` or `src/models.rs`, `src/tui/pr_list/events.rs`

### Short-term Actions (Medium Priority)

2. **Refactor `pr_age_cutoff()` calculation**
   - Extract pure `compute_age_cutoff()` function
   - Add tests for edge cases
   - **Impact:** Better test coverage for configuration logic
   - **Effort:** 20 minutes
   - **Files:** `src/core.rs`, `src/sync.rs`

### Long-term Considerations (Low Priority)

3. **Create `transformations.rs` module**
   - Move all pure API-to-domain conversions from `service.rs`
   - Improves discoverability and module cohesion
   - **Impact:** Better code organization
   - **Effort:** 1-2 hours
   - **Files:** New `src/transformations.rs`, refactor `src/service.rs`

4. **Add integration tests for orchestration**
   - Test `sync.rs` orchestration logic with test doubles
   - Validate concurrent sync behavior
   - **Impact:** Better confidence in orchestration layer
   - **Effort:** 3-4 hours
   - **Files:** New test module or separate integration test suite

## Conclusion

The pr-tracker-rust project demonstrates **excellent adherence** to functional core / imperative shell principles:

- ✅ Clear architectural boundaries
- ✅ Pure business logic thoroughly tested (88% of tests)
- ✅ I/O properly isolated in shell modules
- ✅ Time injection in critical paths
- ⚠️ One significant violation (event handler acknowledgment)
- ⚠️ One minor violation (environment-based cutoff)

**Overall Grade: A-**

The codebase is in a **strong state** architecturally. The recommended improvements are incremental refinements rather than fundamental restructuring. Implementing the high-priority recommendation would bring the codebase to an **A** grade with near-perfect architectural adherence.

## References

- **Pure modules:** `core.rs` (342 lines, 6 tests), `models.rs` (382 lines, 12 tests), `scoring.rs` (460 lines, 23 tests)
- **Test coverage:** 139 total tests, 123 in pure modules (88%)
- **Violations identified:** 2 (1 high priority, 1 medium priority)
- **Code inspection date:** 2026-03-07
