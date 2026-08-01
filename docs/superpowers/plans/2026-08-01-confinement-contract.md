# Shared Confinement Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the advisory-only mode placeholder with a truthful, compile-time enforcement request, validated report states, and a fail-closed platform-backend seam without adding dependencies or OS-level `unsafe` code.

**Architecture:** `sandbox::confinement` remains the single public policy module. It distinguishes advisory, pending, and proven-enforced states; production enforced builds route through a private backend seam that initially returns `BackendUnavailable`, while unit backends exercise success and failure paths. OS filters are deliberately excluded from this slice and receive separate human-gated plans.

**Tech Stack:** Rust 2021, Cargo features, existing `sandbox` and `worker-main` crates, standard library only.

## Global Constraints

- Cite ADR-008, ADR-016, ADR-020, ADR-022, SDS §3.1, SDS §12.2, SDS §14 M0, IG AI-4..6, IG CR-Z, GR-1, GR-7, and GR-8.
- Add no dependency and no `unsafe` code in this plan.
- `enforced-confinement` is compile-time only and default-off; there is no environment override.
- An enforcement request without an installed backend fails worker startup; it never falls back to advisory mode.
- `filters_active=true` is valid only with `ConfinementState::Enforced`.
- Do not mark M0 confinement complete or change the milestone tracker to `Met`.
- Preserve the user-owned changes in the original `codex/jobs-scheduler` checkout.

---

### Task 1: Compile-time confinement request

**Files:**
- Modify: `core/sandbox/Cargo.toml`
- Modify: `core/sandbox/src/confinement.rs`

**Interfaces:**
- Produces: `pub enum ConfinementState { Advisory, EnforcementPending, Enforced }`
- Produces: `pub const fn requested_state() -> ConfinementState`
- Consumed by: Task 2 report construction and Task 3 startup policy.

- [ ] **Step 1: Add a test that names the missing build-policy behavior**

Add this test beside the existing confinement tests before defining either symbol. It catches accidentally making enforcement runtime-selectable or default-on.

```rust
#[test]
fn requested_state_matches_compile_time_policy() {
    #[cfg(not(feature = "enforced-confinement"))]
    assert_eq!(requested_state(), ConfinementState::Advisory);

    #[cfg(feature = "enforced-confinement")]
    assert_eq!(
        requested_state(),
        ConfinementState::EnforcementPending
    );
}
```

- [ ] **Step 2: Run the test and verify RED**

Run from `core/`:

```powershell
$env:CARGO_TARGET_DIR = 'F:\Rust Projects\pdf-platform\core\target'
cargo test -p sandbox confinement::tests::requested_state_matches_compile_time_policy -- --exact
```

Expected: compilation fails because `requested_state` and `ConfinementState` do not exist.

- [ ] **Step 3: Add the default-off Cargo feature**

Insert after the package metadata in `core/sandbox/Cargo.toml`:

```toml
[features]
default = []
enforced-confinement = []
```

- [ ] **Step 4: Implement the minimum requested-state API**

Replace `ConfinementMode` and `current_mode()` with:

```rust
/// Observable lifecycle state of worker confinement. [ADR-016, GR-8]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfinementState {
    /// The build logs the intended profile but applies no OS filters.
    Advisory,
    /// This build requires an OS backend to install filters before work starts.
    EnforcementPending,
    /// The selected OS backend reported successful filter installation.
    Enforced,
}

/// Confinement state requested by this build; no runtime override exists.
pub const fn requested_state() -> ConfinementState {
    #[cfg(feature = "enforced-confinement")]
    {
        ConfinementState::EnforcementPending
    }
    #[cfg(not(feature = "enforced-confinement"))]
    {
        ConfinementState::Advisory
    }
}
```

Temporarily update existing internal references from `current_mode()` to
`requested_state()` and from `ConfinementMode` to `ConfinementState`; Task 2
finishes the report semantics.

- [ ] **Step 5: Run default and feature builds and verify GREEN**

```powershell
cargo test -p sandbox confinement::tests::requested_state_matches_compile_time_policy -- --exact
cargo test -p sandbox --features enforced-confinement confinement::tests::requested_state_matches_compile_time_policy -- --exact
```

