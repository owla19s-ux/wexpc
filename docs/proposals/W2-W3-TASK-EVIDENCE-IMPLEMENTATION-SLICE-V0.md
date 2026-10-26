# WEXPC W2/W3：任务事实与真实执行最小施工切片 V0

> 状态：施工前接口审查草案，非已实施代码或冻结 ABI。2026-10-27。
> 来源：[连接与执行契约审查](CONNECTION-EXECUTION-CONTRACT-REVIEW-2026-10-26.md)；执行范围以 main 的 [0.1.1 施工计划](../WEXPC-0.1.1-IMPLEMENTATION-PLAN.md) W1–W5、[PC 产品基线](../PC-PRODUCT-AND-TECH-BASELINE.md) 为准。产品版本始终为 **0.1.1**。

## 1. 第一批到底做什么

为现有桌面 Task 草案建立**版本化任务要求 + 成果引用 + 检查记录**，并在应用服务中有唯一可信写入路径，保留旧任务卡和人工日志。先证明“一个任务要求对应哪个版本的成果、做过哪些检查”，**不冒充自动派工、任务完成或 Windows 真机测试已通过**。

WEX 不成为外部 Agent 的全量执行平台：此切片是 WEX 自有任务账本的事实关联，不要求第三方支持相同内置运行时、工具日志、任务状态机。

## 2. 目标文件与边界

| 文件/模块 | 本切片工作 | 不要做 |
| --- | --- | --- |
| `crates/wex-desktop-service/src/domain/mod.rs` 或小型领域子模块 | 保留旧 Task/TaskRecord，增加内部任务要求快照、产物版本、检查事实的受控 DTO | 删除旧任务字段；以 AI 自述构造可信 Verify |
| `crates/wex-desktop-service/src/application/*` | 统一写入引用关系、要求版本冲突、来源校验；任务状态唯一写入点候选在 DesktopService | 新建第二套状态持久化，允许 UI/Agent 绕过权限直接写最终完成 |
| `crates/wex-desktop-service/src/infrastructure/sqlite.rs` | 针对当前 user_version=6 设计显式新版本升级、失败回滚与保旧测试 | 默认字段能反序列化就视为已完成数据库迁移 |
| `apps/desktop/src-tauri/src/main.rs` + `apps/desktop/src/types.ts` | 增加少量受控记录/读取命令及匹配类型 | 直接调用外部进程、在前端维护第二份 Task 真相 |
| `apps/desktop/src/Projects.tsx` | 先只读展示已有真实记录的来源/版本/状态；仍保留人工 TaskJournal | 把进展文本当作真实工具回执或已验收 |
| Core `task.rs`、`dispatch.rs`、`task_operation.rs` | 初期复用、测试和梳理应用映射；W3 才接真实派工 | 本切片先大改状态枚举或把 Operation 失败直接当任务结束 |
| Host 工具 `tool_journal.rs`、`tool_results.rs` | 保留原有 immutable marker / tool outputs；新增引用不得猜测旧记录归属 | 将全部普通对话历史改为必有 task_id |

## 3. 最小模型（语义而非固定表结构）

```text
RequirementSnapshot
  task_id
  requirement_revision
  goal + scope + acceptance
  publication_status / provenance  // 旧草稿不得凭迁移伪造发布批准

ArtifactReference
  task_id
  requirement_revision?            // 未明确归属时不能假造
  artifact_identity
  locator
  version_evidence | unknown
  provenance

TaskCheck
  task_id
  requirement_revision
  artifact_refs[]                  // 精确的成果版本引用
  checked_criteria[]
  kind: review | verification | acceptance
  outcome + evidence_refs[] + source + environment?
```

同一种记录载体可以承载三类检查，但 review、verification 与 acceptance **绝不能互当**。工作引用可用 operation/attempt 或外部交付 job 作为可选关联；不要求全部操作必有 Task。

## 4. 实施顺序与门禁

**S1 · 只读兼容和单一身份**：保存 schema6 fixture，验证旧 Task、TaskRecord、Agent、Conversation、工具证据仍可读取；旧任务 ID 不变，旧人工记录仍归类人工。不修改已有用户需求原文。

**S2 · 要求快照**：提供受控“发布/更新要求版本”操作；新版本不可无痕覆盖旧版本；Task 当前编辑可以继续存在，但已发布要求应有独立不可变引用。任务未发布仍显示 Draft，不借 Core Queued 伪造派发。

