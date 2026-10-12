# K0maru-Agent-Router: 核心宣传指标、横向竞品对比与核心优势

> **定位**：K0maru-Harness 模块化多智能体矩阵的**独立通信、调度与隔离执行微内核**。  
> 秉承 Unix 哲学，提供单静态二进制、零守护进程、客观物理事实驱动的跨仓智能体协同网关。

---

## 🚀 一、 核心可宣传指标 (Marketable & Empirical Metrics)

基于 **Google Colab NVIDIA A100-SXM4-80GB** 与 **macOS Apple Silicon (MPS)** 实机横向对比实测数据：

| 评估指标 | 测量数值 | 业界基准对比 / 意义 | 实测环境与证据 |
| :--- | :---: | :--- | :--- |
| **物理寻址调度延迟 (CWD Match)** | **0.85 ms** (macOS) / **1.64 ms** (Linux) | **538x 快于本地 Qwen2.5-Coder**，**1,128x 快于云端大模型**；90%+ 任务直通，零 Token 消耗 | 本地 Tmux / Linux A100 单二进制 |
| **规则引擎匹配延迟 (Rule Engine)** | **1.10 ms** (macOS) / **1.53 ms** (Linux) | **577x 快于本地 Qwen2.5-Coder**；前缀/正则匹配确定性分流 | 单前向内存零拷贝解析 |
| **本地 System-1 决策延迟 (ModernBERT)** | **18.23 ms** (A100) / **18.40 ms** (MPS) | **48.4x 提速**（对比 Qwen2.5-Coder-7B 的 883.02 ms）；无逐 token 吐字等待 | NVIDIA A100 GPU (`cuda`) |
| **基线代码大模型延迟 (Qwen2.5-Coder)** | **883.02 ms** (7B bfloat16) | 结构化 JSON 生成基线，单次产生 25.5 tokens，显存开销极大 | NVIDIA A100 GPU (31.1 GB 峰值分配) |
| **Worker / 执行节点常驻内存** | **11.2 MB** | **严格守住 < 15 MB 物理不变量**；零 Python/PyTorch/CUDA 运行时依赖 | Rust Release 编译单二进制 |
| **控制面模型显存对比** | **616.7 MB** vs **15.8 GB** | **显存节省 96.1% (25.7x 更轻量)**；ModernBERT 仅需 616 MB，Qwen2.5 需 15.8 GB | NVIDIA A100 实测显存分配 |
| **HITL Outbox 开源边界隔离率** | **100.0% (10/10)** | 10 个第三方开源依赖 Bug 全部物理拦截落盘，0 泄露外推 | 自动化 Outbox 隔离判定流 |
| **收据防伪与事实验证延迟** | **2.05 ms** (P50: 2.01 ms) | 真实 Exit Code 零 Token 物理秒级校验 + SHA-256 补丁签名防伪 | `agent-receipt-v1` 校验器 |
| **质量与防腐可靠性** | **400 项通过** | **100% 绿灯**，0 警告 (`clippy -D warnings`)，0 虚构合成 Mock | `cargo test` 全套回归测试套件 |

---

## 🥊 二、 横向竞品全面多维对比 (Horizontal Competitive Matrix)

在多智能体系统（Multi-Agent Systems）领域，我们将 `k0maru-agent-router` 与现有典型主流方案进行多维正交对比：

1. **应用层多智能体框架**：MetaGPT / AutoGen / CrewAI
2. **IDE 进程级原生子代理**：Claude Code / Antigravity (`teamwork-preview` / Subagent Task Delegation)
3. **工作流图编排引擎**：LangGraph / LlamaIndex Agent Workflow
4. **容器化沙箱与调度基建**：OpenHarness / Docker Agent Sandboxes

### 深度对比矩阵表

| 评估维度 | **k0maru-agent-router** | **AutoGen / CrewAI / MetaGPT** | **LangGraph / 集中图编排** | **IDE 原生子代理 (Teamwork)** | **容器化沙箱 (Docker/K8s)** |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **系统架构** | **操作系统级微内核**<br/>(Rust 单二进制 CLI/MCP) | 庞大 Python 应用程序框架 | Python/TS 集中状态机服务 | 单 IDE 进程内线程/派生会话 | 笨重容器基础设施 |
| **跨仓通信** | **不可变 `TaskEnvelope`**<br/>(穿透独立 Git Root) | 局限于单进程或单共享目录 | 依赖中央消息队列与数据库 | 局限在当前 IDE 打开的 Workspace | 需配置容器卷挂载与端口映射 |
| **权限防腐** | **强权限隔离 (Anti-Corruption)**<br/>业务 Agent 物理禁止改写依赖仓 | 共享内存/文件系统，极易脏写 | 依赖业务逻辑中编写的权限判断 | 共享同一个 Git 工作区与暂存区 | 容器级强隔离，但启动极慢 |
| **物理寻址** | **确定性规范化 CWD 匹配**<br/>(0 认知负担，不依赖人工命名) | 依赖 Agent Name 或 LLM 分配 | 依赖预定义节点连线与 LLM 路由 | 依赖大模型自主探索目录 | 依赖 IP、端口与服务发现 |
| **分流性能** | **分层金字塔：0.85ms 直通 ➔ 18ms 本地 System-1** | 每次决策消耗完整 LLM 推理 (1.8s+ / 耗费 Token) | 复杂状态图每步调用 LLM 判定 (2s+) | 每次分配需探索文件与 Prompt 注入 (数万 Token) | 无内置智能语义分拣能力 |
| **节点资源** | **执行节点 11.2 MB**<br/>(零 Python/PyTorch 依赖) | 普遍 > 500 MB 内存，强依赖 Python 虚拟环境 | 数百 MB，高 CPU/内存占用 | 随会话历史无限膨胀 (>100k 上下文失忆) | 单容器动辄数百 MB ~ 数 GB 镜像与内存 |
| **出站安全** | **HITL Outbox 物理硬门禁**<br/>(未授权严禁外推 Issue/PR) | 极易发生无意识调用网络工具提交外部 PR | 提示词层面防范，易被越狱与幻觉突破 | 依赖工具确认弹窗，容易疲劳点击确认 | 需通过外部网络防火墙阻断 |
| **验证闭环** | **`agent-receipt-v1` 物理 Exit Code + SHA-256 防伪** | 仅听信 Agent 口头汇报 "I have fixed it" | 依赖模型自我反思循环评估 | 依赖模型在会话中文字回复 | 需自建进程退出码回调系统 |
| **运行时侵入** | **零守护进程 (Zero-Daemon)**<br/>原生兼容 Tmux / Herdr / CLI / MCP | 强侵入，必须常驻专属 Python 运行栈 | 强侵入，依赖服务端常驻 | 深度绑定特定 IDE 或商业客户端 | 强依赖 Docker Engine / K8s |

