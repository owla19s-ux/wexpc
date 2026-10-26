# WEXPC 连接与操作边界：增量审查记录（工作稿）

日期：2026-10-26  
状态：**增量审查工作稿，不是正式接口规范**。已确认的架构边界引用 PC 基线 §4.2–4.3；具体字段、Rust trait、SQLite 迁移和安全策略尚待审查/用户确认。本文仅作为下一轮细化的入口，**不改变 0.1.1 目标或现有执行行为**。

## 0. 依据与约束

- 正式基线：[PC-PRODUCT-AND-TECH-BASELINE.md](../PC-PRODUCT-AND-TECH-BASELINE.md) §2.3、§4.2–4.3、§5；工程计划：[WEXPC-0.1.1-IMPLEMENTATION-PLAN.md](../WEXPC-0.1.1-IMPLEMENTATION-PLAN.md)。
- 研究草案：[DECISIONS.md](../../DECISIONS.md) DRAFT-1。**DRAFT 内容不因在本文件复述就变为已确认规则**。
- 当前实现静态参照：`crates/wex-desktop-service/src/domain/mod.rs`、`application/mod.rs`、`crates/wex-core/src/connection_session.rs`、`operation.rs`、`apps/desktop/src-tauri/src/tool_approvals.rs`、`github_config.rs`。以下仅说明静态观察，不代表全量调用审计或 Windows 运行验证。
- 不做上层“联通”语义导致的代码重命名；不设第二套授权、任务状态或连接目录；不把外部 DSH/Hermes 内部运行历史当 WEX 必须存储的数据。
- 优先保留现有 API/文件/进程/Git/GitHub 的执行及安全机制。新增架构只定义它们共享的边界，不提前做完整插件市场或通用可视化工作流。

## 1. 本次发现的具体耦合（已核对源码）

| 位置 | 静态事实 | 为什么需要审查 |
| --- | --- | --- |
| Desktop Service `domain/mod.rs` | `NewConnection`、`AiConnection` 为 AI 特定结构；`DesktopSnapshot.connections: Vec<AiConnection>` | 无法直接承载非模型连接及动态能力 |
| Desktop Service `application/mod.rs` | Catalog 从 AI 连接投影；`prepare_chat` 构造含会话、模型和凭证的 `PreparedChat` | 一部分能力入口耦合于聊天上下文 |
| Core `connection_session.rs` | 已有连接记录、类型、启停及 Catalog；模型选择和对话解析是其一部分 | 有可复用基础，不应直接另起一套重复 Catalog |
| Core `operation.rs` | `OperationIntent.conversation_id` 必填；批准/派发/未知结果已有独立生命周期 | 无 Task 或无 Conversation 的调用需要明确来源模型 |
| Tauri `tool_approvals.rs` | 多类模型工具提议集中组装及分派，并广泛依赖 `PreparedChat` | 将未来所有通用调用继续塞进此文件会扩大耦合 |
| Tauri `github_config.rs` | GitHub 单独配置文件与 vault 凭证引用，启停及撤权有专门逻辑 | 通用身份可以增加，但不应消除敏感资料隔离 |

以上还不能得出“运行时存在多个独立授权系统”的结论；要逐路径追踪最终核验后才能定性。

## 2. 已确认的职责边界（不新增决策）

1. **组织**：Space → 可选 Group → Context 关联对象、资源与工作背景，引用和分类不自动授权。
2. **连接/能力**：管理实例、状态、能力查询及目标解析；不拥有任务队列、Task 正式状态或执行结果的权威结论。
3. **适配器**：与具体协议、应用、外部 Agent 对话，按实际支持返回发送/查询/取消能力，不要求对方实现 WEX 内部协议。
4. **执行服务**：作为 UI、普通对话、Agent/Dispatcher 的共用操作请求、授权协调、持久操作事实和回执结算边界；最终执行层继续实施安全核验。
5. **Dispatcher**：确定性排队、路由、依赖、范围占用、尝试与恢复，不替模型决定技术方案。
6. **Task**：领域服务唯一维护工作目标、范围、验收、版本与正式任务状态。
7. **原生/外部**：原生操作留存自身可信调用证据；外部委托仅记录 WEX 可观测的投递、接收、定位、成果与核验。外部失联/结果未知不能盲目重派。
8. **并发**：WEX 内部锁不能约束任意外部进程；外部真实隔离尚待验证，不能用“目录不同”或“设置了工作范围”冒充沙箱。

## 3. 提议的最小逻辑对象（尚未冻结字段）

| 对象 | 最小语义 | 与其他对象的边界 |
| --- | --- | --- |
| AdapterDefinition | 一种协议/实现及其支持范围 | 不等于已安装实例、凭证或授权 |
| ConnectionInstance | 适配器及配置引用、稳定身份、启停/可用状态 | 不强制所有对象具备模型列表；不持有 Task 状态 |
| CapabilityDescriptor | 某来源提供的能力、输入输出契约与限制 | 声明不自动等于实际可调用，更不产生权限 |
| CapabilityBinding | 某能力在特定连接或内置来源上的可用绑定 | 同一种能力可有多个来源，选择不意味着可跨账号互换 |
| CredentialRef | 指向 OS/vault 等受控敏感存储的引用 | 不把密钥写进普通 Snapshot/回执 |
| ExecutionTarget | 操作实际到达的环境/资源定位 | 与连接身份分离，跨连接同资源的写冲突须识别 |
| Operation/Attempt/Receipt | WEX 自有请求、尝试、真实反馈与不确定性 | 可独立于 Task/Conversation；不能用工具成功代替 Verify |

**关键建议**：统一的是能力执行语义，而不是强迫纯本地内置功能也必须创建“外部连接账号”。可考虑把内置来源视为可枚举的能力提供者；具体是否形成持久 ConnectionInstance，列为待确认选项。

## 4. 拟保持的主要调用约束

- 来自 UI、模型工具循环或 Dispatcher 的操作，先形成稳定的 actor/operation 关联；Conversation、Task、Context 根据实际用例可选，但权限不从任一组织关联推导。
- 调用前区分 **能力可见、可用、授权通过、适配器已接受、结果已知、独立核验**，各状态不能互相代替。
- 对带副作用的操作，执行前保留 durable InFlight/尝试关联，发生结果未知时不得自动重放；断开和取消不推断远端已停止。
- 执行服务协调控制与回执，流式传输和大型结果可保留专用数据路径，不能要求全部字节通过同一个状态数据库更新。
- 以实际资源身份而非仅 `connection_id` 判定跨连接冲突；无法识别外部任意 I/O 时明确降低并发保证。
- 任务编排与纯操作分离：Task 可对应多个操作，普通操作可不对应 Task；Dispatcher 调用执行服务，而不绕过最终权限检查。

## 5. 尚需确认、不能直接改代码的三个问题

### Q1 — 内置能力是否必须形成持久 ConnectionInstance？

**建议**：不强制。统一 CapabilityBinding/Operation 入口，用户配置的连接才进入连接实例目录；内置能力有明确来源及权限边界。  
**需验证**：现有 Core Catalog 是否能容纳内置来源，是否需要兼容历史 literal 工具 ID。

### Q2 — 统一执行服务应放在何处？

**建议**：先以 Rust 应用服务中的薄协调层承接，Tauri 保留 OS/进程/vault 与实际执行，Core 保留可测试的纯状态/检查规则；逐步迁移工具入口，禁止一步搬空全部 Host。  
**需验证**：跨服务状态提交的原子性、崩溃窗口、回执发布一致性；不得凭设计图决定 crate 数量。

### Q3 — 旧数据与连接身份的迁移何时实施？

**建议**：先稳定接口与数据映射，保留旧 AI ID、凭证引用和关联；新增 GitHub 连接身份只能在安全迁移确认后实施。  
**需验证**：当前 Snapshot schema、回执/权限存储、前端 TS 类型、旧版本写回行为；迁移应提供失败保护与旧程序拒绝策略。

以上问题在获得确认及具体兼容验证前，不修改正式基线中的实现承诺，也不将方案视为可直接施工的 DTO/trait。

## 用户确认记录（2026-10-26）

本轮用户确认以下**三个设计方向**，可据此继续细化接口和迁移方案；这不表示具体 DTO/trait/数据库版本、权限策略或实现已经获批。

1. **内置能力无需强制创建持久 ConnectionInstance**：统一能力发现、调用和授权边界；需要明确配置的外部服务使用 Connection，内置能力可直接作为来源明确的 CapabilityBinding。具体目录实现及历史兼容仍待技术核对。
2. **统一执行服务优先采用现有 Rust 应用服务中的薄协调层**：复用 Core 状态及安全规则，Tauri/具体 Adapter 继续负责实际 OS、进程、凭证和协议操作。不得新建第二套调度、授权和任务真相。具体放置及跨层崩溃一致性仍需设计验证。
3. **连接数据先设计契约与兼容映射，之后实施迁移**：保留既有连接 ID、凭证引用、会话/Agent 绑定与历史回执；迁移必须防止失败覆盖和撤权失效。具体 schema、迁移时点和降级策略仍待审查。