**S3 · 成果引用**：接受 Git commit SHA、文件内容版本、报告修订/批次或外部可核对引用；若确实没有可核对的版本则写 unknown，不能凭相同文件路径复用既往检查。

**S4 · 检查记录**：记录条件覆盖、产物版本、工具/审查者、证据、环境及已知结果；没有运行的 Windows 实机检查必须是 NOT_RUN/UNKNOWN，不得显示 PASS。只有 WEX 可核实的事实才能获得对应可信标识。

**S5 · 与执行链合流（W2/W3）**：再将发布要求、明确派工身份/代次、现有 Host 的 operation/attempt/marker 和成果引用关联；派发时 Host 继续做最终授权及资源校验。普通对话保持无 Task 执行能力。

**S6 · 回归与恢复**：覆盖迁移失败保旧、重复证据去重、旧成果验证不继承、新 Agent 迟到回执、工具 UNKNOWN 无盲重试、不同 Agent 非冲突写入，以及磁盘 marker 与 SQLite 写入不同步的恢复显示。

## 5. 测试矩阵与完成声明

| 检查 | 最低验收条件 |
| --- | --- |
| 旧数据 | schema6 可加载，升级事务失败不改坏原数据 |
| 要求版本 | V1、V2 历史并存，人工日志保留，不自动产生历史批准 |
| 成果版本 | 同一 locator 的不同内容版本不会共享已通过结果 |
| 检查范围 | 单元测试通过不等于 Windows 真实运行验收 |
| 来源可信 | 人工报告、AI 文字、Host 工具回执、外部可核实数据能区分 |
| 无任务调用 | 普通聊天使用工具不需要生成 Task |
| 写入安全 | 授权撤销、UNKNOWN 和崩溃场景不自动重复外部写入 |
| 复用 | 没有第二执行器、第二 Dispatcher 或强制外部 Agent 插件协议 |

**完成级别分开记录**：文档审查完成 / 代码接线完成 / 本地测试通过 / Windows 实机通过，四者不可互相替代。

## 6. 待确认的最少技术取舍

- 是否在既有 DesktopSnapshot 单行 JSON 中新增集合，还是采用一份独立关联存储（必须核对性能、事务、跨文件恢复和迁移成本）。
- 业务 Task 到 Core 单 Agent 派工的最小映射实体，以及多成员并行的关联粒度。
- `task_operation.rs` 中单 Operation 失败引起 Task Failed 的修正规则：保留操作失败证据，任务层再决定修复/阻塞/终止。

上述不影响已确认的产品方向，但**没有为任一方案授权修改生产代码**。施工时应先做最小源码与测试更改，再根据实际证据作取舍。

## 7. 用户最新确认：内容归属、跨 Context 引用、执行者可替换（2026-10-27）

> 本节为用户对上一轮三个问题的直接答复；优先于本草案中不兼容的“全部资料按 Task/Agent/API 组织”假设。具体数据物理格式和文件目录尚未冻结。

1. **按内容实际归属组织**：未分类的普通/短期对话可以作为独立记录保存，不强制成为长期 Context 或项目；属于项目的对话、任务及成果归属项目；长期 Context 可以组织或引用已有对象，避免为引用而重复拷贝。用户使用“单个文件”描述独立保存的方式，后续应区分逻辑上的独立内容单元与实际物理文件格式，不能未经验证直接冻结一对话一文件。
2. **归属不等于独占访问**：已有项目资料和独立对话可在有效授权下由其他 Context、Agent 或工具读取、调用和引用；跨项目/Context 的引用不自动改变原归属，也不自动扩权。
3. **任务不永久绑定角色/模型/API**：同一 Task 在不同时刻可由不同 Agent、模型、连接与外部执行环境处理。保存分派、交接、换人、换 API、失败、继续和恢复的真实历史；单次执行关系才引用当时实际执行身份与能力。
4. **执行与调度记录独立于最终 Task 结果**：一个任务可以有多次尝试；失败不擦除旧记录，下一轮修复不改写旧回执。任务结束依据需求/成果/验证，而非某个执行者或某次工具成功。
5. **物理持久化开放**：建议仍利用现有 SQLite 管理对象身份、归属、权限关联和状态/索引，但独立对话正文、项目文件及外部产物可按类型落在各自合适的存储位置；不能以单行 JSON 快照限制长期组织，也不能擅自迁移现有文件。数据库与文件的具体事务边界需要迁移/恢复测试确认。

