# Project: K0maru-Agent-Router

## Architecture
`k0maru-agent-router` is a cleanroom, zero-daemon, single-static-binary Agent Router serving as the second core pillar of the K0maru-Harness modular matrix. It decouples cross-repo bug auto-routing, architecture anti-corruption (permission isolation), third-party outbound staging gate (HITL Outbox), and zero-cognitive-load physical addressing via CWD path matching in terminal multiplexers (Tmux / Herdr).

### Layer Boundaries & Data Flow
1. **L1: Contract & Policy Guard** (`src/envelope.rs`, `src/policy.rs`, `src/outbox.rs`, `src/error.rs`):
   - Ingests `TaskEnvelope` JSON according to `agent-task-v1` schema.
   - Evaluates `BoundaryPolicy`: `TrustedInternal` vs `UntrustedExternal`.
   - `UntrustedExternal` triggers `HitlOutbox` quarantine, generating structured MRE Markdown at `.scratch/staged/issue_<id>.md`.
2. **L2: Deterministic Physical Addressing** (`src/addressing.rs`):
   - Canonicalizes target workspace paths, resolving symlinks, trailing slashes, and relative segments.
   - Matches canonical paths against live sessions.
3. **L3: Multiplexer Driver SPI** (`src/driver.rs`):
   - `MultiplexerDriver` trait abstraction (`list_sessions`, `send_prompt`, `spawn_pane`).
   - `TmuxDriver` POSIX CLI implementation querying live tmux panes and parsing session status (`Idle` vs `Working`).
4. **L4: Coordinator & CLI Entrypoint** (`src/coordinator.rs`, `src/main.rs`, `src/lib.rs`):
   - Orchestrates the full pipeline: Ingest Envelope -> Evaluate Policy -> Quarantine or Resolve Physical Address -> Dispatch/Prompt.
   - CLI subcommands: `status`, `route`, `probe`, `outbox [list|approve|reject]`.