Expected: one test passes in each command.

- [ ] **Step 6: Commit the independently reviewable build-policy change**

```powershell
git add core/sandbox/Cargo.toml core/sandbox/src/confinement.rs
git commit -m "feat(sandbox): add compile-time confinement request" -m "Cites: ADR-008, ADR-016, SDS §12.2, SDS §14 M0, GR-8"
```

---

### Task 2: Truthful report invariants

**Files:**
- Modify: `core/sandbox/src/confinement.rs`

**Interfaces:**
- Consumes: `ConfinementState`, `requested_state()` from Task 1.
- Produces: `ConfinementReport::validate(&self) -> Result<(), ConfinementError>`.
- Produces: `ConfinementError::InvalidReport(String)`.
- Preserves: `confinement_report()` and `display_text()` for CLI consumers.

- [ ] **Step 1: Write failing invariant tests**

Replace the old `report_never_claims_filters_active_while_advisory` test with these tests. They catch both a false active-filter claim and dishonest user-facing wording.

```rust
#[test]
fn advisory_report_rejects_active_filters() {
    let report = ConfinementReport {
        platform: "test",
        state: ConfinementState::Advisory,
        profile_lines: vec!["test profile".into()],
        filters_active: true,
    };

    assert!(matches!(
        report.validate(),
        Err(ConfinementError::InvalidReport(_))
    ));
}

#[test]
fn pending_report_is_inactive_and_never_claims_sandboxing() {
    let report = ConfinementReport {
        platform: "test",
        state: ConfinementState::EnforcementPending,
        profile_lines: vec!["test profile".into()],
        filters_active: false,
    };

    report.validate().expect("pending report is internally valid");
    let text = report.display_text().to_ascii_lowercase();
    assert!(!text.contains("sandboxed"));
    assert!(text.contains("pending"));
}

#[test]
fn platform_report_matches_requested_state_before_installation() {
    let report = confinement_report();
    assert_eq!(report.state, requested_state());
    assert!(!report.filters_active);
    report.validate().expect("pre-install report must be honest");
}
```

- [ ] **Step 2: Run the tests and verify RED**

```powershell
cargo test -p sandbox confinement::tests::advisory_report_rejects_active_filters -- --exact
```

Expected: compilation fails because `state`, `InvalidReport`, and `validate` do not exist yet.

- [ ] **Step 3: Implement typed report validation**

Add the error variant and display arm:

```rust
InvalidReport(String),
```

```rust
Self::InvalidReport(msg) => write!(f, "invalid confinement report: {msg}"),
```

Change `ConfinementReport.mode` to:

```rust
/// Advisory, pending installation, or proven enforced.
pub state: ConfinementState,
```

Add:

```rust
pub fn validate(&self) -> Result<(), ConfinementError> {
    if self.filters_active != (self.state == ConfinementState::Enforced) {
        return Err(ConfinementError::InvalidReport(format!(
            "state={:?} filters_active={}",
            self.state, self.filters_active
        )));
    }
    Ok(())
}
```

Update `display_text()` to print `state={:?}`. For `Advisory`, append the existing advisory note. For `EnforcementPending`, append:

```text
NOTE: Enforcement requested; OS filters are not active yet
```

Do not append either note for `Enforced`.

- [ ] **Step 4: Make every platform report pre-installation state honestly**

In `confinement_report()`, set:

```rust
let state = requested_state();
```

Use `state` in every platform-specific `ConfinementReport`, keep
`filters_active: false`, and prepend `STATUS: would apply (not yet enforced)`
only for `Advisory`. For `EnforcementPending`, prepend
`STATUS: enforcement pending platform installation`.

- [ ] **Step 5: Run report tests in both configurations and verify GREEN**

```powershell
cargo test -p sandbox report -- --nocapture
cargo test -p sandbox --features enforced-confinement report -- --nocapture
```

Expected: report tests pass in both builds; neither command prints a false
active-filter claim.

- [ ] **Step 6: Commit the report contract**

```powershell
git add core/sandbox/src/confinement.rs
git commit -m "fix(sandbox): make confinement reports stateful and honest" -m "Cites: ADR-016, ADR-020, SDS §12.2, GR-8"
```