### 对第一施工切片的约束变动

- `task_id` 不成为普通对话、独立文件或长期 Context 引用的前置字段；可为项目任务的成果建立显式 task 关联。
- 添加/恢复内容时必须区分 **owner/归属** 与 **reference/引用**：多个 Context 可以指向同一个有稳定 ID 的对象，但对象不能因被引用自动移动目录或暴露更大权限。
- 不将 `assigned_agent_id`、`model_id` 或 `connection_id` 作为 Task 的永久身份属性；它们属于相应分派、执行尝试或会话绑定的历史事实。
- UI 可显示当前执行者、前任执行者、历次尝试、失败/修复/换模型等，但不能从“当前执行者”推断整个 Task 只有一个执行主体。
- 测试增加：独立短期对话不建项目；项目内资料跨 Context 引用仍归原项目且遵循授权；执行者/API 切换后旧 attempt/receipt 保留，新尝试不继承旧权限；有失败后持续执行及多个成果版本独立检查。

**确认程度**：以上为产品与数据语义方向已确认；数据库表、文件目录、文件格式、角色/权限 API、迁移实现仍需源码对照，不视为已经编码。

## 8. 第十五批：已有 Project / Context / Conversation 的归属与引用接口核对（2026-10-27）

### 8.1 main 源码观察

本批读取 `crates/wex-desktop-service/src/domain/mod.rs`、`application/organization.rs` 和 `apps/desktop/src/types.ts`。基于当前已读路径：

| 现有结构 | 已观察到的事实 | 不能视为已实现 |
| --- | --- | --- |
| `Project` | `id/name/goal/resources/context_id?`；resources 注释声明只是引用、不授予工具权限 | 多 Context 引用同一项目的完整索引、项目文件物理归档策略 |
| `Conversation` | `id/project_id?/context_id?/connection_id/model_id`；可不属于项目或 Context | 一段对话一个物理文件；跨多个 Context 的显式引用集合 |
| `Context` 与 `ContextPlacement` | Context 自身有稳定 ID；placement 指向 Space、可选 Group，支持单位置移动 | 每次移动必须移动实际资料/变更所有权 |
| `bind_conversation_context` | 校验 Context 是否存在后修改 Conversation 的一个 `context_id` 字段 | 多个 Context 同时引用同一对话；被引用时不会误认为重新归属 |
| `bind_project_context` | 修改 Project 的单个 `context_id` 字段 | 多 Context 对同一项目的引用关系 |
| `Task` | `project_id` 必填，`agent_ids` 列出参与成员，仅 Draft 状态 | 记录不同 Attempt 对应的实际执行 Agent、API 和委托环境 |
| `Agent` | 实例保存 `role/connection_id/model_id/conversation_id` | 换 API、模型或执行者时自动生成可核实执行历史 |

### 8.2 用户已确认语义与现有结构的落差

**重要：不能把 `context_id` 同时当成独占拥有者和可多处引用的集合。** 已有 Project/Conversation 的 `context_id` 是单值关联；虽然可以重新绑定，但没有已证实的多对多“引用同一个对象”语义。若要实现“仍归原项目、在其他 Context 引用”，应**保留原对象 ID/原归属**，使用独立的受控引用关联，而不是通过 `bind_*_context` 改写原归属。

同时，用户所说“未分类对话独立保存”与 `Conversation.project_id=None,context_id=None` 已具备的逻辑独立性相容。它**不等于**现有实现已经使用单文件储存，暂不修改物理存储方案。

关于模型/API：`Conversation` 和 `Agent` 当前存默认连接/模型选择。这与“Task 不永久锁定执行模型/API”不冲突，但真实执行时应**记录本次尝试实际使用的绑定和来源**，不能仅通过事后读取当前 Agent/Conversation 配置来倒推旧执行事实。

### 8.3 优先采用的最小补充接口（候选，不是已批准 ABI）

```text
ObjectReference:
  reference_id
  source_object_kind + source_object_id
  target_context_id
  purpose?                      // 如索引/研究引用，不改变所有权
  created_by + created_at

ExecutionAttempt:
  attempt_id
  task_id?
  assignment_ref?
  actual_agent_id?
  model_or_provider_ref?
  connection_or_external_runtime_ref?
  operation_or_external_job_ref?
  outcome + evidence_ref?
```