此记录将 §5 的 Q1–Q3 从“待用户确认方向”调整为“方向已确认、实现细节待审查”。不得因方向确认直接修改正式产品权限规则、数据库或业务源码。

## 6. 下一轮审查与可接受条件

1. 沿真实调用路径画出普通 AI 工具调用、无 Conversation 本地操作及 Dispatcher 派发的授权/回执交汇点；逐个核对谁持有权威状态。
2. 按实际调用方整理 ConnectionInstance / CapabilityBinding / Operation 的**字段候选、版本、错误语义与迁移影响**，并标记每个字段的必要性。
3. 核对写入前持久标记、批准凭据、取消、撤权、重复事件、跨连接共享资源；不将既有安全检查降级为 UI 提示。
4. 对照 0.1.1 两 Agent 真实施工与独立 Review/Verify 用例做设计验收；单一聊天工具演示不作为完整验收。
5. 若与已确认 PC 基线冲突，记录章节、事实、两种方案及影响，停下等待用户决策，再改正式规范。

## 7. 当前变更边界

本文件本身只新增设计工作稿，无应用源码、数据库、接口签名、权限规则或正式基线修改。未运行 Rust/Windows/真实服务/Actions 测试；不对能力接通或安全隔离作成功声明。

## 8. 第二批：源码级调用链及最小契约审查（2026-10-26）

本节已实际读取 main 对应源码：`wex-core/src/operation.rs`、`connection_session.rs`、`wex-desktop-service/src/domain/mod.rs`、`application/mod.rs`、`apps/desktop/src-tauri/src/tool_approvals.rs`。**本节只记录静态代码可见事实，不等于已审完全部派工和持久化链路。**

### 8.1 可直接证明的当前调用路径

- **配置 AI**：Desktop Service `add_connection` 强制 `base_url`、`models`，创建 `AiConnection`；其内部构造临时 `Catalog` 并注册 `ConnectionKind::Ai`。因此 Core Catalog 支持 Tool 种类，当前产品快照仍只持久化 AI 连接。
- **准备对话**：`prepare_chat` 解析会话绑定、模型与凭证引用，生成 `PreparedChat`。这属于 AI 会话专用请求快照，不应升格为通用能力请求基类。
- **模型工具**：Tauri `tool_approvals.rs` 对 tool name 执行显式分派，分别进入文件、命令和 GitHub 的具体处理器；部分操作会创建 `OperationIntent`、`OperationLedger`，批准后调用 `begin_approved_dispatch`。此入口是现有模型工具路径，并不能直接证明无人对话任务已复用。
- **操作领域规则**：Core `OperationIntent` 目前必须带 `actor_id`、`conversation_id`、`connection_id`、`capability_id`、`resource_id`、`intent_digest`；`begin` 验证批准版本、连接启用、身份认证、平台与资源可用，然后进入 InFlight。Core 注释明确：必须由可信持久化服务先记录 InFlight，再派发外部副作用。
- **现有本地调用样例**：在 Tauri 文件读取处理中，`connection_id = local`，`capability_id = local_file_read`，且 `conversation_id` 从 `PreparedChat` 取得；这里的 `local` 是已有操作来源标识，不证明已经存在一个用户配置的持久本地 Connection。
- **工具输出**：模型工具分支显式调用 `save_tool_result`，使用 `conversation_id`、`turn_id`、tool call 及实际结果保存输出；这份会话输出保存不能自动替代跨任务的 Operation/Attempt/Receipt 权威关联。

### 8.2 最小接线提案（仅逻辑边界，不冻结 Rust 签名）

| 入口 | 控制信息 | 实际执行 | 最终关联 |
| --- | --- | --- | --- |
| 普通对话工具 | actor + 可选 conversation + capability + 资源/参数摘要 | 沿用当前 Host/Adapter | operation / attempt / receipt；会话工具结果按原专用格式继续保存 |
| 用户直接操作 | 用户 actor + capability + 资源/参数摘要；不必造 conversation | 沿用现有执行器 | operation / attempt / receipt；无 task 亦可 |
| Dispatcher 派工 | task/revision + assignment + actor + capability + 资源/参数摘要 | 通过同一可信执行协调边界使用 Host/Adapter | task → dispatch → operation / attempt / receipt，再交回任务服务 |
| 外部 Agent 委托 | 目标 connection/capability + task/operation + 外部定位和交付批次 | Adapter/原生外部运行时 | WEX 自身投递与接收证据、成果定位、验证引用；不伪造外部内部工具事件 |

**必须保留的两个分离**：

1. 能力入口不以 `PreparedChat` 为公共参数；模型工具适配层将其映射为通用操作请求，而不是删掉现有专用对话信息。
2. `OperationLedger` 是纯领域状态机，不负责数据库事务、OS/Vault、资源许可核验或外部副作用；持久记录与最后一次有效权限核验必须在可信边界完成。不能简单把一个内存 Ledger 移到新 crate 就声称任务级可恢复。

### 8.3 建议的接口评审检查清单

- **来源身份**：区分 `actor`、`conversation`、`task/revision`、`operation/attempt`；缺 conversation 不得通过生成假会话解决。
- **能力身份**：区分来源提供者（内置或持久 Connection）、能力种类、配置版本和实际目标；内置来源不默认取得 OS 文件读取权。
- **授权事实**：用户策略与资源/账号权限分开；模型工具声明与目录可见性不形成可信授权；保留执行前最终核验。
- **调度与结果**：Dispatcher 的投递/lease 不等于外部成功；Operation 的 `Succeeded` 也不等于 Review/Verify 通过。未知结果阻断有副作用的自动重试。
- **持久化边界**：副作用之前 durable InFlight，发布回执及会话输出的原子性/顺序/去重需要沿现有实际存储调用继续核对。
- **错误与兼容**：未配置连接、能力不存在、认证失败、授权拒绝、版本冲突、适配器拒绝、超时 UNKNOWN 分别呈现；不因通用化折叠成 `false` 或空字符串。

### 8.4 本批新发现的风险与待核对项

**R1（重要，需继续审查）**：当前 Tauri 工具执行样例会按操作新建内存 `OperationLedger::default()`；已有 durable marker、回执和中断处理位于其他边界，**不能由这一片代码断言持久化缺失**，但也不能认定全部 Operation 状态和任务级恢复已被持久统一。下一轮必须追踪 `begin_approved_dispatch`、marker 写入、工具输出、回执持久化及错误/取消时的结算顺序。

**R2（重要，已知设计约束）**：`OperationIntent.conversation_id` 为必填，不支持无对话任务的真实输入。拟允许可选来源前必须核对所有 `valid_intent`、序列化、测试、UI 和回执读取调用者，不能仅把 `String` 改成 `Option<String>`。

**R3（重要，已知设计约束）**：Core `ConnectionKind::Tool(adapter)` 可以表达工具类别，但 DesktopSnapshot 持久层强制 `AiConnection`。不要创建平行的第二份可修改 Connection 真相；先制定从旧 AI 实例及 GitHub 独立凭证引用映射到统一读取视图的规则。

**R4（需要审查）**：模型工具名称分派已集中于 Tauri；提取可注册的能力绑定应先保留每一条具体参数验证、权限复检和结果处理，再迁移调用者；不能先暴露通用执行后靠 UI 判断安全。

### 8.5 已允许推进与暂缓项目

**可继续审查/写草案**：现有执行路径、最小来源关联、能力提供者与绑定关系、操作错误语义、兼容测试清单。

**须先完成专项核对或另行确认再改**：通用化 `OperationIntent` 的结构、旧 Snapshot 数据格式、GitHub 连接身份落盘、外部进程隔离方式、任何授权默认放宽及正式 Product Baseline 条款。

本批没有修改 Rust、Tauri、React、SQLite、权限策略或正式产品基线；未运行 Actions / 编译 / 测试。

### 8.6 追加核对：Core InFlight 与持久 marker 并不是同一个状态容器

在 `tool_approvals.rs` 的通用进程路径中，`begin_approved_dispatch` 先通过权限回调与内存 `OperationLedger.begin`，随后 `journal(..., "in_flight", ...)` 写入 `tool-receipts`，然后才调用进程 Supervisor / 实际进程执行。就此片段的**调用顺序**而言，已安排持久 marker 早于进程外部副作用，不能将“使用临时 Ledger”误判为没有 durable InFlight 防护。但记录的原子性、各类文件/GitHub 路径是否同样遵守、失败回收、旧 marker 兼容，以及 marker 与未来持久 Task/Dispatcher 的一致性仍需逐路径核实。

