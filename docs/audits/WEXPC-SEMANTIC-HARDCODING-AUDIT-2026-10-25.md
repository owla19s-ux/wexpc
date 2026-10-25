# WEXPC 语义硬编码专项审查（2026-10-25）

> 依据：本分支现有 `crates/wex-desktop-service`、`crates/wex-core`、`apps/desktop`、Tauri Host 的代码；对照 `docs/PC-PRODUCT-AND-TECH-BASELINE.md` §2.7。**静态源代码审查，不等于全仓扫描、编译、Windows 运行或已完成整改。** P0/P1 表示应优先处理的产品建模风险，不等同于已发生安全漏洞。

## 已确认的语义绑定

| ID | 优先 | 源码位置 | 实际约束 | 与已确认语义的冲突 | 整改建议 |
| --- | --- | --- | --- | --- | --- |
| S01 | P0 | `crates/wex-desktop-service/src/domain/mod.rs` `Agent`/`NewAgent` | 成员必须有一个 `project_id: String` | 用户级秘书、跨项目成员或无项目成员无法使用同一身份 | 将成员身份与 Context/Project 职责**分离为关联关系**；兼容旧记录 |
| S02 | P0 | 同文件 `AgentRole` / `Agent.role` 及前端 `apps/desktop/src/types.ts` | 单角色且只能是 `Lead/Worker/Verifier/Reviewer` | 秘书可以兼任项目主 AI；未来职责不固定为四项 | 成员可以持有多项职责分配；模板可扩展；不把角色本身当成授权 |
| S03 | P0 | `application/mod.rs` `add_agent()` | 创建成员时强制检查项目，同时创建一个项目下专属对话，并固定 connection/model | 成员、职责、会话、模型、运行时在创建时被绑成一体 | 将身份、模型选择、职责分配、Session 生命周期分离；预留旧 API 适配 |
| S04 | P0 | `application/mod.rs` `create_task()` | `agent_ids` 必须对应本项目下 `Agent.project_id` | 同一外部/跨项目成员不能按有效授权承担任务 | 检查当前项目的有效职责分配与授权，而不是成员的唯一归属 |
| S05 | P0 | `application/member_brief.rs` | `AgentRole::Lead` 直接决定项目级工作资料可见集合，其他角色仅自己/指派任务 | 将默认职责映射成事实上的数据投影访问规则；尚未依据真实授权作细粒度过滤 | 引入独立可审计的 Context/资源/字段可见范围；保留审查者适用的广泛只读 |
| S06 | P0 | `application/mod.rs` `prepare_chat()` | `match agent.role` 生成四类固定 system 职责文案 | 秘书复任及动态岗位无法表达；提示词不等于权限 | 由受控职责配置合成提示/工具可见性；真实授权必须在服务端执行 |
| S07 | P0 | `domain/mod.rs` `TaskStatus` 与 `crates/wex-core/src/task.rs` | 桌面仅 `Draft`，Core 已有 `Queued/Running/Blocked/Completed/Failed` | 同一产品出现割裂的任务语义，无法持续调度 | 先确定唯一权威 Task 状态与迁移，再接 Dispatcher；不得简单映射为 UI 文本 |
| S08 | P1 | `domain/mod.rs` `ChatTurn` / `apps/desktop/src/types.ts` | 一轮回复仅整段 `assistant: Option<String>` | 序列号、逐段勾叉、分支讨论、来源/历史版本无稳定对象身份 | 设计 Message/Sequence Item/Selection/Branch 的版本化关系；兼容旧文本 |
| S09 | P1 | `apps/desktop/src-tauri/src/tool_approvals.rs` | 列举文件读、Git 状态/差异、两个 Cargo 命令和 GitHub 文件工具；每次经 picker/对话审批 | 通用能力发现/执行策略及项目持续授权尚无完整实现 | 将当前 allowlist 标为“初期适配层”；设计可配置能力/运行环境与可撤权的策略裁决 |
| S10 | P1 | `crates/wex-desktop-service/src/connectors/terminal_dispatch.rs` | `TerminalTask { CargoTest, CargoFmtCheck }`，实际构造 `Command::new("cargo")` | PC 不能永久限制为 Cargo，不足以支持 Flutter、Node、Python、Android SDK 等项目环境 | 引入受控环境配置与命令能力描述，保留 OS 安全校验、超时、取消、实际退出码 |
| S11 | P1 | `apps/desktop/src-tauri/src/main.rs` `send_message()` | 流式与带工具非流式分为两条执行路径；每个会话同时只允许一个请求 | 生成中追加排队与工具循环不能使用同一会话调度机制 | 引入会话队列及真实运行生命周期，不把“追加”实现成隐式取消 |
| S12 | P1 | `application/mod.rs` `prepare_chat()` | 每轮把所有成功历史消息重放入 API 请求 | 长期项目记忆被会话历史绑定；大项目将增加 token、旧信息污染风险 | 改为项目基准/快照/来源索引 + 按需检索 + 短期窗口；不得以未经核验摘要替代事实 |
| S13 | P1 | `crates/wex-desktop-service/src/domain/organization.rs` `ContextPlacement` | 一个 Context 只有一条主要 Space/Group 归属记录（文档也明确“One primary location”） | 如未来要求 Context 多处呈现/快捷引用，需要区分主位置与引用关系 | **当前并非明确错误**；保留主归属，后续用关联/别名支持多视图引用；不据此自动改动 |
| S14 | P1 | `apps/desktop/src/Projects.tsx` | UI 文本默认“先添加主 AI”，且成员只能从当前项目创建；四种职责下拉 | 把“主 AI 是可选职责”和“成员可跨 Context”误传成固定流程 | 修改文案和选择流程，但需等待成员/职责数据关系确定后统一施工 |