规则：`ObjectReference` 为 **可选跨 Context 引用**，并不授予访问权；对象实际归属仍由已有稳定 identity/项目字段及后续正式归属规则决定，不能从引用推导 ACL。无 Task 的普通对话依旧有效。执行尝试记录的是**历史当时值**，后续换 Agent/模型/API 不回写旧 Attempt。

避免急于新增万能 ObjectStore、通用抽象平台、强制“所有内容归属 Context”逻辑。先用现有具体实体类型与兼容性检查建立最窄关联。

### 8.4 应当新增的用例

1. `Conversation(project_id=None,context_id=None)` 可以存在和使用，不生成虚构项目。
2. 项目原归属不变的情况下，两个 Context 可以在各自有效授权下引用同一个项目资料；取消一处引用不删除源对象。
3. 原属项目被移动到其他 Context 时，原始 Task、执行回执及真实资料地址不因展示位置变化而更改。
4. 一个 Task 连续经历 Agent A + 模型 X、Agent B + 模型 Y、再由 A 恢复，三个 Attempt 都保留当时执行身份、真实证据和结果；旧模型绑定不被事后覆盖。
5. 改变当前 `Agent.connection_id/model_id` 不会重写已经发生的历史 Attempt，也不会从旧许可推断新模型/外部运行时获得资源权限。
6. 无可核实执行回执的“AI 自述已调用模型”只算声明，不自动制造一次真实执行事件。

### 8.5 施工优先级调整

在上一节 S1 兼容测试基础上，先补**归属与引用不能互相覆盖**的回归测试；再将跨 Context 引用作为最小引用索引能力，与 Task 成果/验证记录平行推进。执行尝试的实际 Provider/Agent 身份在 W2/W3 接线时记录，不能仅将 `Task.agent_ids` 当作派工历史。

以上仅是代码审查所得的接口候选；未检查完整跨 Context ACL、当前模型请求实际拼装与所有存储路径前，不宣布参考实现已经完备，也不直接变更生产数据结构。

## 9. 第十六批：跨 Context 引用的授权链与威胁边界（2026-10-27）

### 9.1 当前源码检查范围

核查 main 的 `application/organization.rs`、`application/mod.rs`、`domain/mod.rs` 与 Tauri `tool_approvals.rs`：

- `bind_conversation_context`、`bind_project_context` 校验目标 Context 存在后修改单值关联；在这两个操作中**未观察到 OS 目录、连接实例、会话工具授权的赋权动作**。
- `Project.resources` 注释明确是引用，不赋予工具权限。
- `assign_agent_duty` 所在代码注释明确工作职责分配不等于工具或资料授权；Task 创建按项目职责关系检查可关联成员，**不代表成员得到该项目的全部文件读写许可**。
- Tauri Tool bridge 的本地工具按照目录授权、conversation、操作类别和执行策略提供能力；GitHub 读写同样带当前连接、对话策略及 Core 授权。此前设计的跨 Context 引用不能绕开这些既存的 Host 校验。
- 上述是所检查代码的局部事实，**不是全仓安全审计**；本批不宣布跨 Context 访问控制已闭环。

### 9.2 最小安全规则（沿袭已确认的“组织 ≠ 权限”）

**引用登记与实际访问必须分成两件事。**

1. 可以保存 `Context B → Object X` 引用，只代表用户想在 B 中定位/使用 X；不能由引用自动复制 X 的凭证、目录权限、会话政策或 Task 执行范围。
2. 用户在界面查看某条引用时，先核实对象仍存在、引用仍有效及适用的数据读取范围；不能把曾经显示过的内容或缓存直接当作永久访问授权。
3. AI 或工具请求访问引用对象时，必须再按当前真实执行主体（Agent/Conversation/外部委托）、目标资源和操作类型执行授权检查。**只拿到 reference_id 不能跨权限读取文件。**
4. 源对象发生删除、移走、断开连接、密钥失效或授权撤销时，引用可以保留为不可访问/待修复，但不能假装内容仍然可读，更不能从过期索引取密钥。
5. 跨 Context 可见性、搜索和全文索引不应通过关联关系绕过来源范围；检索摘要、工具结果和缓存同样可能泄露原文，需要检查其真实来源授权。
6. 归属变更是独立且明确的业务操作；取消引用不是删除来源，更不是资源权限变更。Context 的位置移动也不自动调整实际 OS/第三方授权。

