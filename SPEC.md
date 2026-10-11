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
- `HerdrDriver`: Interacts with Herdr daemon socket / CLI (`herdr agent list`, `herdr agent prompt`).
- `TmuxDriver`: Interacts with native tmux socket / CLI (`tmux list-panes -a -F ...`).
- `SubprocessDriver`: Spawns standalone CLI agent in headless environments or CI.

---

## 4. Phase Milestones & Delivery Plan

- **Phase 1: Spec, Task Envelope & HITL Outbox Gate (Current)**:
  - TaskEnvelope JSON validation & serialization.
  - Boundary policy engine (Trusted vs Untrusted).
  - Staged draft generator & human approval gate CLI (`stage`, `list-staged`, `approve`).
  - Scaffolding & unit/integration test suite.
- **Phase 2: Physical CWD Addressing & Multiplexer SPI**:
  - CWD canonical normalization & live session matching.
  - Herdr & Tmux driver implementations.
  - Hot session wake up vs Cold spawn logic.
- **Phase 3: Decision SPI & Memory Preload**:
  - DeterministicRuleEngine, SemanticVectorEngine (ONNX).
  - Hook to invoke `k0maru-agent-memory` for project blueprint preload.
- **Phase 4: Harness Synthesis & Autonomous Loop**:
  - End-to-end integration with K0maru-Harness ecosystem.
