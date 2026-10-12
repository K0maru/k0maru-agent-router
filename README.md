# k0maru-agent-router 🧭

[English](README.md) | [简体中文](README.zh-CN.md)

Autonomous cross-repository task routing, defect dispatch, and HITL quarantine for multi-agent coding harnesses.

`k0maru-agent-router` serves as the second core pillar of the **K0maru-Harness** modular matrix (alongside [`k0maru-agent-memory`](https://github.com/K0maru/k0maru-agent-memory)). It bridges the gap between isolated workspace agents and upstream dependencies through deterministic physical multiplexer addressing, strict boundary isolation, and human-in-the-loop outbox verification.

---

## ⚡ Core Capabilities

- **Zero-Relay Defect Auto-Routing**: Eliminates manual copy-paste clipboard friction. When a consumer workspace encounters an upstream defect, it packages the issue as a standardized `agent-task-v1` envelope and routes it directly to the target project.
- **Architectural Anti-Corruption**: Subagents are strictly restricted from mutating external repositories or dependencies directly within consumer codebases.
- **HITL Outbox Gate**: Third-party external open-source dependencies (e.g. GitHub OSS) are quarantined into reproducible Minimum Reproducible Example (MRE) drafts under `.scratch/staged/` requiring explicit human sign-off before any outbound dispatch.
- **Physical Zero-Cognitive-Load Addressing**: Discovers and addresses live agent panes strictly via canonical directory path matching (CWD / Git root) in **Herdr** and **Tmux** without requiring human-managed pane names or IDs.
- **Dual Exposure (CLI & MCP)**: Operates both as a high-performance native CLI tool (`k0maru-router`) and as a Model Context Protocol (MCP) server for Antigravity, Claude Code, and other AI agents.

---

## 🏗️ Architecture

![k0maru-agent-router Architecture Topology](assets/architecture_topology.png)

---

## 📊 Empirical Performance & Decision Engine Benchmark

`k0maru-agent-router` integrates a Non-Autoregressive System 1 ModernBERT decision engine ([Laya](https://github.com/NandhaKishorM/laya) / TypeSafe Jev) alongside zero-overhead deterministic rules and physical multiplexer addressing.

Empirically verified on macOS Apple Silicon (MPS):

![k0maru-agent-router Decision Performance Benchmark](assets/benchmark_decision_performance.png)

| Decision Tier | Runtime Mechanism | Mean Latency (ms) | Speedup vs LLM | Memory / VRAM Footprint | Architectural Role |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Tier 0: Direct CWD** | Multiplexer Path Match (Herdr / Tmux) | **0.85 ms** | **2,176x** | **11.2 MB** | Worker & Compute Nodes (<15MB invariant) |
| **Tier 1: Rule Engine** | Regex / Exact / Prefix Matching | **1.10 ms** | **1,681x** | **14.8 MB** | Cockpit & Entry Nodes |
| **Tier 2: Laya MPS** | ModernBERT-large (Apple Silicon MPS) | **18.40 ms** | **100.5x** | **1.12 GB** (resident) | Cockpit Control Plane (Optional) |
| **Tier 2: Laya CPU** | ModernBERT-large (8-Thread SIMD) | **42.10 ms** | **43.9x** | **1.15 GB** (RAM) | Non-GPU Headless Server |
| **Tier 3: Autoregressive** | JSON-Schema Structured LLM API | **1,850.00 ms** | 1.0x (baseline) | **8.30+ GB** (local) | Cloud Fallback |

- **100x Latency Advantage**: Non-autoregressive ModernBERT single forward pass evaluates candidates in **18.4 ms** compared to 1,850 ms for autoregressive LLMs.
- **Strict Role Decoupling**: Worker nodes operate under an immutable **<15 MB Zero-Model Invariant**; heavy models are strictly optional and isolated to the Cockpit control plane.
- **Calibrated 85% Confidence Gate**: High-affinity defects (≥85%) dispatch autonomously; ambiguous chores (<85%) are safely quarantined into the HITL Outbox for human review.

*(For detailed methodology, boundary analysis, and reproduction steps, see the full [Empirical Benchmark Report](docs/BENCHMARK_REPORT.md).)*

---

## 🚀 Quick Start

### 1. Build and Install

```bash
# Prerequisites: Rust 1.75+ (Cargo)
git clone https://github.com/K0maru/k0maru-agent-router.git
cd k0maru-agent-router
cargo build --release

# The compiled binary will be located at:
./target/release/k0maru-router
```

### 2. CLI Usage

```bash
# Check system status, driver availability, and outbox metrics
k0maru-router status

# Probe active multiplexer sessions for a workspace directory
k0maru-router probe --workspace /path/to/target-project

# Route a TaskEnvelope JSON file
k0maru-router route --file task.json

# Manage staged external issue drafts
k0maru-router outbox list
k0maru-router outbox inspect --id <task_id>
k0maru-router outbox approve --id <task_id>
k0maru-router outbox reject --id <task_id>
```

### 3. MCP Server Configuration

Register `k0maru-router` in your agent MCP settings (e.g., `~/.gemini/config/mcp_config.json` or Claude Code MCP configuration):

```json
{
  "mcpServers": {
    "k0maru-router": {
      "command": "/path/to/k0maru-agent-router/target/release/k0maru-router",
      "args": ["mcp"]
    }
  }
}
```

Available MCP Tools:
- `route_task`: Ingests and routes a `TaskEnvelope` payload.
- `probe_workspace`: Probes active sessions for a given workspace path.
- `list_outbox`: Lists all staged external drafts in the quarantine area.
- `approve_issue`: Approves an external staged issue for dispatch.
- `reject_issue`: Rejects and archives an external staged issue.

---

## 📜 Task Envelope Protocol (`agent-task-v1`)

```json
{
  "$schema": "https://k0maru.dev/schemas/agent-task-v1.json",
  "id": "task_auth_jwt_01",
  "created_at": "2026-10-12T00:00:00Z",
  "caller": {
    "workspace": "/Users/k0maru3/workspace/consumer-app",
    "agent": "consumer-worker-1",
    "pane_id": "wB:p1"
  },
  "target": {
    "identifier": "internal-auth-lib",
    "workspace": "/Users/k0maru3/workspace/auth-lib",
    "is_external": false
  },
  "intent": "defect_resolution",
  "payload": {
    "title": "Fix JWT token expiration validation flaw",
    "severity": "high",
    "reproduction": {
      "snippet": "let res = auth::verify_jwt(\"expired_token_123\");\nassert_eq!(res, Err(ExpiredToken));",
      "error": "Assertion failed: left: Ok(true), right: Err(ExpiredToken)"
    },
    "expected": "verify_jwt must reject expired tokens and return Err(AuthError::ExpiredToken)."
  }
}
```

---

## 🧪 Testing & Verification

```bash
# Run comprehensive test suite (350+ tests covering protocol, drivers, outbox, and MCP)
cargo test

# Zero-warning lint check
cargo clippy --all-targets -- -D warnings

# Formatting check
cargo fmt --check
```

---

## 📄 License

MIT © K0maru
