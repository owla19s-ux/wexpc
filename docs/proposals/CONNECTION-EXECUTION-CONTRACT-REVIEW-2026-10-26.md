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
