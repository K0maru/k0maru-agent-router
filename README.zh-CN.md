# k0maru-agent-router 🧭

[English](README.md) | [简体中文](README.zh-CN.md)

面向多智能体（Multi-Agent）编程基建的跨代码库缺陷自动路由、边界防腐隔离与出站门禁系统。

`k0maru-agent-router` 是 **K0maru-Harness** 模块化矩阵中的第二块核心积木（与长期记忆系统 [`k0maru-agent-memory`](https://github.com/K0maru/k0maru-agent-memory) 深度协同）。它通过确定性的物理终端寻址、严格的边界隔离防腐策略以及人在回路（HITL）出站门禁，彻底解决多智能体跨工程协作中的割裂痛点。

---

## ⚡ 核心价值与能力

- **跨代码库缺陷零中继分流（Zero-Relay Defect Auto-Routing）**：终结开发者在不同项目之间“人肉复制报错、切换终端标签、粘贴提问”的繁琐中继。调用方智能体遭遇上游依赖缺陷时，直接打包为标准 `agent-task-v1` 任务信封并自动分流至目标项目。
- **架构防腐与权限隔离（Architecture Anti-Corruption）**：业务智能体绝不越权直接修改依赖框架或上游仓库代码，缺陷必须通过不可变契约解耦下发。
- **第三方开源出站隔离门禁（HITL Outbox Gate）**：针对第三方外部开源依赖（如 GitHub 开源库），系统严禁自动化静默外发，强制拦截并在 `.scratch/staged/` 生成标准化最小复现（MRE）草稿，必须由人类技术负责人审查批准后方可外推。
- **零认知负荷的非命名物理寻址（Physical CWD Addressing）**：完全基于规范化路径（CWD / Git root）在 **Herdr** 或 **Tmux** 中确定性检索存活会话与窗格，无需人类维护任何窗格名称或会话标签。
- **双模暴露（CLI & MCP）**：既可作为高性能原生命令行工具（`k0maru-router`）独立运行，也可作为标准 Model Context Protocol (MCP) 服务器无缝接入 Antigravity、Claude Code、Cursor 等主流 AI 编码环境。

---

## 🏗️ 架构拓扑

![k0maru-agent-router 架构拓扑](assets/architecture_topology.png)

---

## 📊 决策引擎实测基准与性能评测报告

`k0maru-agent-router` 集成了非自回归 System-1 决策引擎（[Laya](https://github.com/NandhaKishorM/laya) / TypeSafe Jev ModernBERT-large）、零开销确定性规则引擎与物理终端直接寻址。

在 macOS Apple Silicon (MPS) 实机环境下的全链路实测数据如下：

![k0maru-agent-router 决策引擎性能评测基准](assets/benchmark_decision_performance.png)

| 决策层级 (Tier) | 运行时机制 | 平均判定延迟 (ms) | 相对 LLM 加速比 | 内存 / 显存占用 | 适用拓扑角色 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Tier 0: Direct CWD** | 终端复用器路径匹配 (Herdr / Tmux) | **0.85 ms** | **2,176x** | **11.2 MB** | 执行节点 / 计算节点 (<15MB 铁律) |
| **Tier 1: Rule Engine** | 正则 / 精确 / 前缀规则匹配 | **1.10 ms** | **1,681x** | **14.8 MB** | 控制入口 / 入门级节点 |
| **Tier 2: Laya MPS** | ModernBERT-large (Apple Silicon MPS) | **18.40 ms** | **100.5x** | **1.12 GB** (驻留显存) | 调度控制面 (可选装配) |
| **Tier 2: Laya CPU** | ModernBERT-large (8线程 SIMD) | **42.10 ms** | **43.9x** | **1.15 GB** (内存) | 无 GPU 的无头服务器 |
| **Tier 3: Autoregressive** | JSON-Schema 结构化大模型 API | **1,850.00 ms** | 1.0x (基准) | **8.30+ GB** (本地) | 云端回退兜底 |

- **100 倍延迟优势**：非自回归 ModernBERT 单次前向推理仅需 **18.4 ms** 完成多候选判决，彻底终结自回归大模型动辄 1.8 秒以上的流式生成卡顿。
- **物理权责彻底解耦**：执行节点（Worker）严格遵循 **<15MB 零模型不变量**（实测仅 11.2 MB）；模型仅作为控制面（Cockpit）可选组件，严禁污染执行端。
- **校准置信度与 85% HITL 门禁**：高置信度任务（≥85%）全自动分发执行；模糊或歧义任务（<85%）坚决不盲发，自动拦截并落盘至出站暂存区（`.scratch/staged/`）等待人类签发。

*(完整实验设计与评测报告详见 [docs/BENCHMARK_REPORT.md](docs/BENCHMARK_REPORT.md)，竞品多维对比与核心优势分析详见 [docs/COMPETITIVE_ANALYSIS.md](docs/COMPETITIVE_ANALYSIS.md)。)*

---

## 🚀 快速上手

### 1. 编译与构建

```bash
# 环境要求: Rust 1.75+ (Cargo)
git clone https://github.com/K0maru/k0maru-agent-router.git
cd k0maru-agent-router
cargo build --release

# 编译产物位于:
./target/release/k0maru-router
```

### 2. 命令行工具（CLI）常用指令

```bash
# 查看路由器运行健康状态、驱动可用性与出站指标
k0maru-router status

# 根据目标工程路径物理探测复用器会话
k0maru-router probe --workspace /path/to/target-project

# 路由分发指定的 TaskEnvelope JSON 任务信封
k0maru-router route --file task.json

# 管理人在回路（HITL）出站暂存区中的工单草稿
k0maru-router outbox list
k0maru-router outbox inspect --id <task_id>
k0maru-router outbox approve --id <task_id>
k0maru-router outbox reject --id <task_id>
```

### 3. MCP 服务接入配置

在智能体 MCP 配置文件（例如 `~/.gemini/config/mcp_config.json` 或 Claude Code 设置）中注册服务：

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

支持的标准 MCP 工具集：
- `route_task`: 摄入并路由 `TaskEnvelope` 结构化任务。
- `probe_workspace`: 探测指定工程路径的存活终端会话。
- `list_outbox`: 列出暂存区内所有待审及归档工单草稿。
- `approve_issue`: 批准外发指定的外部工单草稿。
- `reject_issue`: 驳回并归档指定的外部工单草稿。

---

## 📜 任务信封协议规范 (`agent-task-v1`)

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
    "title": "修复 JWT 令牌过期校验穿透缺陷",
    "severity": "high",
    "reproduction": {
      "snippet": "let res = auth::verify_jwt(\"expired_token_123\");\nassert_eq!(res, Err(ExpiredToken));",
      "error": "Assertion failed: left: Ok(true), right: Err(ExpiredToken)"
    },
    "expected": "verify_jwt 必须拦截过期令牌并返回 Err(AuthError::ExpiredToken)。"
  }
}
```

---

## 🧪 质量与测试基线

```bash
# 运行完整自动化测试套件（覆盖协议模型、Herdr/Tmux 驱动、暂存门禁与 MCP stdio）
cargo test

# 零警告静态检查
cargo clippy --all-targets -- -D warnings

# 代码格式化校验
cargo fmt --check
```

---

## 📄 开源许可证

MIT © K0maru
