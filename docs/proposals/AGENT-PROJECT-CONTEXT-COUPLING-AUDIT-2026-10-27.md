# WEXPC Agent / Project / Context 固定绑定代码审计（2026-10-27）

**状态：源码静态审计；非批准改造方案。** 以 main 已更新的 `docs/PC-PRODUCT-AND-TECH-BASELINE.md` 第 0 节为顶层语义依据。只分析现有耦合，不改代码、不重排 0.1.1 施工。

## 1. 证据定位（main）

| 文件/位置 | 源码事实 | 与新顶层语义的关系 |
|---|---|---|
| `crates/wex-desktop-service/src/domain/mod.rs:73-94` | `NewAgent.project_id: String` 必填；`Agent` 持有必填 `project_id`、固定 `AgentRole`、`connection_id`、`model_id`、`conversation_id` | 独立角色只能通过旧项目化实体绕行，职责与模型/会话的关系不够灵活 |
| `application/mod.rs:215-252` | `add_agent()` 先要求项目存在，自动创建 `Conversation{project_id:Some,context_id:None}`，再创建 Agent 和对应项目 Duty | Agent 创建被强绑定项目 + 项目专属会话；角色独立入口没有同等入口 |
| `application/mod.rs:98-109` | `task_member_assigned()` 接受项目 Duty 或旧 `Agent.project_id` 作为成员资格 | 历史兼容回退仍参与任务成员判断；不应在未来混同实际工具授权 |
| `application/mod.rs:258-286` | `assign_agent_duty(agent_id,project_id:Option,duty_id)` 允许多 Duty、非项目 Duty | 是重要扩展基础；但缺少 Context/其他范围的精确定义 |
| `application/mod.rs:294-358` | `NewTask`/Task 带必填项目及成员清单，创建/修改时验证项目归属 | **仅项目型 Task** 的约束未必错误；错误是将它推广为所有工作/协作任务的唯一入口 |
| `application/mod.rs:387-418` | `prepare_chat` 根据 Conversation.context_id 加入上下文文本；根据 Agent.conversation_id 查找 Agent，再用 Agent 固定 role 生成职责提示 | Context 和 Agent 的动态工作绑定尚未形成；提示词不是授权来源 |
| `application/organization.rs:142-169` | `create_context` 要求 space_id，创建单个 `ContextPlacement`；`Context.resources:Vec<String>` | 可保留 Space 主要展示位置，但跨 Context/跨项目引用不可仅靠地址列表模拟 |
| `application/organization.rs:195-210` | `bind_project_context` 为 Project 赋予一个 `context_id` | 可以表示项目主归类，不足以作为项目被多个 Context 研究/引用的关系总模型 |
| `apps/desktop/src/types.ts:1-16` | 前端 `Agent`、`AgentRole`、`AgentDutyAssignment`、`Context` 和 `Project` 逐一复制上述 DTO | 任何后续模型演进都需要兼容 DTO 与 UI 的过渡；不可单侧重命名 |
| `core/dispatch.rs` | DispatchTicket 是项目 Task 级的 agent/epoch 检查；Host 负责额外权限检查 | 不必把所有 Context/独立角色交互硬塞入该 Task API；仍须保持真实授权边界 |

## 2. 核心判断

1. **高优先级：Agent 创建语义不符。** 当前 Agent 事实上首先是“某项目的成员”，而确认的新产品语义允许独立 Agent/角色、再按需参与多个 Context 与项目。不能以 `project_id` 必填作为长期领域不变量。
2. **高优先级：岗位在执行入口缺乏工作范围身份。** Duty 已经多对一登记，但 `prepare_chat` 仍使用 Agent 单个 `role` 提示；执行入口尚未选择/记录本次 duty + scope。不能靠 prompt 模拟授权。
3. **高优先级：旧成员资格回退需要明确边界。** `task_member_assigned` 中 `Agent.project_id` 可替代显式 Duty。这个回退可为旧记录兼容服务，但不应永久充当未来授权凭证。现有它只影响任务关联校验，**本审计未证明已能绕过 Host 文件或工具授权**。
4. **中优先级：Context 关联不是 Context 的父子/包含证明。** Space 单个主要位置和 Project 单个主 Context 可继续用于呈现或归类；需要独立研究多个 Context 如何指向同一项目/其他 Context。不能直接把 `resources:Vec<String>` 升级成 ACL。
5. **可保留：普通 Conversation 能独立于项目存在。** 不应因为任务/角色扩展而将所有对话强制包装成项目 Task；Core Dispatcher 的项目任务 ticket 并非所有操作的上位身份。

## 3. 与产品基线比对

- Context 是可与多个项目/Context 关联的工作环境，不等同 Project。
- 独立角色授权与某 Context 中工作的角色授权分离；跨入口不得自动沿用项目写入能力。
- Agent 身份、岗位职责、模型连接及会话各自可以变化/解绑；具体迁移方式尚未决议。
- 角色拥有工具或某个 Context 引用工具不自动获得真实目标资源权限。
- 本轮不改变第三支柱未来方向、主模型本地化研究阶段、0.1.1 既定交付范围。

## 4. 建议的审查及回归顺序（不是当前批准的代码任务）

A. 首先以语义/兼容测试覆盖：“独立角色存在但不绑定项目”“一个 Agent 在 Context A 和 B 承担不同职责”“外部入口不沿用项目内权限”“普通对话不需要项目 Task”。

B. 在提出具体迁移前，逐个评估创建 Agent、自动会话、职责默认分配、Task 成员兼容、DTO/序列化、现有持久记录的影响。不得贸然删必填字段令旧工作状态丢失。

C. 与 W2/W3 主线对齐：继续采用唯一业务任务状态及 Host 授权实际执行；不可建设新 Dispatcher、虚构外部 Agent 可控性或把 Agent 对话提示词当作正式授权。

D. 继续开放讨论：Context 关联的资源种类、工具来源、Context 内操作和跨入口授权**尚未冻结**，不由本审计决定权限表和能力目录实现。

## 5. 本批验证限制

检查的是 GitHub main 中以上列出的 Rust/TS 源码片段与现行基线，未运行 Cargo、Windows 实机或端到端工具调用测试。此文记录“哪些耦合存在、会影响什么”，不宣称已证明操作越权或已经完成修复。

审查分支 `docs/connection-contract-review-20261026` 与 main 存在分叉。后续合并须与 main 新增顶层三支柱语义对齐，避免旧分支结论覆盖它。