---

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | `agent-task-v1` JSON Schema & Structs | Protocol structs with strict Serde serialization/deserialization | M1 | survey (SPEC §2.1) |
| 2 | Strict Envelope Validation | `validate(&self)` checking non-empty required fields and workspace paths | M1 | survey (R1) |
| 3 | RFC-3339 Timestamp Validation | Strict ISO-8601/RFC-3339 date-time format verification using `chrono` | M1 | survey (SPEC §2.1) |
| 4 | Structured Reproduction Payload | Typed `ReproductionPayload` with `snippet` and `error` strings | M1 | survey (R1, SPEC §2.1) |
| 5 | Typed Error Hierarchy | `thiserror`-based `RouterError` (`InvalidEnvelope`, `IssueNotFound`, etc.) | M1 | survey (R1, AGENTS.md) |
| 6 | Boundary Policy Evaluation | Evaluates `is_external` discriminator to determine trust boundary | M2 | survey (R2) |
| 7 | Untrusted External Quarantine Trigger | Diverts `is_external: true` from dispatch into HITL quarantine | M2 | survey (R2, AGENTS.md) |
| 8 | MRE Markdown Draft Generator | Formats quarantined issues into `.scratch/staged/issue_<id>.md` | M2 | survey (R2, SPEC §2.2) |
| 9 | Outbox Listing Engine | Scans `.scratch/staged/` and extracts summary metadata and approval state | M2 | survey (R2, R4) |
| 10 | Outbox Inspection Engine | Reads and displays full content of staged issue draft for given ID | M2 | survey (R2, R4) |
| 11 | Outbox Human Approval Engine | State transition updating draft status from `Pending` to `Approved` | M2 | survey (R2, R4) |
| 12 | Outbox Human Rejection Engine | Transitions draft status to `Rejected` or deletes staged draft | M2 | survey (R4) |
| 13 | Filesystem Path Canonicalization | Resolves symlinks, `..`, `.`, trailing slashes, and non-existent paths | M3 | survey (R3, SPEC §2.3) |
| 14 | Session State Matching & Resolution | Compares live session canonical CWDs against target to determine Hot/Cold | M3 | survey (R3, SPEC §2.3) |
| 15 | Hot Session Status Resolution | Distinguishes `SessionStatus::Idle` (shell prompt) vs `SessionStatus::Working` | M3 | survey (R3, SPEC §2.3) |
| 16 | `MultiplexerDriver` Trait Contract | Trait with `list_sessions`, `send_prompt`, `spawn_pane` | M3 | survey (R3, SPEC §3) |
| 17 | `TmuxDriver` Implementation | POSIX CLI driver executing tmux commands | M3 | survey (R3, SPEC §3) |
| 18 | Tmux Output Parser | Parses `tmux list-panes` output format into `Vec<LiveSession>` | M3 | survey (R3) |
| 19 | Tmux Server Graceful Detection | Handles non-running tmux server gracefully returning empty list | M3 | survey (R3, edge cases) |
| 20 | Tmux Prompt Submission | Sends keys to target pane (`tmux send-keys -t <pane_id> ...`) | M3 | survey (R3, SPEC §3) |
| 21 | Tmux Pane Spawning | Creates new window/pane in workspace (`tmux new-window -c <workspace>`) | M3 | survey (R3, SPEC §3) |
| 22 | Router Coordinator Pipeline | Orchestrates ingest, policy evaluation, outbox quarantine, addressing | M4 | survey (R4, CONTEXT.md) |
| 23 | `k0maru-router status` Subcommand | Displays router version, supported drivers, outbox counts | M4 | survey (R4) |
| 24 | `k0maru-router route --file <path>` | Ingests envelope, validates, stages or plans dispatch | M4 | survey (R4) |
| 25 | `k0maru-router probe --workspace <path>` | Probes multiplexers and reports `HotSession` or `ColdSpawn` | M4 | survey (R4) |
| 26 | `k0maru-router outbox` Subcommands | CLI commands: `outbox list`, `outbox approve <id>`, `outbox reject <id>` | M4 | survey (R4) |
| 27 | Ergonomic CLI Aliases | Convenience top-level commands `list-staged` and `approve <id>` | M4 | survey (R2, R4) |

---

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | R1: Task Envelope Protocol & Contract Validation | `src/envelope.rs`, `src/error.rs`, unit tests | none | DONE |
| M2 | R2: Boundary Policy Engine & HITL Outbox Gate | `src/policy.rs`, `src/outbox.rs`, unit tests | M1 | DONE |
| M3 | R3: Physical Addressing & Multiplexer Driver SPI | `src/addressing.rs`, `src/driver/mod.rs`, `src/driver/tmux.rs`, unit tests | M1 | DONE |
| M4 | R4: Router Coordinator Engine, CLI & Integration | `src/coordinator.rs`, `src/main.rs`, `src/lib.rs`, unit tests | M1, M2, M3 | DONE |
| M5 | Final Milestone: 100% E2E Test Suite Pass & Adversarial Hardening | Verification of all Tiers 1-4 tests, followed by Tier 5 adversarial coverage hardening | M4, E2E Track | DONE |

### Parallel Track: E2E Testing Track
- **Scope**: Requirement-driven, opaque-box test suite (`tests/`) covering Tiers 1–4 (Feature, Boundary, Cross-Feature, Real-World Scenarios) using `assert_cmd` and `tempfile`. Publishes `TEST_READY.md`.

---

## Interface Contracts

### `envelope.rs` ↔ `policy.rs` & `coordinator.rs`
```rust
pub struct TaskEnvelope {
    pub schema: Option<String>,
    pub id: String,
    pub created_at: String,
    pub caller: CallerInfo,
    pub target: TargetInfo,
    pub intent: String,
    pub payload: TaskPayload,
}

impl TaskEnvelope {
    pub fn from_json_str(json: &str) -> Result<Self, RouterError>;
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, RouterError>;
    pub fn validate(&self) -> Result<(), RouterError>;
}
```

### `policy.rs` ↔ `outbox.rs` & `coordinator.rs`
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    TrustedInternal,
    UntrustedExternal { reason: String },
}

