# TEST_INFRA.md — k0maru-agent-router Test Infrastructure Specification

This document defines the testing philosophy, methodology, directory architecture, test helpers, and runner commands for `k0maru-agent-router`.

---

## 1. Testing Philosophy & Guiding Principles

1. **Opaque-Box / Requirement-Driven Testing**:
   - All tests are derived directly from authoritative requirements (`ORIGINAL_REQUEST.md` and `PROJECT.md`).
   - Tests assert purely on public contracts, observable behavior, return values, file artifacts, and CLI status codes. Internal implementation details and private methods are never targeted.
2. **Highest Observable Testing Seams**:
   - Primary seams:
     - **L1/L2 Protocol Seam**: `TaskEnvelope::from_json_str`, Serde models, `BoundaryPolicy::evaluate`, `HitlOutbox::stage`.
     - **L3 Driver Seam**: `MultiplexerDriver::list_sessions`, path normalization, session state matching.
     - **L4 Coordinator / CLI Seam**: Execution of the compiled binary (`k0maru-router`) via `assert_cmd`.
3. **Zero Synthetic Mock Fallbacks**:
   - Tests execute against real filesystem fixtures using `tempfile`.
   - Real JSON serialization and parsing are tested; synthetic mocks that bypass parsing or file I/O are strictly prohibited.
4. **Deterministic Isolation**:
   - Each test is hermetic and self-contained, provisioning its own temporary directories and disposing of them cleanly.
   - Tests do not rely on execution order or shared mutable state.

---

## 2. The 4-Tier Testing Methodology

The test suite is structured into four distinct verification tiers:

### Tier 1: Feature Coverage (>= 5 tests per feature)
Verifies the nominal and primary happy-path behavior of each capability in the feature inventory.
- **Task Envelope Protocol (`agent-task-v1`)**:
  - Full envelope deserialization with all schema fields.
  - Serialization round-trip preservation.
  - Optional field omission handling.
  - RFC-3339 timestamp verification.
  - Structured reproduction payload extraction.
  - Target trust flag (`is_external`) preservation.
- **Boundary Policy Engine**:
  - `is_external: false` evaluates to `PolicyVerdict::TrustedInternal`.
  - `is_external: true` evaluates to `PolicyVerdict::UntrustedExternal`.
  - Rejection reason contains target identifier.
- **HITL Outbox Gate**:
  - Automatic creation of quarantine directory (`.scratch/staged/`).
  - Formatting of MRE Markdown draft (`issue_<id>.md`).
  - Inclusion of status banner `*Status: Pending Human Authorization (HITL)*`.
  - Preservation of reproduction JSON blocks and expected outcome.

### Tier 2: Boundary & Corner Cases (>= 5 tests per feature)
Stress-tests boundary conditions, adversarial inputs, resource limits, and error handling.
- **Empty & Truncated Inputs**:
  - Empty string JSON input.
  - Truncated or malformed JSON payloads.
  - Empty string field values for titles, IDs, and paths.
- **Missing Required Fields**:
  - Payloads missing mandatory keys (`id`, `caller`, `target`, `intent`, `payload`).
- **Type Mismatches**:
  - Numeric IDs where strings are expected.
  - Object or boolean mismatches in string fields.
- **Encoding & Escaping Integrity**:
  - Payloads containing Markdown formatting characters (`#`, `*`, `_`, backticks).
  - Payloads containing shell characters (`$(...)`, `&`, `|`, `;`).
  - Payloads containing XML/HTML markup (`<TerraButton aria-label='close' />`).
  - Unicode characters, international text, and emojis.
- **Extreme Sizes & Limits**:
  - Extra-large reproduction payloads (large stack traces).
  - Deeply nested JSON reproduction trees.
- **Filesystem Boundaries**:
  - Directory creation idempotency.
  - Overwriting existing staged draft with updated envelope ID.

### Tier 3: Cross-Feature Combinations
Validates end-to-end integration and data flow across architectural seams.
- **Envelope Ingestion ➔ Policy Evaluation ➔ Outbox Quarantine**:
  - External envelope ingested from JSON string -> evaluated by `BoundaryPolicy` -> quarantined into `HitlOutbox` -> draft verified on disk.
- **Envelope Ingestion ➔ Trusted Fast-Path**:
  - Internal envelope evaluated by `BoundaryPolicy` -> verifies no outbox file is generated.
- **Batch Processing**:
  - Multi-envelope staging ensuring independent non-colliding `issue_<id>.md` artifacts.

### Tier 4: Real-World Scenarios
Simulates realistic production workflows in K0maru-Harness.
- **Scenario A: Upstream 3rd-Party OSS Quarantine**:
  - Consumer app agent discovers bug in external dependency (`Terra-UI` / `vitest`).
  - Produces `agent-task-v1` envelope with reproduction snippet.
  - Router blocks outbound dispatch and stages MRE draft for human sign-off.
- **Scenario B: Internal Monorepo Sister Repository Dispatch**:
  - Internal bug report routed directly without outbox quarantine.
- **Scenario C: CLI End-to-End Execution**:
  - Process execution via `assert_cmd` verifying exit codes and output formatting.

---

## 3. Directory Layout

```
tests/
├── common/
│   └── mod.rs              # Shared test fixtures, envelope builders, CLI runner wrappers
├── test_envelope.rs        # R1: Task Envelope protocol, Serde, schema validation, boundaries
├── test_boundary_outbox.rs # R2: Boundary policy engine, HITL Outbox quarantine & staging
├── test_addressing_tmux.rs # R3: Physical addressing, symlink resolution & tmux driver (M3)
└── test_cli_e2e.rs         # R4: Router Coordinator CLI end-to-end integration (M4)
```

---

## 4. Test Runner Commands

### Standard Test Execution
```bash
# Run all tests in the workspace
cargo test

# Run specific test suites
cargo test --test test_envelope
cargo test --test test_boundary_outbox

# Run tests by tier filter
cargo test tier1_
cargo test tier2_
cargo test tier3_
cargo test tier4_
```

### Full Quality Verification Gate
```bash
# 1. Format check
cargo fmt --check

# 2. Strict linter check (zero warnings)
cargo clippy --all-targets -- -D warnings

# 3. Full test run
cargo test -- --nocapture
```