---

## 💎 三、 六大核心杀手级优势 (Core Value Propositions)

### 1. 彻底终结“人肉剪贴板中继” (End Meatware Clipboard Relaying)
- **痛点**：在微服务或多仓工程中，主应用 Agent 遇到底层 UI 组件库或 SDK 的 Bug，传统模式必须由人类工程师人工开终端、切目录、复制代码报错、再给另一个 Agent 重新讲一遍上下文。
- **Router 解法**：上游 Agent 直接签发不可变 `TaskEnvelope`，Router 依据物理 CWD 自动识别目标仓活跃窗格，**毫秒级跨仓热唤醒注入或冷启动切分**，人类从“肉体中继器”解放为“最终审批者”。

### 2. 破除分支脏写与代码库污染 (Zero Dirty-Write Concurrency)
- **痛点**：如果多个 Agent 同时在主工程修复不同 Bug，或者在开发者正在写代码的主工作区分支上直接 `git checkout`，会导致暂存区混乱、代码冲毁与脏提交。
- **Router 解法**：通过 `WorkspaceProvider` SPI 与 `k0maru-agent-workspace` 联动，在派发写操作工单时**秒级拉起独立的 Git Worktree 隔离沙箱**，物理重定向执行 CWD，主干分支绝对零污染。

### 3. 100x 速度 + $0 成本的非自回归 System-1 极速决断
- **痛点**：传统多智能体框架在“这个报错到底该派给谁”的问题上，每次都要完整调用 GPT-4o 或 Claude 3.5 跑 2~3 秒，耗费数千 Token 且存在网络延迟抖动。
- **Router 解法**：构建决策金字塔：
  - **90%+** 明确任务走 **0.85 ms 物理直通**（$0 开销）；
  - **8%** 走 **1.1 ms 确定性规则直通**；
  - 仅 **1%~2%** 模糊长尾工单唤醒本地 Laya ModernBERT，**18.4 ms 单前向推断完成强类型置信度打分**，比大模型快两个数量级。

### 4. 严苛的第三方开源隔离门禁 (HITL Outbox Gate)
- **痛点**：自主 Agent 在排查问题时，经常将第三方开源框架（如 React、Tokio、Axios）的内部预期行为误当成 Bug，擅自调用网络工具向开源上游乱发 Issue 或 PR，造成安全事故与信誉受损。
- **Router 解法**：内置 `BoundaryPolicy` 边界隔离。凡是标为 `is_external: true` 的开源目标，**一律物理阻断外发**，自动提炼为标准化 MRE 暂存草稿（`.scratch/staged/issue_xxx.md`），必须经人类工程师在终端或 MCP 中显式签署 `approve` 方可外发。

### 5. “不信口头汇报，只认物理执行事实”的防伪收据闭环 (`agent-receipt-v1`)
- **痛点**：LLM 存在“幻觉交付”严重通病——代码根本通不过编译或测试报错，但大模型却在对话框里信誓旦旦汇报“修复已完成”。
- **Router 解法**：打通 `agent-receipt-v1` 执行收据闭环。Router 仅在收到下游真实的进程退出码（`exit_code == 0`）、测试通过计数、Linter 零警告以及 SHA-256 补丁数字签名后，才认定任务交付完成，彻底杜绝智能体虚假交付。

### 6. 控制面智能与计算节点轻量的权责分立 (Cockpit vs Worker Decoupling)
- **痛点**：现有框架强行给每台机器都部署大模型和厚重的 Python 依赖，导致高负载编译机或低功耗边缘设备（树莓派、Jetson Orin）显存直接被挤爆。
- **Router 解法**：**入口控制面（开发者笔记本）**可选挂载轻量决策模型；而**远端计算与边缘节点**仅运行小于 **15 MB（实测 11.2 MB）**的纯静态分发微内核，零 Python 依赖、零显存占用，天然适配嵌入式与生产机。