pub struct BoundaryPolicy;
impl BoundaryPolicy {
    pub fn evaluate(envelope: &TaskEnvelope) -> PolicyVerdict;
}

pub struct HitlOutbox;
impl HitlOutbox {
    pub fn stage(envelope: &TaskEnvelope, outbox_dir: &Path) -> Result<PathBuf, RouterError>;
    pub fn list_staged(outbox_dir: &Path) -> Result<Vec<StagedIssueSummary>, RouterError>;
    pub fn inspect_staged(id: &str, outbox_dir: &Path) -> Result<String, RouterError>;
    pub fn approve(id: &str, outbox_dir: &Path) -> Result<PathBuf, RouterError>;
    pub fn reject(id: &str, outbox_dir: &Path) -> Result<(), RouterError>;
}
```

### `addressing.rs` ↔ `driver.rs` & `coordinator.rs`
```rust
pub fn normalize_workspace_path(path: impl AsRef<Path>) -> Result<PathBuf, RouterError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionResolution {
    HotSession(LiveSession),
    ColdSpawn { canonical_path: PathBuf },
}

pub fn resolve_session(
    target_workspace: impl AsRef<Path>,
    live_sessions: &[LiveSession],
) -> Result<SessionResolution, RouterError>;
```

### `driver.rs` ↔ `coordinator.rs`
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Idle,
    Working,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub pane_id: String,
    pub cwd: PathBuf,
    pub status: SessionStatus,
    pub current_command: Option<String>,
}

pub trait MultiplexerDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> bool;
    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError>;
    fn send_prompt(&self, pane_id: &str, prompt: &str) -> Result<(), RouterError>;
    fn spawn_pane(&self, workspace: &Path, command: Option<&str>) -> Result<LiveSession, RouterError>;
}

pub struct TmuxDriver;
impl TmuxDriver {
    pub fn new() -> Self;
}
impl MultiplexerDriver for TmuxDriver { ... }
```

### `coordinator.rs` ↔ CLI (`main.rs`)
```rust
pub struct RouterCoordinator<D: MultiplexerDriver> {
    driver: D,
    outbox_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOutcome {
    Staged { issue_id: String, draft_path: PathBuf },
    HotDispatched { pane_id: String, workspace: PathBuf },
    ColdSpawned { pane_id: String, workspace: PathBuf },
    Planned { resolution: SessionResolution },
}

impl<D: MultiplexerDriver> RouterCoordinator<D> {
    pub fn new(driver: D, outbox_dir: PathBuf) -> Self;
    pub fn route_envelope(&self, envelope: &TaskEnvelope) -> Result<RouteOutcome, RouterError>;
    pub fn probe_workspace(&self, workspace: impl AsRef<Path>) -> Result<SessionResolution, RouterError>;
}
```

---

## Code Layout
```
src/
├── lib.rs                  # Public module exports and prelude
├── error.rs                # Typed RouterError enum via thiserror
├── envelope.rs             # TaskEnvelope data models, Serde, and validation
├── policy.rs               # BoundaryPolicy engine (TrustedInternal vs UntrustedExternal)
├── outbox.rs               # HitlOutbox staging, listing, inspect, approve, reject
├── addressing.rs           # Deterministic path canonicalization and session resolution
├── driver/
│   ├── mod.rs              # MultiplexerDriver trait and session types
│   └── tmux.rs             # TmuxDriver POSIX implementation & output parsing
├── coordinator.rs          # High-level RouterCoordinator pipeline
└── main.rs                 # CLI binary (clap subcommands and formatting)

tests/
├── common/mod.rs           # Shared test fixtures (temp dirs, envelope builders)
├── test_envelope.rs        # R1: Task Envelope validation unit & integration tests
├── test_boundary_outbox.rs # R2: Boundary policy and HITL Outbox gate tests
├── test_addressing_tmux.rs # R3: Physical addressing, symlink & tmux driver tests
└── test_cli_e2e.rs         # R4: Cleanroom CLI integration tests via assert_cmd
```