---

### Task 3: Fail-closed backend seam

**Files:**
- Modify: `core/sandbox/src/confinement.rs`
- Verify: `core/worker-main/src/main.rs`

**Interfaces:**
- Consumes: report model from Task 2.
- Produces: private `trait PlatformConfinement`.
- Produces: `pub struct ActiveConfinement` with a private report.
- Produces: private `fn install_with<B: PlatformConfinement>(backend: &B) -> Result<ActiveConfinement, ConfinementError>`.
- Produces: `ConfinementError::BackendUnavailable(&'static str)`.
- Changes: `lockdown_worker() -> Result<ConfinementReport, ConfinementError>`.

- [ ] **Step 1: Write failing policy tests with narrow test backends**

Add these inside the existing test module. The doubles replace only the absent
OS operation; assertions exercise the real shared policy.

```rust
struct SuccessfulBackend;

impl PlatformConfinement for SuccessfulBackend {
    fn install(&self) -> Result<Vec<String>, ConfinementError> {
        Ok(vec!["test filters installed".into()])
    }
}

struct FailingBackend;

impl PlatformConfinement for FailingBackend {
    fn install(&self) -> Result<Vec<String>, ConfinementError> {
        Err(ConfinementError::Platform("test rejection".into()))
    }
}

#[test]
fn successful_backend_is_the_only_path_to_active_report() {
    let active = install_with(&SuccessfulBackend).expect("backend succeeds");
    let report = active.report();
    assert_eq!(report.state, ConfinementState::Enforced);
    assert!(report.filters_active);
    report.validate().expect("active report is valid");
}

#[test]
fn backend_failure_is_not_downgraded_to_advisory() {
    let error = install_with(&FailingBackend).expect_err("failure must propagate");
    assert!(matches!(error, ConfinementError::Platform(_)));
}

#[cfg(feature = "enforced-confinement")]
#[test]
fn enforced_build_without_platform_backend_fails_closed() {
    assert!(matches!(
        lockdown_worker(),
        Err(ConfinementError::BackendUnavailable(_))
    ));
}
```

- [ ] **Step 2: Run the seam test and verify RED**

```powershell
cargo test -p sandbox confinement::tests::successful_backend_is_the_only_path_to_active_report -- --exact
```

Expected: compilation fails because `PlatformConfinement` and `install_with`
do not exist.

- [ ] **Step 3: Implement the private backend seam**

Add:

```rust
trait PlatformConfinement {
    fn install(&self) -> Result<Vec<String>, ConfinementError>;
}

/// Proof that the selected platform backend installed all required filters.
pub struct ActiveConfinement {
    report: ConfinementReport,
}

impl ActiveConfinement {
    /// Return the validated active-filter report.
    pub fn report(&self) -> &ConfinementReport {
        &self.report
    }

    /// Consume the proof and return its validated report.
    pub fn into_report(self) -> ConfinementReport {
        self.report
    }
}

fn install_with<B: PlatformConfinement>(
    backend: &B,
) -> Result<ActiveConfinement, ConfinementError> {
    let report = ConfinementReport {
        platform: std::env::consts::OS,
        state: ConfinementState::Enforced,
        profile_lines: backend.install()?,
        filters_active: true,
    };
    report.validate()?;
    Ok(ActiveConfinement { report })
}
```

Add the error variant and display arm:

```rust
BackendUnavailable(&'static str),
```

```rust
Self::BackendUnavailable(platform) => {
    write!(f, "enforced confinement backend unavailable for {platform}")
}
```

- [ ] **Step 4: Make startup fail closed when the feature is selected**

Change `lockdown_worker` to return the report:

```rust
pub fn lockdown_worker() -> Result<ConfinementReport, ConfinementError> {
    let report = confinement_report();
    log_report("worker", &report);

    match report.state {
        ConfinementState::Advisory => Ok(report),
        ConfinementState::EnforcementPending => Err(
            ConfinementError::BackendUnavailable(report.platform)
        ),
        ConfinementState::Enforced => Err(ConfinementError::InvalidReport(
            "pre-install report cannot already be enforced".into()
        )),
    }
}
```