### 9.3 建议的最小检查边界（非冻结接口）

```text
add_context_reference(target_context_id, source_kind, source_id)
    -> persisted_reference_or_refusal
    # 验证目标与来源存在、操作者可登记引用；不颁发资源访问许可

resolve_reference(reference_id, requester, purpose)
    -> source_identity_or_denial
    # 引用只解出源对象的身份；按当前请求者与用途检查是否可公开元数据

read_or_execute_on_source(source_identity, current_authority, action)
    -> data_or_operation_receipt_or_denial
    # 使用真正源资源/Host 的访问决策；Task/Context/角色名不能代替凭证和授权
```

具体 UI 的展示许可与工具执行授权不一定是同一种权限；必须分开建模，但没有证据时不要提前发明统一的超级 ACL。

### 9.4 最小攻击/故障用例

| 试验 | 必须得到的结果 |
| --- | --- |
| Context B 引用 A 的报告，B 无底层文件许可 | 可保留引用关系；AI 通过 B 请求读取必须被拒绝或要求适当授权 |
| B 引用 A 的项目并关联 Task/Agent | 新 Task/Agent 不自动继承 A 的目录/GitHub 权限 |
| A 撤销本地目录读许可 | 已登记引用不应继续产生可执行的目录读取权限 |
| 原文件被移动或修改，引用仍存在 | 不从旧文件路径/旧内容摘要假定最新内容有效 |
| 搜索跨 Context 项目资料 | 结果和摘要受到请求者可访问范围约束，不泄露受限内容 |
| 切换 Agent 使用不同 API/Conversation | 新执行请求按新身份/连接/策略重新鉴权；旧成功不能当作新授权 |
| 删除某 Context 的引用 | 源对象及其任务/执行/验证历史不被级联删去 |
| 未知结果的外部写操作 + Context 移动 | 源写入状态仍 UNKNOWN；不能因移动/重分类重新执行 |

### 9.5 对施工顺序的具体影响

- **优先测试已有拒绝路径**，然后再添加跨 Context 引用索引；不能先写一个“跨项目资料全局检索”并在之后才补授权。
- W2 最小关联库需保存来源 ID 和引用本身，但不复制原文件权限和原始凭证。
- W3 真正执行前继续由 Host 检查实际作用域，Dispatcher 的任务/分工票据只证明调度有效，不是授权令牌。
- Task 或 Agent 换模型与 API 后，历史执行记录不变，新动作走当次真实身份与可用连接权限。

本批只更新文档；没有更改代码、数据库、主分支或执行安全测试。

## 10. 用户确认：稳定 ID、用户自定义名称、显示序号相互独立（2026-10-27）

**正式语义原则**：Context 的内部稳定 `id`、用户看到且可编辑的 `name`、用于视图展示的 `display_order` / `display_number` 只需有对应关系，**不得强行相等或相互推算**。名称不必全局唯一；序号只服务组织和排序，可调整、不连续、在不同列表/位置不同。

- ID：系统长期引用身份；重命名、重排序、移动 Space/Group/Context 不更换 ID，也不改写 Task/执行/证据历史。
- 名称：用户自定义显示属性，不参与唯一身份、授权、引用解析与回执去重。
- 序号：展示位置属性；可作为 UI 显示的编号，但不能充当对象主键、权限/执行标识、数据库持久身份。可以按具体视图或 `ContextPlacement` 管理，而非一定固定写死到 Context。
- 同样原则可应用于 Space、Group、Project、Task、Conversation 等可展示对象，具体排序机制不因本节立即统一实现。
- **兼容现有数据**：当前 Context/ContextPlacement 的 id/name/placement 已存在，不得因为缺少序号就生成新 ID；新增排序字段应可选并有稳定显示回退，迁移时保留用户名称和对象关联。
- **测试**：两个同名 Context 不冲突；改名称/排序不影响所有关联和授权；同一对象在不同展示视图可有各自序号；显示编号缺失或重复不造成记录错误归属。

本节不冻结具体字段名、序号格式、排序存储位置或前端交互，也不要求把 ID 暴露给普通用户。