**建议**：未来薄执行协调层应复用当前 marker/receipt 写入能力与已验证的故障处理；先描述它们的现行权威范围，再统一引用，不要直接把 `OperationLedger` 的纯内存记录宣布为新的唯一持久操作数据库。

## 9. 第三批：Dispatcher ↔ Task ↔ Operation 交接审查（2026-10-26）

### 9.1 首要设计约束：协调宿主，不是全量执行平台

**用户新增明确约束**：WEX 是连接、调度、交互和跨系统联通的宿主，不能将它建成对所有执行方式进行绝对标准化的全量执行平台。统一的是**WEX 侧可控制、可记录与可验证的交接边界**，不是要求每个外部系统内化为 WEX 的 Runtime、Task、工具循环或日志模型。

- 对原生文件、命令、Git/GitHub 等已由 WEX 实际执行的操作，继续提供安全检查、操作追踪和可信结果。
- 对 DSH/Hermes 等外部 Agent，只规范 WEX 发出的委托、外部定位、可观测投递/接收、成果引用及独立核验；能力不具备时明确报不支持或未知。
- Dispatcher 只编排 WEX 可掌握的队列、依赖和交接，不接管第三方进程的内部调度；真实的外部并发控制须依赖实际隔离/协调能力。
- 统一契约采用薄公共信封与分类型负载；普通请求响应、长进程、持续会话、模型流和异步委派保留各自专用语义。不要求所有能力都支持 cancel、progress、stream、resume。
- 不把独立能力强制包装成用户可见 Connection，也不为了接口一致新增伪造的 Conversation、Task、Agent 或模拟工具回执。
- 本条作为架构审查约束写入工作稿；具体字段和持久化实现仍待后续收敛，不因此扩大 0.1.1 产品门槛。

### 9.2 核对的 Core 事实

| 源码 | 已经具备的规则 | 尚不能据此宣称完成 |
| --- | --- | --- |
| `wex-core/src/task.rs` | `TaskBoard` 维护任务、Agent 分配及 `assignment_epoch`；中断恢复将 Running 恢复为 Blocked；可信证据去重 | 完整桌面任务服务已持久关联真实工具、审查与验证 |
| `wex-core/src/dispatch.rs` | `admit` 核对项目、状态、Agent 与分配代次；`reserve` 取得资源 lease 并复检 ticket；注释明确 ticket 不是批准 | 获得 ticket 就拥有权限、或 WEX lease 能约束所有外部进程 |
| `wex-core/src/task_operation.rs` | `TaskOperationIndex` 验证任务/操作/Agent 关联；旧分派结果不得更改新分派进度；成功仅收证据，UNKNOWN 阻塞任务 | 任何成功工具回执可以跳过 Review/Verify 直接关闭任务 |
| `wex-core/src/operation.rs` | `OperationLedger` 约束审批、InFlight、结果及 Attempt；属于纯状态规则 | 内存状态机自身就是跨进程持久事务或远端运行时 |

### 9.3 建议的最小跨边界交接契约（语义级，待编码）

**Task → Dispatcher**：交付 `task_id`、项目、当前分配 Agent、`assignment_epoch`、任务版本/工作范围引用、所需能力与依赖引用（如实际存在）。Dispatcher 验证资格；不从任务关联推导新增权限。

**Dispatcher → 执行入口**：在当前范围可用、租约有效时，交付稳定的派发/尝试关联、调用身份、能力或明确外部委派目标、资源与意图摘要。执行入口再次核验可执行权限。无 Task 的普通操作仍可直接走执行入口。

**执行入口 → Task**：返回 WEX 可观察的 `operation_id`、`attempt_id`、已知结果或 UNKNOWN、可信证据/成果定位；由 Task 领域规则管理正式进度。完成、审查、验证是独立事实；不按外部返回的自由文字自动更改正式终态。

**外部 Agent 专项**：若外部只支持提交任务和稍后取得交付文件，交接契约可只保留 WEX 投递结果、批次及产物关联；不要求生成虚构的每一步工具 Attempt。若接口无确认能力，状态保持不确定而不是补造接受事件。

### 9.4 四个实现风险（待继续核对）

1. `DispatchGuard` 的资源 lease 是当前 Core 控制边界；不能当作跨进程、外部 Agent 或 OS 沙箱。后续需查 Host/Dispatcher 实际使用调用者和失败释放路径。
2. `TaskOperationIndex` 是读操作事实并更新 Task 进度的投影；源码注释明确由 Host 负责事务性保存，须进一步核对实际业务持久化和崩溃恢复是否接通。
3. `OperationIntent.conversation_id` 仍为必填，与无需 Conversation 的任务操作场景有语义差异；待完成调用方兼容审查，不直接改字段。
4. `TaskStatus::Failed` 目前可由失败的 Operation 回执触发；需区分“某步失败、可继续修复”和“整个任务失败”的产品语义，**不得未经确认改变现有 Task 转移规则**。

### 9.5 拟定的最小可编码契约包（待进一步核查）

- **A — 能力引用**：支持内置提供者或配置的 Connection 目标，记录能力种类及必要版本信息；不要求全部能力同形输入输出。
- **B — 操作请求信封**：稳定调用身份、授权上下文、操作/attempt、目标/资源和幂等或版本约束（适用时）；Task/Conversation 为可选来源关联。
- **C — Dispatcher 交接**：现有 `DispatchTicket` + 资源 lease + 有效授权复检，绝不把 ticket 当执行令牌。
- **D — 结果/证据引用**：仅记录已知事实、UNKNOWN、外部可核对的成果引用；Review/Verify 独立。
- **E — 适配器专用负载**：模型流、文件、进程、Git、外部 Agent 按协议单独定义；不把统一信封变成通用运行时或强制插件 SDK。

正式 Rust 类型/方法、序列化版本和存储迁移 **未冻结**，本稿不可直接用作变更数据库和生产接口的批准书。

### 9.6 本批结论

原先 `TaskBoard`、`DispatchTicket`、`TaskOperationIndex` 和 `OperationLedger` 的职责边界具有可复用价值。主要工作是核实真实桌面调用与持久化交汇点，再少量补足跨边界关联，**不新建第二套任务状态机或全量执行平台**。

本批为静态源码/文档审查；无代码、正式产品基线、存储迁移或 CI 修改。

## 10. 第四批：桌面 Task 与 Core 派工的真实接线核对（2026-10-26）

### 10.1 源码证据与限制

本批读取 main 的下列文件：`crates/wex-core/src/task.rs`、`dispatch.rs`、`dispatch_leases.rs`、`task_operation.rs`、`crates/wex-desktop-service/src/application/mod.rs`、`domain/mod.rs`、`application/task_records.rs`、`apps/desktop/src-tauri/src/main.rs`。目前未通过源码全文索引确定所有跨 crate 调用点，以下只对已读取文件定性。

| 边界 | 当前可证明的事实 | 审查判断 |
| --- | --- | --- |
| Core 任务领域 | `TaskBoard` 有 Queued/Running/Blocked/Completed/Failed、单 Agent 分派版本和可信证据引用 | 可复用的纯规则；不代表桌面业务已接入 |
| Core Dispatcher | `admit`、`reserve`、`validate_ticket` 使用分派票据和进程内 lease，明确不进行最终执行授权 | 不能将这些纯函数直接宣称为持久队列或外部隔离 |
| Core Operation 投影 | `TaskOperationIndex` 接受任务/操作关联，阻止旧分派回执修改当前任务，结果成功只增加证据 | 投影不承担审批、工具执行或事务持久化 |
| Desktop 任务资料 | Desktop Service `Task` 含 title/goal/scope/acceptance/agent_ids 等字段；桌面 `create_task` 初始为 Draft，`update_task` 修改草案业务内容 | 业务任务资料与 Core 任务执行视图目前形态不同 |
| 桌面人工记录 | `TaskRecord` 明示“用户手工记录，不是可信执行/验收回执” | 不得把人工进度当成 Operation 证据 |
| Tauri 命令入口 | `main.rs` 注册了 `append_task_record` 等人工任务操作 | 不能据此推出真实自动派工已接通 |

### 10.2 发现的关键缝隙：两种 Task 形态的权威关系未证明

Core `Task` 是较小的执行状态和分配事实；Desktop Service `Task` 是包含多成员、目标、范围、验收的工作资料。二者**可以通过明确映射共同存在**，不应看到两个同名 Task 就立即合并，也不能保持两份可独立写入的“最终任务真相”。

**本批提出但不实施的最小做法**：维持 Desktop 面向用户的完整任务资料与 Core 的纯状态规则，先定义稳定 task identity、版本/分配代次、单一正式状态写入路径以及从领域资料到执行 ticket 的只读投影。具体单 Agent 执行票据如何对应多成员协作、何处保存进度，仍需接口核对；不在本稿选定数据库归属或修改状态枚举。