## 当前做得正确、不应误判成硬编码

- `Conversation.project_id: Option<String>`：普通会话并非强制属于项目。
- `Project.context_id: Option<String>`、`Context` 不要求 AI 或代码仓库：没有把所有 Context 固定成代码项目。
- 关联项目/资源不自动授权，且终端仍逐次审批：**是现阶段安全边界**；问题是尚未补上持续授权和可配置执行策略，不是应该先移除审批。
- `crates/wex-core` 的 Task / Operation 状态处理和证据链接具有可复用基础：但并不代表已接入持久 Dispatcher。
- 模型绑定于会话作为默认模型是合理的；问题在于**成员身份被单个模型和会话永久等同**。

## 推荐整改顺序（不擅自冻结数据结构）

1. **先设计兼容迁移**：身份 Member/Agent、可复任职责 Assignment、可用 Connection/Model、独立 Session、实际授权 Scope 的关系；检查 SQLite 历史数据和 UI/API 兼容性后再改域模型。
2. **再接统一权限与调度**：角色只提出可用工作职责，真正工具/项目资料可见性由授权裁决；Desk Task 与 Core Task 合并为单一权威状态，接持久化 Dispatcher 和任务/证据。
3. **并行推进对话结构演进**：在不破坏已保存纯文本轮次下，引入持久段落身份、确认/引用/分支、消息版本及追加队列；长程记忆从全量历史中解耦。
4. **最后扩充执行环境**：受控工具发现、安装环境映射、项目多工具命令、冲突范围协调，保持当前未授权时拒绝的安全行为。

## 核查门槛

- **多职责**：一个成员同时是用户秘书与某项目 Lead；不同项目权限互不泄露；同一模型支撑两个成员但不同身份/会话各自隔离。
- **跨项目**：同一施工者在两个项目有独立分配，不能因一边授权访问另一边资料。
- **审查只读**：Reviewer 可读被授权的广泛项目证据，但无法直接修改正式源码/任务基准。
- **对话序列**：编辑/重发/分支后旧序列引用不误指新段落；勾选不是工具审批。
- **工具**：项目授权的合法执行可持续，权限撤销后后续请求拒绝；未知副作用不自动重试。
- **项目恢复**：换模型/新对话仍能从已核验文件、决策及快照恢复；旧摘要与真实文件冲突时标记待核验。

## 审查范围与状态

本次直接核对核心域 `domain/mod.rs`、`domain/organization.rs`，`application/mod.rs`、`application/member_brief.rs`，`wex-core/src/lib.rs`、`wex-core/src/task.rs`，前端 `types.ts`、`Projects.tsx`，Tauri `main.rs`、`tool_approvals.rs`，`terminal_dispatch.rs`。**不是对仓库所有源码、历史分支和 SQL 迁移文件的穷尽扫描**。未运行编译、测试或 Windows 实机验收；本报告只登记已由源码佐证的语义耦合及迁移风险，不声称已经修复。

## 2026-10-25 首批整改记录（部分完成）

- **新增职责绑定对象**：`domain/agent_assignment.rs` 的 `AgentDutyAssignment {id, agent_id, project_id?, duty_id}` 是**明确不授予工具权限的工作分配元数据**。同一已有成员现在可新增多个不同项目职责及用户级 `secretary` 职责；允许扩展 duty ID。旧 `Agent.project_id/role/connection/model/conversation` 暂时保留，未声称完成彻底解耦。
- **服务与 UI 接线**：新增 `DesktopService::assign_agent_duty`、Tauri `assign_agent_duty` 命令以及项目页试验性“关联已有成员职责”入口。服务验证实际成员及项目存在，拒绝重复职责记录与非法 ID。**该关联不创建新的项目工作会话、不赋予该项目资料可见权、不允许额外工具调用或跨项目施工**；真正的职责 ACL、Task 投影和 Dispatcher 仍待接通。
- **数据兼容性**：`DesktopSnapshot.agent_duties` 以 `#[serde(default)]` 读取旧数据；新建 Agent 时登记原项目职责；SQLite `user_version` 从 4 升至 5，防止旧二进制静默丢失新字段。新旧持久数据读回、重复职责和非法值已增加源代码测试，但**本轮未执行 Cargo、CI 或 Windows 测试**。
- **语义提示修正**：更新 `prepare_chat()` 的旧提示词，明确普通对话部分工具已接通而自动派工尚未接通；明确职责提示本身不是授权。
- **尚未修复**：S01～S06 只是为身份/职责关系建立兼容桥，旧项目单属字段、固定 Role 和单一会话仍存在；S05 的 Lead 可见性投影、S07 Task 状态分裂、S08 序列聊天、S09～S12 工具/环境与记忆均未完成。不要把这些标为“已解决”。
