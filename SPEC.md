# SPEC.md — k0maru-agent-router Technical Specification

Version: 0.1.0-draft  
Status: Active Spec  
Target Architecture: K0maru-Harness Second Core Pillar  

---

## 1. Overview & Architectural Blueprint

`k0maru-agent-router` is a cleanroom, single-static-binary agent routing microkernel designed to bridge independent coding agents across distinct workspaces, repositories, and multiplexer panes.

```
k0maru-agent-router
├── L1: Contract & Policy Guard (TaskEnvelope, BoundaryPolicy, HitlOutbox)
├── L2: Decision Engine SPI (DeterministicRule, SemanticVector, DecisionModel)
├── L3: Multiplexer Driver SPI (HerdrDriver, TmuxDriver, SubprocessDriver)
└── L4: Execution Bus & Memory Preload (Receipt, k0maru-agent-memory hook)
```

---

## 2. Core Protocols & Data Models

### 2.1 Task Envelope Protocol (`agent-task-v1`)

Every cross-agent or cross-workspace request is encapsulated in an immutable Task Envelope:

```json
{
  "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
  "id": "task_9f82a1b4",
  "created_at": "2026-10-11T18:30:00Z",
  "caller": {
    "workspace": "/Users/k0maru3/workspace/consumer-app",
    "agent": "agy",
    "pane_id": "%1"
  },
  "target": {
    "identifier": "Terra-UI",
    "workspace": "/Users/k0maru3/workspace/terra-ui",
    "is_external": false
  },
  "intent": "bug_report",
  "payload": {
    "title": "TerraButton missing aria-label attribute pass-through",
    "severity": "blocking",
    "reproduction": {
      "snippet": "<TerraButton aria-label='close' />",
      "error": "HTML button element does not receive aria-label"
    },
    "expected": "Attribute forwarded to rendered <button> element"
  }
}
```

### 2.2 Boundary Policy & HITL Outbox Gate

- **Trusted Targets (`is_external == false`)**:
  - Target workspace belongs to local/internal repositories (e.g., sister projects in `~/workspace/*`).
  - Forwarded directly to Router Core for multiplexer dispatch.
- **Untrusted / External Targets (`is_external == true`)**:
  - Targets external open-source repositories (e.g. React, Vitest, upstream libraries).
  - Outbound dispatch to external APIs/GitHub is strictly blocked.
  - Generates a structured MRE draft under `.scratch/staged/issue_<id>.md`.
  - Requires explicit human approval via CLI command: `k0maru-router approve <id>` before any outbound action can be taken.

### 2.3 Physical Addressing Algorithm

Addressing does not rely on human-assigned names. Given a target workspace path:
1. Canonicalize and resolve all symlinks: `target_canon = canonicalize(target_workspace)`.
2. Query active multiplexer sessions from driver (e.g., `herdr agent list --json` or `tmux list-panes`).
3. For each live session, compare canonical CWD:
   - Match found and session is `idle` ➔ Return `HotSession(pane_id, status: Idle)`.
   - Match found and session is `working` ➔ Return `BusySession(pane_id, status: Busy)`.
4. No match found ➔ Return `ColdSpawn(target_workspace)`.

---

## 3. Multiplexer Driver SPI

```rust
pub trait MultiplexerDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn list_sessions(&self) -> Result<Vec<LiveSession>, RouterError>;
    fn send_prompt(&self, session_id: &str, prompt: &str) -> Result<(), RouterError>;
    fn spawn_pane(&self, workspace: &Path, command: Option<&str>) -> Result<LiveSession, RouterError>;
}
```

Implementations:
- `HerdrDriver`: Interacts with Herdr daemon socket / CLI (`herdr agent list`, `herdr agent prompt` bracketed-paste).
- `TmuxDriver`: Interacts with native tmux socket / CLI (`tmux list-panes -a -F ...`).
- `AnyDriver`: Dynamic auto-detecting wrapper choosing Herdr when available, falling back to Tmux.

---

## 4. Decision Engine SPI (Phase 3 Specification)

