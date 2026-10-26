# Context 可选组合与软件开发模板渗透审计（第四批，2026-10-27）

**状态：静态审计和语义纠偏，不是已批准代码设计或测试结果。** 权威语义见 main `docs/PC-PRODUCT-AND-TECH-BASELINE.md` §0.3（本次新增）。

## 1. 根因

早期 WEX Context 设计没有完整写明：**用户可以为单个 Context 自行勾选、组合连接方式、具体能力、Agent 和资源。** 开发实现偏向软件开发模式中的“项目→Agent 成员→项目任务→会话”流程，使这种具体模式的默认组合被错误当成通用 Context 结构。这是产品设计表达不足与代码绑定共同造成的偏差；不是要求删除软件开发功能。

## 2. 用户确认的通用行为

- Context 可以仅有一个资源地址、一个 Agent、一个搜索能力，也可以有零到多个资源／Agent／工具／连接，按实际使用需要组合。
- 能力可以从不同接入方式、连接实例和工具中选择；用户可以调整、增加、取消组合，而不是套用唯一软件开发模板。
- Space / 可选 Group / Context 是组织与工作环境结构；Context 类型和模板是选择性预设，不应覆盖通用模型。
- 软件开发模式仍可提供 Project、任务、角色分工、调度和可信回执，只是其中一种选择。
- **选择／关联不是授权。** 能力是否可用、具体权限、执行策略和可信回执分别检查；不能把“勾选搜索”视为已经授予远端数据访问权。
- Context 不是固定角色树，也不限制一个 Context 引用多个 Context／项目或相反引用关系。

## 3. 核对的源码事实

| 位置 | 实际代码 | 对自由组合的影响 |
|---|---|---|
| `domain/organization.rs::Context` | `id/name/goal/resources: Vec<String>` | 能表达普通资源字符串，尚无显式能力选择、连接实例、Agent 或关联关系模型 |
| `application/organization.rs::create_context` | 要求 space_id，可选 group_id；创建 Context 和唯一主要位置 | 可以作为 UI 组织位置；不能将其作为工作能力或授权唯一来源 |
| `application/organization.rs::update_context` | 只更新名称、目标、资源地址 | 不能直接维护选中的工具、接入方式或 Agent 组合 |
| `application/organization.rs::bind_project_context` | 一个项目保存一个可选 context_id | 可暂作为主关联；不足以表达跨 Context／项目多对多引用 |
| `domain/mod.rs::NewAgent` 和 `application/mod.rs::add_agent` | Agent 创建必须指定项目并自动创建项目会话、项目 Duty | 模板专属默认流程污染通用 Agent 入口 |
| `application/mod.rs::prepare_chat` 与 `member_brief::build` | 从固定 Agent.role/project_id 决定本次职责和资料 | 可选组合尚未成为工作调用的真实选择依据 |

目前只验证了这些直接路径，不能由此断言代码库里其他能力目录、适配器或 Host 授权全部不存在。

## 4. 兼容纠偏原则

1. 保留现有 Project/Task/Agent/Conversation 与软件开发执行链；将固定项目关系视为软件开发模式中的默认组合，不是所有 Context 的通用不变量。
2. 先审视“Context 的可选择对象／能力／连接引用”与“模板初始组合”的语义，再设计兼容的增量关系表达；不要把资源地址字符串当成万能 ACL 或直接扩大为全部关系。
3. 区分“能力可发现／连接已配置”“Context 已选择／引用”“入口当前实际授权”“真实操作回执”。多选功能不是权限升级。
4. Context 模板应支持初始预设之后调整；模板变更不应暗中撤销用户选择或给旧实例新增权限。
5. 不把 `space_id`、`project_id`、`role`、`conversation_id` 等原字段直接批量删除；先完善调用范围，审查持久化／DTO 迁移。
6. 保持软件开发 0.1.1 已确认交付边界；类型／模板具体存储、UI 勾选粒度与能力发现协议暂未冻结。

## 5. 后续审核用例（尚未运行）

- Context X 仅关联资源 A、Agent B、搜索能力 C，无 Project/Task，可保存、重开并在获实际授权后正常搜索。
- Context X 改用另一搜索连接实例，仅变更这项选择，不自动迁移或授予其他工具权限。
- 从软件开发模板创建 Context 后取消其中一项可选能力，不破坏 Project 记录或其他工作资料。
- 一个 Agent 被两个 Context 引用，分别选择不同 Duty 和能力，不串用会话提示及执行授权。
- 同一 Project 被多个研究 Context 关联，变更其中一个引用不移动项目、清空其他引用或改变权限。
- 禁用连接、撤销工具授权、移走 Context 关联分别有可区分后果；已发生的外部写入保持可信状态。

## 6. 边界

本批只更新文档；未修改 Rust/前端、未合并审计分支、未触发 Actions、未运行本地测试。下一步应继续审查连接能力发现、配置勾选与 Host 权限实际求值之间的契约，不先冻结数据库字段。
