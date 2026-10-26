# WEXPC Agent 工作身份与运行范围审计（第三批，2026-10-27）

## 0. 根因更正：将“软件开发工作模式模板”误升格为通用 Context 架构（用户 2026-10-27 澄清）

**本节为产品语义更正，优先于本审计关于固定流程的简化表述。** 旧实现以“创建项目 → 创建项目成员 Agent → 分配项目 Task”为默认路径，其根源不只是工程方便，而是将**软件开发工作模式**中围绕 Space 的管理方式、Context 的组织结构及其**类型／模板**，错误地当成 WEX 所有 Context 的通用结构。结果是某一种工作模式的项目、成员、岗位、会话、任务关系被写进通用对象创建和运行入口。

需要分开：
- **Space / 可选 Group / Context**：通用的组织、关联与工作环境结构。Space 管理类型、模板、类别与呈现；Context 可独立关联多个项目、Context、角色及资源，但不能因此获得实际资源权限。
- **软件开发模式（Context 类型／模板及其工作组织规则）**：一种具体应用方式，通常需要 Project、项目成员、Lead/Worker/Reviewer/Verifier、项目 Task、任务证据和开发流程。它可以提供便捷的默认组合，但不是通用 Context 的本体结构，也不能成为全部 Agent、Conversation、Context 的创建前置条件。
- **自由组合规则**：独立对象和能力按当前场景建立明确、可变、可解除的工作关系；不是取消软件开发场景自身必要的项目约束、任务分工和执行验收。

因此，审计不应得出“项目流程应该被废弃”的结论；应判断**项目流程被错误地提升为系统级必经流程**，并检查所有将模板默认行为写成通用领域不变量的代码和文档。尤其要区分模板初始化时的默认绑定、可随时调整的工作关系，以及执行时才判定的真实授权。

**审查约束：** 不引入强制新层级，不把“管理”凭空冻结为独立数据库对象；不将全部 Context 强制采用软件开发模板；不立即删掉旧 `project_id` 或重建 Dispatcher。先审计模板、类型、实例与组织位置的语义和入口，再讨论兼容迁移。以上是架构根因纠偏，不表示代码已经修复。


**状态：代码静态审计／待裁决建议；不代表已批准施工。** 接续 `AGENT-PROJECT-CONTEXT-COUPLING-AUDIT-2026-10-27.md`。核对 main 的 `domain/mod.rs`、`domain/agent_assignment.rs`、`application/mod.rs`、`application/member_brief.rs`、`application/organization.rs` 与 `PC-PRODUCT-AND-TECH-BASELINE.md` §0、§2。

## 1. 新增证据

| 代码位置 | 已核对事实 | 推论边界 |
|---|---|---|
| `application/member_brief.rs::build` | 只用 `Agent.project_id` 定位项目；无调用方指定的本次项目/Context 参数 | 一个 Agent 另获项目 Duty，并不使该工作简报切到新项目 |
| 同函数 | `lead = member.role == AgentRole::Lead`；Lead 可查看该项目全部任务/成员，其他角色仅返回本人任务/成员 | 固定 role 决定提示资料视图，而非当前所选 Duty；这不是 Host 工具授权证明 |
| `application/mod.rs::prepare_chat` | 通过 `Agent.conversation_id` 找 Agent；根据固定 role 拼职责提示并调用 `member_brief::build` | 本轮没有显式 Duty/Scope 选择或校验；岗位登记尚未落实到运行时 |
| `domain/agent_assignment.rs` | `AgentDutyAssignment {agent_id, project_id: Option<String>, duty_id}`；None 表示用户级 Duty | 既有结构不能直接表达 Context 级职责；不宜把 None 偷用成任意范围 |
| `domain/mod.rs` | `ChatTurn` 保存会话、消息、状态，不保存执行使用的 Duty/Scope 或配置快照；`Agent` 保存固定 role、模型、会话 | 历史对话无法仅由当前可变职责登记可靠解释本轮身份；是否需要独立工作调用记录待设计 |
| `application/mod.rs::task_member_assigned` | 项目 Duty 与旧 `Agent.project_id` 是 OR 关系 | 如撤销 Duty 而旧 project_id 仍匹配，任务成员校验依然可通过；只证明成员校验结果，不证明工具越权 |

