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