### Distributed Topology Node Specialization
In a multi-node distributed harness (Cockpit, Compute, Storage, Edge), ambiguity exists **only at the ingress control plane**:
- **Cockpit / Ingress Plane (Developer Laptop / CI Gateway)**: Directly interfaces with humans and external alert streams. May *optionally* mount a lightweight decision model (Laya / Jev) to resolve ambiguous bug reports among multiple candidate repositories.
- **Worker / Compute / Edge Nodes (Linux GPU Boxes, Raspberry Pi, Jetson, CI Runners)**: Downstream nodes receive already-pinned, immutable `TaskEnvelope` contracts with explicit `target.workspace`. **Worker nodes strictly run in zero-model pure dispatch mode**, requiring zero Python/PyTorch/CUDA runtime dependencies and zero VRAM footprint (<15MB static memory).

### Zero-Cost Invariant & Optionality
A decision model is **never mandatory**. Over 90% of tasks have an explicit target and bypass the decision engine entirely (<1ms, $0 cost).
- **`NoopDecisionEngine` (Default)**: Pure pass-through with zero overhead.
- When an incoming task lacks an explicit target:
  - If a decision engine is configured: it evaluates candidate targets.
  - If no decision engine is configured: it safely quarantines the task to `.scratch/staged/` (HITL Outbox) for human confirmation, rather than guessing or crashing.

```rust
#[async_trait]
pub trait DecisionEngine: Send + Sync {
    /// Engine identifier name.
    fn name(&self) -> &'static str;
    
    /// Availability and health probe.
    async fn is_healthy(&self) -> bool;

    /// Evaluates a TaskEnvelope against a list of candidate targets.
    async fn evaluate_task(
        &self,
        envelope: &TaskEnvelope,
        candidates: &[(&str, &str)], // (identifier, semantic_description)
    ) -> Result<DecisionRoutingOutcome, RouterError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionRoutingOutcome {
    pub target_identifier: String,
    pub confidence: f64,
    pub engine_source: &'static str,
    pub action_gate: String, // "act" | "escalate"
    pub should_stage_outbox: bool,
}
```

### Pluggable Engines:
1. **`DeterministicRuleEngine`**:
   - Exact string matching, globs, or regular expressions against envelope fields (`caller.workspace`, `payload.title`, etc.).
   - Execution latency: ~1ms / $0 cost. Zero model dependencies.
2. **`DecisionModelEngine` (System 1 & System 2, Optional)**:
   - **Laya Client (`LayaDecisionClient`)**: Open-source non-autoregressive decision model (Convai Innovations, ModernBERT-large 421M). Single forward pass (32~50ms), zero hallucination, mathematically calibrated probabilities via RLCD. Uses `choice` primitive on `POST /v1/systemone`.
   - **TypeSafe Jev Client (`JevClient`)**: Commercial cloud fallback consuming identical `POST /v1/systemone` wire protocol.
   - **Structured LLM Client**: OpenAI-compatible chat completions fallback enforcing JSON Schema structured output for complex long-context reasoning.
3. **`CascadingDecisionEngine`**:
   - Cascading fallback: Local Laya ➔ Cloud Jev ➔ Structured LLM ➔ HITL Outbox Quarantine gate.

---

## 5. Cognitive Memory Hook SPI & Graceful Decoupling

`k0maru-agent-router` strictly upholds the **Unix Modular Decoupling Invariant**:
- The router is an independent, portable microkernel. It **MUST NOT** require `k0maru-agent-memory` to be installed or running to perform its core routing and dispatch duties.
- Memory integration is an **optional cognitive enricher**.

```rust
pub trait MemoryHook: Send + Sync {
    /// Name of the memory provider.
    fn name(&self) -> &'static str;
    
    /// Whether the memory provider is currently available on the host system.
    fn is_available(&self) -> bool;

    /// Attempts to preload architectural context or blueprint for a given workspace.
    /// Returns `Ok(Some(context))` if found, `Ok(None)` if unavailable or disabled.
    fn preload_context(&self, workspace: &Path, project_name: &str) -> Result<Option<String>, RouterError>;
}
```

### Graceful Degradation Rules:
1. **`NoopMemoryHook` (Default)**: Used when memory integration is omitted or disabled (`--no-memory`). Returns `Ok(None)` with zero overhead.
2. **`OptionalMemoryHook<H>`**: Wraps any memory hook implementation (e.g. `k0maru-memory` CLI / MCP).
   - If the `k0maru` binary is missing (`NotFound`), the MCP server is unreachable, or a query times out:
     - The hook logs a debug trace: `Memory hook skipped (provider unavailable)`.
     - **It catches all errors and returns `Ok(None)`**, never failing or aborting the routing pipeline.
   - If context is returned: the prompt injected into the target agent is transparently enriched with architectural invariants and project blueprints.