### 10.3 不做“全量执行标准化”的交接方案

- **WEX 自己执行**：Task/Dispatcher 可关联 operation/attempt 与本地可信回执；最终权限和文件/进程/Git 执行仍交专门 Host/Adapter。
- **外部 Agent 委托**：WEX 只需记录可观察到的委托、外部 session/job 定位、批次、产物地址、接收情况及独立核验；不强迫外部 Agent 实现 Core 的每个 `OperationState`、过程内部步骤或 Task 结构。
- **只做连接使用或普通聊天**：不要求存在 Task 或 Dispatcher ticket。不要用伪造 Task 填满通用调用字段。
- **多 Agent 协作**：允许 Task 的业务参与成员多于单次执行票据的 Agent；先核对现有分派/会话关联，再决定是否需要 Assignment 或 WorkItem 中间概念。不得将整个系统约束成永久“一个任务只允许一个 Agent”。

### 10.4 暂缓修改、需要重点裁决的三项语义

1. **正式任务事实由谁持有**：建议保留一个权威任务状态写入路径，Core 纯规则可用于校验，不另立并行权威。具体由 Desktop Service 还是其他应用服务持有，须先完成调用者与持久化审计。
2. **操作失败的影响**：现有 `TaskOperationIndex.reconcile` 可把 Running Task 标成 Failed；对持续开发，编译/测试失败常是下一轮修复的输入。应先区分“单操作失败”“任务暂时阻塞”“任务最终失败”，不能未经确认改写既有状态转换。
3. **多成员 Task 与单 Agent 派工票据**：一张任务卡可以分配多个成员，但一次实际派发应具明确执行 Agent 和版本。需要核对是否已存在分工实体，避免把多个成员简化成单一 `assigned_agent`。

**状态**：发现真实结构差异，尚不能断言发生了双写状态冲突。仅记录审查问题，待用户确认实施取舍后再动正式契约、核心代码与持久化。未修改安全边界，不新增全量执行平台要求。

本批无测试/构建/Actions 运行，源码与正式基线均未改。

## 11. 第五批：任务需求、产出与验证的对应关系（用户确认，2026-10-26）

### 11.1 已确认的产品/架构原则

用户确认：**完成流程不必标准化，但任务需求、产出与验证必须对得上**。据此，本节只约束跨环节的可追溯关系和判定依据，不规定必须按某个统一步骤、固定数量的 Agent、工具或验证程序执行。WEX 是跨来源的联通/协调宿主，不是要求接管外部系统内部执行的全量执行平台。

* 工作目标与验收条件应能指向具体成果及其版本。
* 执行成功、产生了成果、成果已检查、目标已验收是不同事实，不得相互冒充。
* 验证证据必须指明其覆盖的成果版本、检查条件/范围和环境（适用时）；成果改动后不得自动继承旧版本验证结论。
* 不要求所有任务都走“开发→编译→测试→人工验收”的固定流程；研究、创作、信息巡检、外部委派等根据明确目标选择不同的验证方式。
* WEX 对外部 Agent 只需保存自身可观察的委派、成果定位、核验引用和结论，不伪造第三方内部步骤或工具日志。
* 某一步失败可作为后续修复工作的输入，不必自动等于整个任务失败；如何调整现有 `TaskStatus` 转换仍需要代码影响审查。

### 11.2 建议的最小关系（逻辑关联，不是强制流水线）

| 关系 | 必须能回答的问题 | 建议限制 |
| --- | --- | --- |
| Task → Requirement/AcceptanceCriterion | 要达到什么结果、按什么条件判断 | 允许不同任务使用文本条件及可选结构化条件；不能要求所有条件都能机器测试 |
| Task → Assignment | 谁负责当前工作、派工对应哪个版本/范围 | 一个任务可有多 Agent/多个分工；一次派工身份必须明确 |
| Task/Assignment → Operation 或 ExternalDelegation | 哪些工作实际发生，已知结果是什么 | 执行记录与 Task 状态分离；外部仅留可观察边界 |
| Task → ArtifactRevision | 产出了什么、位置在哪、当前是哪版 | 文件、提交、报告、章节、数据快照均可；同名地址不等于同一版 |
| Verification → Criterion + ArtifactRevision | 这一检查针对哪个条件、哪个具体产物版本 | 保存检查类型、范围、执行环境与证据引用；结果为 unknown 时不得伪称通过 |
| Acceptance → Task + Criterion coverage + ArtifactRevision | 为何认为需求已达成，是否尚有待验证条件 | 决策依据与来源可追溯；不强制所有任务都由工具自动验收 |

这些概念可由若干引用/记录组合实现，**不预设六个数据库表、六个服务或统一插件协议**。普通对话或一次工具调用也不必创建 Task。

### 11.3 三类事实的最低边界

1. **产出事实**：某个路径、文件、commit、外部交付批次或报告版本已经被观察/记录。存在本身不能证明符合需求。
2. **验证事实**：检查人/工具、检查条件、产物版本、所用环境、证据及明确结果；单元测试通过不替代未进行的 Windows 实机验证。
3. **验收事实**：对该任务目标及其适用条件所作的独立结论，可为已接受、未满足、待核验等语义；具体枚举未定。验收依据不能只依赖模型自述或“operation succeeded”。

### 11.4 不同任务的适配示例（仅示例）

- **软件开发**：需求为“在 Windows 实机可运行”；Linux 单测和格式检查通过可归档为部分验证，真实 Windows 运行未验证，不能宣称目标验收通过。
- **研究报告**：要求来源可核对、覆盖指定议题；引用与报告版本关联，审查可以是人或现有外部能力，不要求创建编译/测试步骤。
- **外部 Agent 委派**：收到指定批次成果；WEX 核对产物归属及既定要求，不要求看到外部 Agent 内部的每条命令。
- **持续巡检**：每轮运行有自己的时间窗、数据快照和检查事实；上一轮通过不等于下一轮通过。

### 11.5 对现有代码的最小接入建议（未执行）

- 暂保留 Core `TaskBoard`、`TaskOperationIndex`、`OperationLedger` 和 Desktop 任务卡；先核对单一正式任务状态入口及双模型映射。
- 优先新增**可追溯的引用关系和版本锚点**，而不是扩大 `TaskStatus` 为几十个混杂的流程枚举。
- `TaskOperationIndex` 对单个失败 Operation 可能直接置 Task 为 Failed：记录为待审查的代码语义差异；先核对现有场景，不能直接改正式状态转换。
- 验证与验收可重用现有工具、第三方能力或人工；WEX 保存证据和结论，不必自己执行每一种检查。
- 验收前要能指出“需求/条件—成果版本—验证结论”的缺口，若条件尚未覆盖，应显式标待验证而非自动完成。

### 11.6 后续可编码契约的收口条件

下一批要核对具体持久化与前端调用者，才能选定字段、数据迁移方式及状态转移。至少验证：

1. 同一任务两个 Agent 产出不同 ArtifactRevision，可分别验证且最终合并检查；
2. 同名文件的新版本不继承旧版本验证；
3. 编译失败可以保留失败证据并继续授权修复，不把单步失败强制解释为目标失败；
4. 外部 Agent 只返回成果地址，WEX 不要求完整内部历史也能独立核验；
5. 用户拒绝验收或实际环境不可测时，保存成果与部分证据，不伪报完成；
6. 任务记录与真实操作证据来源明确分离，撤权和不确定副作用保护仍有效。

**实施状态**：本节原则已获用户确认；表内字段/对象拆分、状态枚举和数据库布局仍属候选。无业务代码或正式接口改动，无测试/Actions。

## 12. 第六批：任务资料、持久化与前端契约审查（2026-10-26）

### 12.1 已核查的实际存储事实

- Desktop Service 的用户任务对象包含 `id/project_id/title/goal/scope/acceptance/agent_ids/status`，其中本阶段桌面任务状态只有 `Draft`；前端 `Task.status` 同样只允许 `draft`。这是任务卡草案视图，而非 Core `TaskBoard` 的五种运行状态。
- `append_task_record` 验证任务存在和 Snapshot revision，追加 `source: User` 的 `progress/blocker/handoff` 文本与引用，并保存 `task_snapshot`、`recorded_revision`。**该版本号属于桌面快照写入序号，不证明引用对象内容的版本**；人工记录不能自动充当真实产出、测试或验收的可信回执。
- `apps/desktop/src-tauri/src/main.rs` 提供 `create_task/update_task/append_task_record` 等命令。就这些入口可证实的是任务草案和人工记录编辑，不是自动派工闭环。
- `DesktopStore` 当前用 SQLite `desktop_state` 单行 JSON 保存 DesktopSnapshot；`PRAGMA user_version` 为 **6**。高于 6 拒绝读取；1–5 向 6 迁移时先反序列化校验、只更新版本并保留原 payload。不能把此机制误写成已支持未来结构的无损自动升级。
- 现有 `Snapshot.revision` 提供并发修改时的版本检查，却不是任务验收条件、成果文件或测试环境的版本标识。后续至少须区分这几类版本。

