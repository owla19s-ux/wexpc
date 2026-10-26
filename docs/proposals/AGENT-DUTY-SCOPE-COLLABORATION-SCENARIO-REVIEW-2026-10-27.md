# WEXPC 岗位职责、作用范围、协作与授权：四场景审查

> 2026-10-27，**静态源码场景测试**。未下载完整仓库、未执行 Cargo/Windows 集成测试；结论仅限已核对源码，不能冒充实际执行成功。对应 WEXPC 0.1.1 W2–W5。

## 前提与实证
- `domain/organization.rs`：Space/Group/Context 与 ContextPlacement 是组织关系，不授予访问权。
- `application/mod.rs::assign_agent_duty`：可给 Agent 添加多个 Duty；project_id 可为空；重复同一三元组被拒绝；注释明确岗位不赋予工具/数据权限。
- `domain/mod.rs::Agent`：保留旧 project_id/role/connection_id/model_id/conversation_id；`Task.agent_ids` 是成员清单，非不可替换的执行者。
- `domain/execution_policy.rs`：`ToolPolicySetting` 由 conversation_id + capability_id + policy + revision 标识，Automatic/Ask/Block。
- `core/dispatch.rs`：单次派工 ticket 绑定 task、project、agent、assignment epoch；Host 必须再做真实权限与资源检查。
- 项目、Context 的引用和职位安排不能凭自身授予文件/GitHub/终端权限。

## 四个用户场景与可行性判定

| 场景 | 可复用部分 | 未接通的必要关系 | 静态判定 |
| --- | --- | --- | --- |
| A. 秘书跨项目读取状态、传达用户微调，但不能直接改项目 | Duty 可不绑定项目；独立对话和项目对象已有；Host 授权边界存在 | 项目列表/状态的受控跨项目读取；传达消息的来源和接收/确认回执；禁止用消息直接充当已批准改动 | 部分满足 |
| B. 项目主 AI 独立运行，安排施工与审查 | 项目角色、Task 成员、Dispatcher 纯规则、工具权限分别存在 | 从项目职责→真实分工→任务派发→结果/修复→持久恢复的端到端接线 | 部分满足 |
| C. 独立研究 AI 同多个项目协作而不成为其下级 | 项目外职责可登记，Context 独立于项目，资源引用理念已有 | 跨对象的合作/委托关系，研究成果身份及版本、访问授权和交付回执 | 未形成完整实现 |
| D. 同一个 Agent 兼任秘书和某项目负责人，只撤销其中一项 | 多 Duty 关系可登记，已有独立工具执行策略 | 撤销单项职责的写入口、岗位×作用范围×操作类别的当次授权决策、执行时记录实际 Duty | 部分满足 |

## 关键风险测试（待编码为契约测试）
1. 秘书获项目 A 状态读取和向其主 AI 传话授权：允许查询和投递；直接修改项目 A 代码必须被 Host 拒绝。
2. 秘书传达用户“小调整”：保存用户原始话语、传达身份和项目主 AI 接收回执；不能自动把传达消息升级为用户正式批准的新需求。
3. 主 AI 拥有项目范围内派工职责但无文件写权限：允许按规则委派给有独立授权的执行者；不得以主 AI 身份直接越权写入。
4. 研究 AI 同时回应两个项目的请求：交付成果能被各项目在有效授权下引用，不自动取得两项目仓库写权限。
5. Agent A 同时兼任秘书和项目主 AI：当次行为标明采用的职责和作用范围；撤销项目职责后新操作不得继续使用其授权，历史 Attempt 不改写。
6. 任务更换 Agent 或 API：分工/模型变化只影响后续行为，历史回执、失败和权限来源不被覆盖。
7. Context 移动、重命名、显示序号变化：不更改 Duty 授权范围、资源身份或外部访问凭证。

## 建议只补一个薄组合契约，不增加一个完整组织管理系统

```text
DutyAssignment:
  agent_id + duty_id + scope_ref? + assignment_id

CollaborationLink:
  source_agent_or_duty + target_agent_or_duty + relation_kind + scope_ref + limits

ActionContext:
  acting_agent + duty_assignment_ref? + active_scope + operation + target_resource
  + conversation/connection? + effective_authority_revision

HandoffReceipt:
  source + recipient + message_or_task_ref + origin/provenance + delivery_state
```

此处仅列**语义信息**，不是批准新增四张表或四个类。最小实现应尽量将这些字段添加到现有 Duty/Task/Dispatcher/Host 流转的合适位置；没有 Task 的秘书传话或研究交付仍然有效。

**协作关系与授权必须分离**：上下级、平行、传达、委派均是带具体作用范围的工作关系；不能直接决定文件/网络/工具权限。用户可给岗位模板、资源授权和执行策略，但权限不因职级自动累加。现有 Automatic/Ask/Block 继续为执行策略，而不是组织权限级别。

## 结论与下一步门槛

架构概念上四种场景能够用现有组件组合表达，暂未发现需要颠覆 Space/Context、Duty、Core Dispatcher 或 Host 权限体系的根本冲突。**源码层面四个场景均未验证完整端到端通过**；最明显缺口是岗位作用范围、跨 Agent 工作交接以及执行时的职责身份关联。

优先从 A 做最小端到端样例：秘书只有项目状态读取与消息转交，项目主 AI 独立判断和继续工作，Host 对越权写入拒绝。A 通过后，复用同一交接/身份/授权机制检验 B–D；不得仅以增加角色名称或聊天提示词作为测试通过。