---

## 6. Execution Receipt Protocol (`agent-receipt-v1`)

To form a closed-loop execution lifecycle and verify deterministic results from downstream agents and verifiers (`k0maru-agent-verifier`), `k0maru-agent-router` ingests and validates immutable execution receipts:

```json
{
  "$schema": "https://k0maru.dev/schemas/agent-receipt-v1.json",
  "task_id": "task_demo_auth_01",
  "workspace": "/path/to/.worktrees/task_demo_auth_01",
  "verdict": "success",
  "checks": {
    "compilation": { "status": "passed", "exit_code": 0 },
    "tests": { "status": "passed", "total": 22, "failed": 0, "duration_ms": 340 },
    "linter": { "status": "passed", "warnings": 0 },
    "scope_guard": { "status": "passed", "modified_files": ["src/auth.rs", "tests/auth_test.rs"] }
  },
  "artifacts": {
    "git_patch_sha256": "8f4a2b91c7...",
    "receipt_timestamp": "2026-10-12T14:20:00Z"
  }
}
```

### Data Structures & State Machine
- `ReceiptVerdict`: `Success` | `Failed` | `Blocked`
- Checks:
  - `compilation`: status (`passed` | `failed`), exit code
  - `tests`: status (`passed` | `failed`), total, failed, duration_ms
  - `linter`: status (`passed` | `failed`), warnings count
  - `scope_guard`: status (`passed` | `failed`), list of modified files
- Receipt Verification Invariants:
  - `git_patch_sha256`: Optional cryptographic SHA-256 digest of the diff/patch for anti-tamper verification.
  - Fail-fast rejection: When `verdict == Failed` or any hard check has non-zero exit code, the task is flagged as failed.

---

## 7. Workspace Isolation Provider SPI (`WorkspaceProvider`)

To prevent dirty-write branch concurrency in active developer workspaces, the router provides an optional workspace isolation hook interfacing with `k0maru-agent-workspace`:

```rust
#[async_trait]
pub trait WorkspaceProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> bool;
    async fn provision_sandbox(
        &self,
        task_id: &str,
        repo_root: &Path,
        base_branch: Option<&str>,
    ) -> Result<PathBuf, RouterError>;
    async fn teardown_sandbox(
        &self,
        task_id: &str,
        repo_root: &Path,
    ) -> Result<(), RouterError>;
}
```

### Pluggable Providers & Graceful Degradation:
1. **`InPlaceWorkspaceProvider` (Default)**:
   - Returns the original `repo_root` untouched. Zero overhead, zero external dependencies.
2. **`CliWorkspaceProvider`**:
   - Executes `k0maru-workspace provision --task-id <id> --repo <path>` via subprocess.
   - If `k0maru-workspace` binary is absent, gracefully falls back to `InPlaceWorkspaceProvider` with a log notice.
3. **Dynamic Routing Integration**:
   - When a task envelope requests isolation (or `--isolate` flag is passed), the Coordinator provisions a worktree sandbox, redirects the target CWD to the isolated path, and dispatches the agent inside the sandbox.

---

## 8. Phase Milestones & Delivery Plan

- **Phase 1: Spec, Task Envelope & HITL Outbox Gate**: ✅ Completed (v0.1.0)
- **Phase 2: Physical CWD Addressing & Multiplexer SPI**: ✅ Completed (v0.1.0, Herdr + Tmux)
- **Phase 3: Decision SPI & Decoupled Memory Preload**: ✅ Completed (PR #2 merged, 386 tests green)
- **Phase 4: Harness Synthesis & Autonomous Loop (Active)**:
  - `agent-receipt-v1` receipt protocol & validation (`src/receipt.rs`).
  - `WorkspaceProvider` SPI & isolation adapter (`src/workspace.rs`).
  - Receipt storage, CLI commands (`k0maru-router receipt`), and MCP tools (`ingest_receipt`, `query_receipt`).
  - Coordinator closed-loop workflow: Envelope ➔ Optional Sandbox ➔ Dispatch ➔ Receipt Verification.
- **Phase 5: Distributed Federation & Heterogeneous Hardware**:
  - Remote SSH driver over Tailscale, GPU compilation servers, and edge hardware bus integration.