### 12.2 最小关联契约建议：不固定工作流程

建议先用**引用关系 + 版本身份 + 来源类别**收口，不强制新增独立的任务引擎/全部流程节点。

| 逻辑记录 | 必须可核对的最小内容 | 不承担 |
| --- | --- | --- |
| RequirementRef | 所属 task、需求/验收条件版本、条件内容或受控引用 | 预设统一检查方法 |
| ArtifactRef | 所属 task、产物身份、具体版本/内容摘要/交付批次、存放位置、来源 | 只凭产物存在即宣称验证通过 |
| VerificationRef | 被检查的 RequirementRef 与 ArtifactRef 版本、检查方式/环境、证据、已知结果 | 以验证成功覆盖未检查的其他条件 |
| AcceptanceRecord | 被判定的 task 目标版本、已覆盖/未覆盖条件、结论及判定者/依据 | 把操作结果或模型自述直接变成正式验收 |
| WorkLink | task 或 assignment 与 operation/外部委派的事实性关联 | 按关联本身赋予执行权限 |

**数据模型粒度暂不冻结**：这些可编码为若干结构化引用及记录，不要求五个 SQLite 表、五套生命周期或统一外部 SDK。允许人工、工具或外部来源的验证，但必须保留来源/证据类型，不把用户草稿笔记提升为系统可信执行回执。

### 12.3 一条可检验的端到端示例

需求 R1：“功能在 Windows 本机可正常启动”；需求版本 `r1-v2`。
产出 A1：“代码提交 X”，内容版本 `commit-X`。
验证 V1：Linux 上的 Cargo 单测通过，绑定 `r1-v2 + commit-X`，覆盖部分技术检查，但**不证明 Windows 启动**。
验证 V2：Windows 实机启动尚未执行，明确记录为未验证。
验收：在约定要求包括 Windows 启动的情况下不得因为 V1 成功而宣布 R1 完成。
如下一次修改产生 `commit-Y`，对 `commit-X` 的验证仍保留为历史事实，不能默认转用于 `commit-Y`。

这只是版本、检查范围和结论的关系示例；研究、小说、巡检、外部委派分别采用适合自己目标的方式，不强制“编译/测试”环节。

### 12.4 当前代码迁移的最低安全门槛

1. **先定义单一 Task 权威写入路径**，将 Desktop 草案和 Core 运行规则通过可检查的映射连接，不直接将双方 `TaskStatus` 拼接成一个枚举；待核对其余调用方。
2. **优先做只读映射/并行演示**，验证旧任务卡、旧 `TaskRecord` 的身份与内容仍可读；不能通过虚拟执行记录填补旧数据。
3. 若改变持久 JSON 字段与含义，必须明确 `user_version > 6` 的新版本、写入事务、失败回滚与旧版本拒绝行为；不以随手加 `serde(default)` 代替正式迁移。
4. 前端 `Task` 类型、Tauri 命令、DesktopService 和测试要同步改变。现有 `TaskRecord.source = User` 不应在没有依据时改写为 `Verified`。
5. 对工具回执存储和桌面 Snapshot 的跨边界一致性进行专项故障测试；不能因为 SQLite 事务存在就假定文件系统证据和 Task 状态天然原子提交。
6. 内置工具及外部执行的权限与取消规则原样保留；本工作稿不得直接授予任何能力、改变授权继承或自动补全不明产物版本。

### 12.5 需后续裁决的实现取舍（本批不改）

**D1 — Task 权威持有者及状态写入路径**：建议由可信应用服务统一协调，Core 继续提供纯规则；须先完整核对 Desktop 草案与 Core TaskBoard 的调用关系，不能定数据库或拆模型方案。

**D2 — Verification/Acceptance 的最小存储形式**：建议先采用支持证据来源、产物版本、检查范围的结构化记录，用户可自定义检查类型；是否单独持久化对象、何时建立索引待评估。

**D3 — 失败后的任务推进语义**：一次 Operation 失败可能是任务修复循环的正常输入，须与真正无法继续或验收失败区分。现有 `reconcile` 直接置 Task `Failed` 的逻辑未改，待产品语义与调用链一起审定。

这三项目前不属于用户已明确批准的数据库/实现规则。先维持现有代码和文档基线。

### 12.6 本批审查状态

已核对 Desktop Task/TaskRecord、Tauri 前端类型及命令、DesktopSnapshot/SQLite schema 6 的静态实现。未执行真实 Windows 验证、迁移测试、Rust/TS 测试或 Actions；未修改 main、源代码或正式接口。

## 13. 第七批：APS → WEX → WEXPC 历史设计回收与差异矩阵（2026-10-26）

本批依据旧 WEX 仓库 **main** 的实际文档，而非重新假定用户尚未作出决策：