Extract the current logging loop into private
`fn log_report(prefix: &str, report: &ConfinementReport)` so logging behavior
remains unchanged in advisory builds.

The existing worker startup already uses `if let Err(e) = lockdown_worker()`
and exits non-zero, so no production edit to `worker-main/src/main.rs` is
required.

- [ ] **Step 5: Replace the old advisory startup test**

Use configuration-specific assertions:

```rust
#[cfg(not(feature = "enforced-confinement"))]
#[test]
fn advisory_startup_returns_inactive_report() {
    let report = lockdown_worker().expect("advisory startup succeeds");
    assert_eq!(report.state, ConfinementState::Advisory);
    assert!(!report.filters_active);
}
```

Keep `enforced_build_without_platform_backend_fails_closed` from Step 1.

- [ ] **Step 6: Run all sandbox tests in both configurations**

```powershell
cargo test -p sandbox -- --test-threads=1
cargo test -p sandbox --features enforced-confinement -- --test-threads=1
```

Expected: all sandbox tests pass in both commands. The enforced command proves
that a missing backend is an error, not an advisory success.

- [ ] **Step 7: Commit the fail-closed seam**

```powershell
git add core/sandbox/src/confinement.rs
git commit -m "feat(sandbox): fail closed without reviewed OS backend" -m "Cites: ADR-008, ADR-016, SDS §3.1, SDS §12.2, SDS §14 M0, IG AI-6, GR-8"
```

---

### Task 4: Review-package reconciliation and verification

**Files:**
- Modify: `docs/security/confinement-review-package.md`
- Modify: `docs/superpowers/plans/2026-08-01-confinement-contract.md`

**Interfaces:**
- Documents the exact state after Tasks 1–3.
- Does not enable an OS backend or change milestone status.

- [ ] **Step 1: Update the review package truthfully**

Change the current-mode table to state:

```markdown
| Default build | `Advisory`; `filters_active=false` |
| `enforced-confinement` build | Fails closed with `BackendUnavailable` until the reviewed OS slice lands |
| Child lockdown | Shared policy contract active; OS filters not implemented |
| Public claim | “Advisory confinement”; never “sandboxed” |
```

Add `2026-08-01-enforced-worker-confinement-design.md` as the governing design
and list Linux, Windows, and macOS backends as three separate unsigned review
items.

- [ ] **Step 2: Mark completed plan checkboxes**

Change each executed `- [ ]` in this plan to `- [x]`. Do not mark a step until
its command has produced the expected result.

- [ ] **Step 3: Run formatting and targeted verification**

```powershell
cargo fmt --all -- --check
cargo test -p sandbox -- --test-threads=1
cargo test -p sandbox --features enforced-confinement -- --test-threads=1
cargo test -p worker-main --no-fail-fast -- --test-threads=1
```

Expected: all commands exit zero. Pre-existing compiler warnings may remain;
record them without claiming pristine output.

- [ ] **Step 4: Run the full workspace verification**

```powershell
cargo test --workspace --no-fail-fast -- --test-threads=1
```

Expected: exit zero. If the command exceeds the execution window, record it as
incomplete and retain the successful targeted evidence; do not claim a full
workspace pass.

- [ ] **Step 5: Inspect the final diff for scope and honesty**

```powershell
git diff --check main...HEAD
git status --short
git log --oneline -5
```

Expected: only the sandbox contract, its tests, the approved design/plan, and
the review package differ from `main`; no canonical ADR/SDS/PRD/DS file is
modified.

- [ ] **Step 6: Commit the review-package evidence**

```powershell
git add docs/security/confinement-review-package.md docs/superpowers/plans/2026-08-01-confinement-contract.md
git commit -m "docs(sandbox): reconcile fail-closed confinement contract" -m "Cites: ADR-008, ADR-016, ADR-022, SDS §12.2, SDS §14 M0, IG AI-6, IG CR-Z, GR-8"
```

## Self-review result

- Every requirement in the approved design's shared-contract section maps to Tasks 1–4.
- Linux, Windows, and macOS filters are intentionally excluded and remain separate human-gated plans.
- Function names and state variants are consistent across all tasks.
- The plan contains no dependency addition, `unsafe` block, runtime enforcement override, or M0 completion claim.
