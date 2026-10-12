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
- **主从智能体信号发射总线（Master-Worker Signal Bus）**：彻底破除主编排智能体“手动轮询/读取从属终端 TTY 输出”的工程瓶颈。执行 Worker 通过单次离散 CLI 信号（`k0maru-router signal`）向全局标准总线（`~/.k0maru/signals`）汇报完成状态（`completed` / `failed` / `in_progress`）、耗时与摘要，主控毫秒级响应。
- **零守护进程全局状态巡检（Zero-Daemon Status Inspector）**：无需任何常驻后台守护服务，通过多线程轻量扫描实现**全矩阵 <30ms 极速巡检**（`k0maru-router orchestrator status`），一键聚合所有仓 Git 分支、HEAD 哈希、未提交改动数、最新 Worker 信号以及最终执行收据，支持紧凑 ASCII 表格与结构化 JSON。
- **确定性执行收据闭环验证（`agent-receipt-v1`）**：联动验证器闭环任务执行事实，摄入并加密校验下游 Git Patch SHA-256 哈希、编译退出码、测试用例统计、Linter 零警告与文件作用域守卫，原生支持跳过（`skipped`）与通过（`passed`）状态校验。
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
cargo install --path .

# 二进制已安装至 ~/.cargo/bin/k0maru-router
k0maru-router --version
```

### 2. 命令行工具（CLI）常用指令

```bash
# 1. 查看路由器运行健康状态与物理寻址探查
k0maru-router status
k0maru-router probe --workspace /path/to/target-project

# 2. 路由分发指定的 TaskEnvelope 任务信封
k0maru-router route --file task.json
k0maru-router route --file task.json --dry-run
k0maru-router route --file task.json --isolate

# 3. 管理人在回路（HITL）出站暂存区中的工单草稿
k0maru-router outbox list
k0maru-router outbox inspect --id <task_id>
k0maru-router outbox approve --id <task_id>
k0maru-router outbox reject --id <task_id>

# 4. 从属智能体任务执行完成/进度信号发射（Worker Signal Ingest Bus）
k0maru-router signal --task-id task_01 --status completed --summary "修复完成"
k0maru-router signal --task-id task_02 --status failed --summary "单元测试失败"
k0maru-router signal --task-id task_03 --status in_progress --json

# 5. 主控全局状态极速巡检（<30ms 零守护进程，一键扫描全矩阵工作区）
k0maru-router orchestrator status
k0maru-router orchestrator status --json
k0maru-router orchestrator status --workspaces /path/to/ws1,/path/to/ws2

# 6. 确定性执行收据验证与查询（agent-receipt-v1）
k0maru-router receipt ingest --file /path/to/agent-receipt-v1.json
k0maru-router receipt status <task_id>
k0maru-router receipt list
```

### 3. MCP 服务接入配置

在智能体 MCP 配置文件（例如 `~/.gemini/config/mcp_config.json` 或 Claude Code 设置）中注册服务：

```json
{
  "mcpServers": {
    "k0maru-router": {
      "command": "k0maru-router",
      "args": ["mcp"]
    }
  }
}
```

支持的标准 MCP 工具集（9 大工具）：
- `route_task`: 摄入并路由 `TaskEnvelope` 结构化任务。
- `probe_workspace`: 探测指定工程路径的存活终端会话。
- `list_outbox`: 列出暂存区内所有待审及归档工单草稿。
- `approve_issue`: 批准外发指定的外部工单草稿。
- `reject_issue`: 驳回并归档指定的外部工单草稿。
- `ingest_receipt`: 摄入 `agent-receipt-v1.json` 执行收据并校验检查项与补丁 SHA-256 哈希。
- `query_receipt`: 查询指定任务 ID 的执行收据与全量检验结果。
- `emit_signal`: 向全局信号总线（`~/.k0maru/signals`）发射任务完成、失败或进行中信号。
- `orchestrator_status`: 毫秒级探测并返回跨工作区的 Git 状态、最新信号与最新收据。

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
# 运行完整自动化测试套件（128 个高信噪比测试用例，覆盖 11 个整合测试二进制目标）
cargo test

# 零警告静态检查
cargo clippy --all-targets -- -D warnings

# 代码格式化校验
cargo fmt --check
```

---

## 📄 开源许可证

MIT © K0maru