- [WEX 顶层基线（2026-10-17）](https://github.com/owla19s-ux/WEX/blob/main/docs/WEX-DESIGN-BASELINE-2026-10-17.md)，尤其 §1.2、§1.3、§2、§7。
- [主 AI、施工/审查 AI 与 Dispatcher 协作架构（2026-10-18）](https://github.com/owla19s-ux/WEX/blob/main/docs/architecture/CONTEXT-LEAD-AI-DISPATCHER-BASELINE-2026-10-18.md)，尤其 §1–6。
- [最小交接/审查/唤醒协议草案 V0.1](https://github.com/owla19s-ux/WEX/blob/main/docs/architecture/CONTEXT-AI-DISPATCHER-MINIMAL-HANDOFF-PROTOCOL-DRAFT-V0.1.md)，尤其 §2–4、§10–12；**其字段和状态枚举仍是旧候选，不视为已批准实现**。
- [PTAL 架构审查](https://github.com/owla19s-ux/WEX/blob/main/docs/architecture/PLATFORM-TOOL-ACCESS-LAYER.md)；接入与运行机制是候选，不继承旧 Android 技术限制。

### 13.1 已存在的历史设计，不再重复当作新决策

| 设计原则 | 历史依据 | 在 WEXPC 的继承方式 |
| --- | --- | --- |
| WEX 联通实际工作，不要求每件工作都有固定 Task 或固定软件开发流程 | 顶层基线 §1、§1.3、§7 | 任务关联按场景可选；不建设全量执行平台 |
| Execution / Result / Evidence / Verification 分离 | 顶层基线 §2；协作架构 §2 | 区分已执行、已有产物、已有证据和已通过的真实验证 |
| 验证结果可统一记录，验证方式不强制统一 | 顶层基线 §1.3 | 适配开发、研究、创作、外部委派各自验收条件 |
| 提交完成、审查通过、Verify 通过、真机验收通过各不相等 | 协作架构 §2 | 产物、审查、技术验证和最终验收分别保留依据 |
| Dispatcher 非 AI，仅机械性交接、记录和恢复 | 协作架构 §1、§5 | 不用 Dispatcher 替代主 AI 的判断，也不管理外部 Agent 内部执行 |
| 施工 AI 报告不是最终成功，审查/验证需有可核对证据 | 协作架构 §1–2；PTAL §5 | 操作或工具成功不自动关闭 Task |
| 施工卡/成果版本与失败历史必须可追溯 | 协作架构 §3；交接协议草案 §4、§10 | 新结果不覆盖旧失败；旧 Commit 的验证不继承给新 Commit |
| 多人/多 AI 的概念不能被单 Agent 首期样例写死 | 协作架构 §6.2；交接协议草案 §11 | 多 Agent 分工允许多实体；单个派发票据仍只关联明确执行者 |

**精确状态说明**：旧基线已经确认“职责分离、证据、审查/验证边界”；旧交接协议中的具体事件名、字段、细分状态仍是研究草案。不能引用旧草案就宣布具体数据格式已经冻结。

### 13.2 旧版本排期不得反向覆盖 PC 当前目标

旧 Android WEX 的 2026-10-20 冻结范围曾将完整主/施工/审查 AI 工作模式推迟到后续；这属于**当时 Android 版本的实施排期**，不撤销其已确认的长期架构原则。当前 WEXPC 0.1.1 产品基线已明确要求轻项目管理、Dispatcher、多 Agent 施工、Review/Verify 和中断接续。后续施工应以 **WEXPC PC 基线 §5** 的版本目标为准，不能据历史 Android 排期将这些要求删去或后置。

旧技术实现（APS Kotlin、Android Flutter 运行时和安卓工具权限）不作为 PC 必须迁移的源码/平台模型。继承的是语义、已验证规则和行为要求。

### 13.3 真正需要继续设计、而非重新讨论的工程缺口

1. **业务 Task 与 Core TaskBoard 的状态权威映射**：桌面仍存 `Draft` 任务，Core 具备运行状态。要确定统一写入口、任务版本与多成员派发身份。
2. **需求—成果版本—验证—验收的最小持久关联**：早期资料讲清了关系和信任层级，但没有为 PC 冻结具体可编码结构、前端协议和迁移版本。
3. **部分操作失败后的继续修复**：历史已明确正常修复可继续；现有 `TaskOperationIndex` 可能在单操作失败时置 Task `Failed`，须细化映射策略及调用链，不能让某次测试失败被认为用户任务终结。
4. **真实 Dispatcher 到 Host 的派发接线**：Core 含 admission/lease/链接纯规则；尚需证明桌面业务任务、执行回执、审查和恢复都已接通。
5. **跨系统交接的事实格式**：原生内部操作与外部 Agent 不同深度；只统一 WEX 可核对的引用、接收、成果和验证，绝不要求外部强行使用 WEX Operation 内部状态机。
6. **持久化安全**：SQLite DesktopSnapshot 当前 user_version=6，人工 TaskRecord 不是可信回执；应在明确归属后设计增量兼容与崩溃一致性，不能擅自改 schema。

### 13.4 后续审查执行原则

今后每项条目先标注为 **历史已确认 / PC 新确认 / 历史候选 / 代码事实 / 真正待裁决**。先利用现有历史设计与 Core 逻辑，再提出最小接口。只有发现新冲突、扩大权限、改变业务验收决策或不可逆迁移，才提交用户选择；未发现则在独立文档分支继续审查。未经确认不更改正式基线、Task 状态机或数据库。

本批为旧文档和当前 main 静态对照，未改业务代码、正式文档或 CI；未宣称 Windows 真实验证成功。

## 14. 第八批：最小状态权威与失败接续的代码核对（2026-10-27）

### 14.1 核对范围和结论限度

再次读取 main：`wex-desktop-service/src/application/mod.rs`、`domain/mod.rs`、`wex-core/src/dispatch.rs`、`task_operation.rs`、`apps/desktop/src-tauri/src/main.rs`。当前被核对的桌面入口只包括创建/更新草稿 Task、追加用户 TaskRecord；在这些文件中未发现桌面业务调用 `TaskBoard`、`DispatchTicket`、`TaskOperationIndex` 或 `ResourceLeases` 的路径。**这是所审文件范围内的观察，不等于对仓库所有模块完成了引用搜索，也不宣称所有能力均未接线。**

- 桌面 `Task.status` 目前只有 Draft，普通用户编辑行为由 `DesktopService` 修改 `DesktopSnapshot`。
- Core `TaskBoard` 提供独立的执行相关状态和规则；`TaskOperationIndex` 投影来自 `OperationLedger` 的结果，但未在上述桌面入口看到这条路径的真实启用。
- `dispatch.rs` 的纯准入检查与资源 lease 仍需要 Host 的持久化、最终权限检查与实际派发。
- 现阶段最明确的缺口不是“没有状态类”，而是**缺乏已经证实的从用户任务卡→明确分工→可执行尝试→成果版本→验证/验收→任务展示的端到端桥接**。

### 14.2 在不新建第三套任务真相前提下的落地顺序（建议）

1. **冻结任务身份及需求版本引用**：旧 Desktop Task ID 保持不变，Goal/Scope/Acceptance 变更应有可核对的旧版与新版；已有 TaskRecord 原样保留为人工事实。
2. **薄桥接层**：把已发布/已确认的业务任务投影成 Core 可处理的单次执行单元；一次执行者/分派代次明确，但业务任务可保留多 Agent。桥接层不拥有第二个业务任务状态。
3. **执行与成果关联**：记录 Task、分派、Operation 或外部委托、ArtifactRevision 的关联；普通操作不要求 Task，外部成果不要求完整 WEX 内部操作追踪。
4. **验证和验收**：只记录实际对某个要求版本和成果版本进行了哪些核验，未验证明确保留；最终任务状态更新由可信任务写入口作出，不能因模型文字或一个成功回执直接完成。
5. **故障演练和兼容**：旧草稿可读、迟到回执不污染新分派、断电/中断不凭空产生成功、修改成果后旧验证不自动继承；再进入正式 schema 和 UI 迁移。

上述为审查实施建议，不冻结具体类、表或 crate，也不要求所有任务都采用相同步骤。

### 14.3 单次 Operation 失败与 Task 状态（已有原则对照）

历史已确认：执行失败、产出、审查、验证和最终任务状态互不等价；允许在已批准任务范围内继续普通修复。

**当前源码差异**：`TaskOperationIndex.reconcile` 对 `OperationState::Failed` 在 Running Task 上直接 `transition(Failed)`，而 `TaskBoard` 的 Failed 只允许通过重新排队后才能再次进入正常执行。对“编译失败→AI 自动读报错→修复→重新运行”的常规场景，这种逐条操作失败直接终结任务的规则存在潜在语义不匹配。

**建议的最小修正方向（未改代码）**：Operation Failed 永久保存为该次尝试的真实事实；是否继续工作由当前任务目标、已确认范围、预算、实际安全条件及调度/主 AI 决策决定。只有任务层的退出/无法继续条件成立时才进入正式 Failed；授权缺失、未知副作用优先表现为 Blocked/待核实。具体是否改 `reconcile` 为“仅写证据及派生阻碍”，须完整核实其调用者与测试后实施。

### 14.4 特别保护：不可绝对标准化

- 不要求小说章节、研究报告、外部委派都走代码构建/自动测试环节。
- 不要求第三方 Agent 使用 WEX 的 TaskBoard、OperationLedger 或执行沙箱；仅对 WEX 可观察的投递/产物/验证事实建立关联。
- 不要求每个 Capability 必有 Connection，也不要求每次调用必有 Task 或 Conversation。
- 不把“可信任务状态写入口”理解成接管所有外部执行、服务或 AI 自主过程；其职责仅在 WEX 自有任务账本内。
- 保留可人工验收与“尚未验证”结论，不以通用工作流模板代替用户对目标的定义。

### 14.5 需继续追踪的具体工程证据

下一批应扩展到桌面任务 UI、操作回执文件和 TaskBoard/Dispatcher 是否经其他 service/Host 路径调用，结合测试和数据迁移方案制定准确接口变更表。完成全仓引用检查前，不应断言整个 Dispatcher 闭环完全不存在。

本批没有修改正式基线、业务代码、TaskStatus、SQLite 或工作流，未进行编译/Actions；只更新审查工作稿。

## 15. 第九批：桌面界面、工具 marker/结果与最小接口变更表（2026-10-27）

### 15.1 实际界面与记录现状

本批核查 main 的 `apps/desktop/src/App.tsx`、`Projects.tsx`、`apps/desktop/src-tauri/src/main.rs`、`crates/wex-desktop-service/src/tool_journal.rs`、`tool_results.rs`。这些是源码静态观察，不是桌面实机验收或全仓调用索引。

- 项目 UI 在 `Projects.tsx` 显示任务卡、目标、范围、验收条件、人员和人工 TaskJournal，明确提示“施工、实际验证、审查与修复流转尚未接通”。因此用户可编辑任务资料，**不能把该界面当作已交付执行闭环**。
- `App.tsx` 通过项目页挂接上述组件；当前 `Snapshot` 展示 Task/TaskRecord，不含可查询的版本化 Artifact/Verification/Acceptance 关联。
- Tauri `main.rs` 有 `tool_receipts(conversation_id)` 和 `tool_results(conversation_id,turn_id)`，读的是按对话维度的工具证据，并没有从这两个命令直接把证据归并成任务验收。
- `ToolReceiptMarker` 保存 operation、attempt、state、内容摘要及可选 conversation/turn/policy。marker 是单次执行的不可变状态记录；其读取按 conversation 筛选，**未绑定 Task 或成果版本**。遗留不带 conversation 的 marker 不被猜测归属到某一会话。
- `SavedToolResult` 保存 conversation/turn/call/tool/参数/结果；`sequence` 仅是**同一次对话回合的发布顺序**，不是跨任务全局时钟，也不是 Artifact 内容版本或 Verification 编号。
- marker 和 tool result 是两个独立的文件记录区域。不能假定它们和 SQLite 的 DesktopSnapshot 构成天然原子事务；后续任务引用须有崩溃恢复与证据存在性检查。

### 15.2 最小接口变更表（面向施工者的审查提案，不是已获准修改的源码清单）

| 现有入口 | 建议处理 | 新增或保持的最小语义 | 阶段 |
| --- | --- | --- | --- |
| Desktop `Task` / `TaskRecord` | **保留**用户草案和人工日志，不抹除历史 | 稳定 task_id；区分要求版本与 DesktopSnapshot.revision；人工记录保持 `source=User` | 先审查映射 |
| `TaskBoard` / `DispatchTicket` | **复用**既有纯规则；接可信任务写入口 | task ID、assignment_epoch、单次派工主体和权威运行状态；多成员任务不降级为永远单 Agent | 先接只读投影 |
| `OperationLedger` / `ToolReceiptMarker` | **复用**既有安全和不确定性保护 | 操作/尝试关联、实际可观察结果及 marker 的原始身份；不强制重写旧证据 | 优先接引用 |
| `SavedToolResult` | **保留**对话工具结果存储 | 由受控索引将既有记录关联任务/成果，不能从纯文本内容直接推定“已验证” | 增量 |
| Capability / Connection | **扩展**来源与能力绑定，支持内置和配置连接 | 实际目标、能力身份、配置/版本引用；不预设统一运行时 | 先契约 |
| 新增成果关联 | **最小新记录或引用**，形式待定 | task + requirement version + artifact identity/version + source | 待迁移方案 |
| 新增核验关联 | **最小新记录或引用**，形式待定 | 检查条件、artifact version、验证手段/环境、证据、结果 | 待迁移方案 |
| 新增验收记录 | **可追溯结论**，形式待定 | task 要求版本、已满足和待验证条件、决定来源；工具成功不自动验收 | 待迁移方案 |
| UI `Projects`/TaskJournal | **保留基础交互，按证据逐步展示** | 明确区分人工进度、真实执行、成果、验证和验收，未知则显示未验证 | 接线后实现 |
| 外部 Agent Adapter | **仅补跨边界事实引用** | 委托、外部 session/job、交付批次、成果及可观察核验；不追逐完整内部操作 | 按需实现 |

### 15.3 不应立即施工的改动

1. 不能将 `ToolReceiptMarker` 简单改成强制 task_id：普通对话工具可以没有 Task，已有历史 marker 不应被编造归属。
2. 不把 `SavedToolResult.sequence` 当作任务级有序执行编号，更不能拿它证明某版成果通过测试。
3. 不删除 Desktop 的 `Draft` 任务，也不直接用 Core 的 `Queued/Running/Failed` 覆盖现有 UI 字段。
4. 不因为“统一执行入口”而强制 DSH/Hermes 内部采用 WEX OperationLedger 或做逐命令上报。
5. 不在未完成旧 JSON、marker、Tauri TS 与 SQLite 迁移核对前直接冻结新增数据库字段。

### 15.4 当前可收敛与真正待定

**可收敛为契约原则**：Task 身份和需求版本必须稳定；工具回执和人工日志分离；成果版本与检查条件可追溯；原生与外部交接深度不同；执行失败不直接代表任务终结。

**需要真实接口设计或实施前核对**：Task 状态的单一权威服务及它与 Core/桌面草案的映射；新记录的具体格式和事务策略；多 Agent 分工的最小实体；失败状态映射；外部成果版本取得方式。均不因本次表格直接得到修改生产代码的授权。

后续应以一个真实 WEXPC 开发任务为样例逐项走通：发布任务 → 两人不重叠派工 → 文件/测试证据 → 成果版本 → 独立核验 → 任务视图与重启恢复。真实 Windows 验证必须明确 NOT_RUN 时不得宣称通过。

本批无代码、数据库、正式规范或 Actions 修改；仅审查文档提交。

## 16. 第十批：最小可编码数据契约 V0（审查提案，2026-10-27）

> **范围**：为已经确认的“任务要求 ↔ 实际产出 ↔ 对应验证 ↔ 验收依据”提供能写代码、能做测试的最小协议草案。该方案不是全量执行平台、不是所有 Task 的固定阶段，也不改变 Rust/SQLite/前端的正式接口。字段名只是建议，不视为已冻结 ABI。

### 16.1 与既有领域对象的边界

- `Desktop Task` 继续承担用户看到的目标、修改范围、验收条件、参与成员；旧 task ID 和人工 TaskRecord 不改变语义。
- `Core TaskBoard` 继续提供分派相关状态规则，但不把它变成第二个独立持久化的正式任务真相。
- `OperationLedger` 继续管理 WEX 自己执行的操作及结果；外部 Agent 的全部内部步骤不强制映射成 Operation。
- `Dispatcher` 只管可控制的派工/交接/资源占用，权限与外部运行时仍由相应 Host/Adapter 负责。
- 结构化关联记录是 **Task 工作事实的索引**，不是执行器或通用工作流节点。

### 16.2 语义字段候选（允许以单一表、JSON 记录或其他受控存储实现）

| 概念 | 最小字段候选 | 校验不变量 |
| --- | --- | --- |
| `RequirementSnapshot` | `task_id`, `requirement_revision`, `goal`, `scope`, `acceptance_criteria`, `captured_at` | 已发布要求版本不可被静默覆盖；新编辑生成新版本/引用；旧 Task 可以仅有历史原文而不伪造发布审批 |
| `WorkReference` | `task_id`, `assignment_ref?`, `operation_id?`, `external_job_ref?`, `source_kind` | 原生操作和外部委派均可只存 WEX 可观察边界；无任务的对话工具不强制此记录 |
| `ArtifactRevision` | `artifact_id`, `task_id`, `requirement_revision?`, `locator`, `version_kind`, `version_value`, `produced_by?`, `observed_at` | 仅 locator/文件名不构成版本；不可确认版本时记录 unknown，不继承已通过验证 |
| `VerificationRecord` | `verification_id`, `task_id`, `requirement_revision`, `artifact_revisions[]`, `criterion_refs[]`, `method`, `environment?`, `result`, `evidence_refs[]`, `verified_by` | 必须明确覆盖的要求与成果版本；实际未运行不能标 pass；审核意见与可信测试结果保留来源区别 |
| `AcceptanceRecord` | `acceptance_id`, `task_id`, `requirement_revision`, `artifact_revisions[]`, `covered_criteria[]`, `outstanding_criteria[]`, `decision`, `decided_by`, `evidence_refs[]` | 完成结论有明确条件与依据；仅一个 operation succeeded 不足以自动验收；人工确认不伪装成机器验证 |

这里 `?` 表示字段可选，不代表“无证据也能写成功”。时间信息只辅助排序，不替代内容版本或可信外部 ID。`version_kind` 可为 commit hash、内容摘要、外部修订号、不可变交付批次等受支持来源；不是一律要求所有成果 SHA256。

### 16.3 针对已确认产品规则的三个操作示例

**开发任务**：代码版本 `commit-A` + Linux 单测 pass；Windows 实机为 unknown/not_run。验收条件包含 Windows 实机可运行时，任务不得显示“已全部验收”。新代码 `commit-B` 不自动继承 A 的单测。

**研究任务**：报告 `rev-2` + 资料来源核查记录；是否满足全部研究范围由适用审查或人工判定。无需自动编译、运行环境、统一 Agent 工具日志。

**外部委托**：外部系统返回 job ID 与交付件 revision；WEX 记录可核验的交付及条件对应；若只返回临时链接/缺少版本，标版本未知与待核实，不编造第三方内部命令。

### 16.4 最小接口边界（伪签名，仅表示请求与责任）

```text
TaskApplication:
  publish_requirement(task_id, expected_revision, terms) -> requirement_ref
  record_work_link(task_id?, dispatch_ref?, operation_or_external_ref, observed_source) -> work_ref
  record_artifact(task_id, artifact_identity, version_evidence, provenance) -> artifact_ref
  record_verification(task_id, requirement_ref, artifact_refs, checked_criteria, method, outcome, evidence) -> verification_ref
  propose_acceptance(task_id, requirement_ref, considered_artifacts, criterion_coverage, rationale) -> proposal
  commit_acceptance(proposal, authorized_decision) -> acceptance_ref

Dispatcher:
  admit_task(assignment_and_scope) -> ticket_or_refusal
  handoff(ticket, target_capability_or_external_job) -> observable_handoff
  on_receipt(reference, observed_result) -> event  // no AI judgment or automatic final acceptance

NativeExecutionHost:
  authorize_and_execute(intent) -> operation/attempt/evidence   // retains current security/recovery controls

ExternalAdapter:
  delegate_or_query(supported_request) -> observable_reference_or_unknown  // no universal internal runtime contract
```

**注意**：以上不是“必须照这个方法名创建新类”。一个已有应用服务可以暴露若干受控操作来实现，不需要造新的通用 Manager、Engine 或额外后台常驻模块。工作链接不是新增授权来源，所有写入需保持来源身份与事务校验。

### 16.5 对失败与未验证的最小状态语义

- `OperationFailed`：某次实际执行失败，错误与证据永久保留；默认不等同任务最终失败。
- `Blocked`：不能在现有权限/环境/证据范围内继续，或副作用结果不明，需要核对；禁止对不确定写入盲重试。
- `NeedsRepair`：是可在任务视图/调度建议中表达的下一步工作类型，**不要求立即新增 Core TaskStatus 枚举**；必须有批准的工作范围和实际执行权限才能执行修复。
- `TaskEndedUnfulfilled`：任务层依用户/明确规则决定停止但目标未达成；具体映射现有 Failed/其他终态待实现审查。
- `AcceptancePending`：已有成果但条件未覆盖或尚未取得有效验收依据；可独立于派工状态存在，不把它强塞到每个执行状态。

### 16.6 兼容与落地门禁

1. **第一步只读评估**：旧 `DesktopSnapshot` 的 Task/TaskRecord 仍能读取；原始人工记录不得自动晋升为 Verification。
2. **稳定身份**：维持原 task ID、operation/attempt ID 与对话工具回执身份；新增引用建立显式关联，不根据相似名称猜测归属。
3. **版本不可混用**：DesktopSnapshot 全局 revision ≠ requirement_revision ≠ artifact_revision ≠ assignment_epoch；需要在接口上防止误用。
4. **事务与恢复**：SQLite 任务记录与磁盘工具 marker 分属不同介质，必须验证掉电/失败后的引用完整性及结果 unknown 的保守显示。
5. **权限与多成员**：发布、记录成果、验证、正式验收各自按实际已授权身份写入；Task 归属不自动授予外部资源权限；业务多成员不强制降为单 Agent。
6. **外部兼容**：不强迫外部 Agent、插件或 MCP 适配器支持内部 Task、状态枚举、progress/stream/cancel/resume 全套能力；只要求声明真实可用能力和可观察回执。
7. **测试用例**：至少覆盖旧数据读取、同路径新版不继承旧验证、同卡多 Agent、验证失败后继续修复、缺少 Windows 实机验证时不能最终完成、外部只交付文件、迟到回执、未知副作用不得重试。

### 16.7 决策状态

本节的**事实分离、需求/产出/验证对应、灵活流程、外部边界**是用户已确认和旧设计可继承的原则。**具体字段、持久化对象个数、方法签名、权限角色和自动验收条件**为本次新增的实现提案，未获冻结批准；施工前应对照实际调用方进行最小化裁剪。

本批仅文档审查，无业务代码、正式接口、SQLite schema、Actions 或 Windows 实机测试变化。

## 17. 第十一批：最小接口裁剪及版本兼容门禁（2026-10-27）

### 17.1 对实际可写路径的复核

本批核对 main：
- `DesktopService.create_task`：校验项目/成员职责，创建仅有 `Draft` 的用户任务卡；原始 Task ID 在此分配。
- `DesktopService.update_task`：校验 `DesktopSnapshot.revision`，保留项目归属、修改标题/目标/范围/验收文本及成员；**当前是覆盖式编辑，没有单独的任务要求历史版本**。
- `DesktopStore.update`：以 SQLite Immediate 事务加载单行 JSON，执行一次变更并递增全局 revision，再完整写回；这能保护同一次 DesktopSnapshot 写入，但**不能保证磁盘工具 marker、外部代码文件和远端交付同时原子提交**。
- `Core TaskBoard.assign`：维护单执行者及 assignment_epoch；其执行单元不应被混同于 Desktop `Task.agent_ids`（可多成员）。
- 旧桌面 `Task.status` 只有 Draft，不能直接映射为 Core 的 Queued/Running；未发布任务应保留其真实草案身份。

### 17.2 对第十批 V0 契约的裁剪

**不建议新建五套 manager、六阶段工作流、统一外部运行时。** 第一阶段仅需在可信 Desktop 应用服务中可持久化三类独立工作事实；它们通过 task、要求版本和成果版本形成关联：

| 最小记录 | 核心含义 | 建议的最少关联信息 |
| --- | --- | --- |
| `TaskRequirementRevision` | 曾经批准/发布了什么任务要求 | task_id、修订号、目标/验收原文、来源/发布时间（存在时） |
| `TaskArtifactRecord` | 任务产生/收到了什么版本的东西 | task_id、artifact identity、version evidence 或 unknown、来源与定位 |
| `TaskCheckRecord` | 对哪个具体要求、成果版本做了什么检查或验收 | task_id、requirement revision、artifact refs、检查条件及范围、检查/验收类型、结论、证据及身份 |

`TaskCheckRecord` 可以以区分 kind 的方式容纳审查、工具验证、人工验收，**但不能把三者的信任层级合并**；若实现中需要分表，可后续再拆。普通 AI 对话工具依旧只产生其自身 ToolReceipt/SavedToolResult，不强行注册 TaskArtifact。

`WorkReference` 先作为可选的现有 operation/attempt 或外部委托引用字段使用；暂不优先建设全局工作流事件引擎。只在有真实任务分工时生成执行关联。

### 17.3 推荐的单一写入口原则

- **业务状态的权威写入口**：由现有 `DesktopService` 这一可信应用服务承担 Task 及新关联事实的受控持久写入；底层 SQLite `DesktopStore.update` 继续处理单快照事务。**这是面向现有源码的实施建议，不预设它必须独占所有未来运行时存储。**
- **Core TaskBoard**：保留纯规则用于合法状态转换、派工准入与断线恢复；状态变更从可信应用服务受控提交后才对 UI 发布，不建立第二个可独立修改的持久 TaskBoard 快照。
- **Dispatcher**：只申请派工/提交真实投递与观察事件，不能直接改需求、声称最终验收或越权写原始 SQLite。
- **AI/第三方**：提出成果与检查声明，来源必须明确；系统读取/核实外部可验证事实后才能写可信结果。用户人工决定允许存在，但不可伪造成已运行的自动测试。

### 17.4 迁移与兼容最小方案（审查提案）

1. 先定义新任务记录的 schema version（必须大于当前 6），同时保持 `Task` 的旧字段、已有 `TaskRecord` 与旧 ToolReceiptMarker 不变；不要将未发布旧草案自动升级为已批准施工卡。
2. 旧 `Task.goal/scope/acceptance` 可以作为**历史可见原文**读取，但没有可核实的发布版本时不能制造“以前已经批准发布”的证据。
3. 采用只增加可选新字段/关联集合的方式准备过渡；首次升级前先检查旧负载能被新程序解析并保留，失败则回滚，不覆盖原始 JSON。
4. 明确要求版本与全局 snapshot revision 相互独立；修改已发布需求时生成新 requirement revision，旧成果/旧验证继续指向旧版，不能隐式转移。
5. 保存新记录时校验 task、requirement、artifact 引用存在且版本匹配；磁盘或远端证据引用需要允许暂不可访问并明确 `unknown`，不得自动宣称核验通过。
6. 对新旧程序的打开行为与所有权保护做迁移测试，保证版本超前时拒绝旧版本写入；新增前端类型、Tauri 命令和读取视图只能在实现批次同步落地。
7. 只在验证真实用例后允许自动验收逻辑；早期可由已授权用户/明确策略作出最终验收，不把 AI 文本自报当可信证明。

### 17.5 建议先写的契约测试，而不是先扩大量接口

- 旧 schema6 快照中的任务草案、人工日志、Agent 绑定和对话都能无损读取与升级；失败时原记录仍在。
- 创建两个需求版本，V1 产生的成果/测试只对应 V1；V2 编辑后不能继承 V1 的验收。
- 两个 Agent 在一个 Task 的不同工作范围协作，分别关联产物；不会因为第二个 Agent 施工而冲掉第一个 Agent 的记录。
- 工具失败可被保存并继续修复；任务是否结束独立决定。
- 纯普通对话产生 ToolReceipt，不必生成 Task/Artifact/Verification。
- 外部 Agent 只有委托号/产物地址、没有内部命令日志，依旧能存可核验成果；暂不可核实时明确 unknown。
- Windows 实机检查未进行：Linux 测试即使通过也不能把 Windows 验收标记通过。
- 中断或 marker/result/SQLite 写入跨界失败：不制造成功，也不自动重试有副作用操作。

### 17.6 待确认风险与范围提醒

**重要提醒**：提出“DesktopService 作为统一 Task 权威写入口”是基于现有主线架构的具体实现建议；正式冻结前还需检查应用服务调用/持久化冲突、多 Agent 事务和现有测试，且不得把这一局部写入口扩展为 WEX 对全部外部能力的全量管理。

同理，“三类记录”是降低首版实现成本的结构候选，不代表三种验证类型语义完全等价。若出现强制单流程、强制外部 Agent 的 WEX 内部协议或权限自动继承，均视为偏离本轮已确认架构。

本批仅提交审查文档，无 main、Rust/TS、SQLite、Actions 或 Windows 真实验收改动。