## 2. 具体风险场景

- Agent 最初在项目 A 创建，再指派项目 B 的 reviewer Duty：现有成员简报仍固定从项目 A 取资料。
- Agent 固定 role 为 Lead，实际在另一项目仅有 reviewer Duty：简报仍以旧 Lead 身份判断是否展示完整项目任务；当前构造通常取原项目，不能以此声称已越权读取另一个项目。
- 删除或调整 Agent 的显式项目 Duty：只要原 `Agent.project_id` 仍相符，`task_member_assigned` 的旧路径可能继续返回真。
- 通过项目外入口与 Agent 交互：当前对话运行路径不能证明本次明确选择了独立入口职责、工作范围、以及该入口实际许可；不能用 role 提示代替授权。

## 3. 语义裁决建议（非字段冻结）

1. **稳定身份与临时工作绑定分开。** Agent ID 是身份；本次会话/任务调用需要独立解析工作范围及 Duty。项目成员是关系，不是 Agent 的本体身份。
2. **工作资料须基于本次范围生成。** 对项目 A/B 或 Context 入口分别校验所选职责，再构造简报；不得从 Agent 原始 project_id 猜测本次工作目标。主 AI 的广域工作视图也只在当前匹配范围生效。
3. **授权由既有 Host/连接器机制求值。** Duty 决定职责和资料选择，不自行发放工具/文件许可；资料装配也应避免在越界入口向外部模型提交非本次范围的数据。
4. **记录工作调用身份快照。** 后续设计应使审计可回答：谁（Agent）、以何职责、在何作用范围、通过哪条会话/连接、依据何时的许可发起工作；不冻结具体表名或授权 token 算法。
5. **旧项目归属回退有明确迁移终点。** 先识别和迁移旧快照中的项目成员职责，验证无损，再缩窄/取消旧字段参与成员判定；撤销显式 Duty 不应被旧回退静默抵消。
6. **普通 Conversation 保持独立。** 不把每次角色交互、Context 研究或普通聊天强制转为项目 Task；既有项目型任务结构仍可保留。

## 4. 对 0.1.1 的施工边界

**本版必须修正的契约缺口**：项目协作中的本次 Duty/项目选择、对应简报构造、旧成员资格回退的安全迁移、以及可核对的执行身份与范围记录；否则无法真实验收多 Agent 多岗位的项目协作。

**不应借机扩大首版范围**：通用 Context→工具 ACL、全域关系图、跨项目自治 Dispatcher、任意外部 Agent 内核接管、全新权限引擎、未来策略积累体系，均不得因该审计直接升级为 0.1.1 新施工要求。

## 5. 建议回归用例（尚未执行）

1. Agent A：项目甲 lead、项目乙 reviewer，在乙启动工作时仅取得乙的适当工作资料和 reviewer 提示。
2. 移除项目甲显式 Duty 后，旧 `Agent.project_id=甲` 不会在兼容期结束后重新赋予任务成员资格。
3. 用户级 secretary Duty 不隐式赋予项目 Task 成员资格。
4. Context/用户级入口不会继承项目型会话的工具授权；即使 Agent 身份相同。
5. 对已有 Agent/Conversation/Task/SQLite 快照进行兼容读取和迁移，失败时保留原数据并明确返回错误。
6. 历史执行证据能定位本轮 Duty/Scope，不以当前 mutable assignment 反推过去的授权。
7. 普通对话不绑定项目 Task 依然可进行（在实际连接与权限配置有效时）。

## 6. 验证限制

本批为 GitHub 源码与产品基线静态审计；未运行 Cargo、Windows 实机、端到端回归；未证明已经发生工具越权。文档仅记录当前缺口与候选修复顺序，不改生产源码、不更改 0.1.1 基线、不触发 GitHub Actions。
