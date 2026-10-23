# WEXPC 桌面首批施工计划

> 执行方式：由当前会话直接实施；用户已授权技术决策与开始开发，UI 后续细化。本计划记录本批，不另行增加产品审批。

**Goal:** 建立 Windows Tauri/React 正式入口，接入 Rust 的项目、AI 成员、任务草案和普通 API 对话持久化。

**Architecture:** Rust 服务负责业务校验和 SQLite 权威写入；Tauri 只传输意图并使用 Windows Credential Manager 保存密钥；React 提供可操作的基础页面。复用 `wex-core::connection_session::Catalog` 的连接/模型选择规则，不把旧 Flutter FFI 迁成桌面接口。

**Tech Stack:** Rust、SQLite/rusqlite、reqwest、Tauri 2、React/TypeScript、Vite。

**Spec:** `docs/PC-PRODUCT-AND-TECH-BASELINE.md`；本会话明确的并发施工、独立验证/审查、Android 模拟器要求。UI 草图仅作入口参考，视觉未冻结。

## Global Constraints

- 产品版本保持 0.1.1；Windows 首先交付。
- 密钥不进入 SQLite、普通日志、模型上下文和前端回读。
- 资源关联不等于 AI 授权；本批创建项目/任务不启动外部工具施工。
- 多成员绑定到连接/模型，职责和会话分开；任务不写死单施工/单审查数量。
- 本批不是完整 0.1.1：文件/终端/GitHub 工具循环、并发范围调度、验证审查引擎、模拟器实际接线仍须后续施工。

## Review Focus

- 冷重启后项目、分工、消息保留；中断的请求明确标记，不自动重复发送。
- 连接禁用后新请求拒绝；已有对话不会偷偷切到其他模型。
- 同一对话不能重叠提交，不同对话不被网络全局锁阻塞。
- API 失败和存储失败可见；供应商原始错误不携带密钥进入记录。
- 任务关联/成员绑定校验，普通对话不强制项目，创建任务不伪报已施工。

## Tasks

1. Rust 服务与测试：`crates/wex-desktop-service`，先写行为测试并观察失败，再实现项目/成员/任务/连接/对话和 SQLite 恢复。正式复用 Catalog 校验；用本地 HTTP 服务验证真实请求格式、响应及错误路径。
2. 桌面 Host 与基础 UI：`apps/desktop`，接入上述服务、Windows 密钥库、项目创建、成员绑定、任务草案与普通对话；网络在独立阻塞工作线程执行。浏览器预览明确无桌面服务，不使用模拟数据冒充运行。
3. 合批验证：Rust 单元/集成、fmt/clippy，前端类型检查与生产构建；只针对桌面改动设置相应 CI。记录本环境限制，提交对应 Commit 与 PR，不构建安装包。

## 快速复用结论

- 直接依赖：Rust Catalog 的身份、连接启停、模型绑定和选择校验。
- 保留待工具批接线：`wex-core::access/operation`、Windows `wex-local-resources` 与 `wex-access-store`，不修改其安全行为。
- 不迁移：旧 Flutter 页面、Android Host 与 FFI；连接器 Dart 代码仅参考协议行为，不逐文件审计。
- SQLite 是本批新桌面数据的唯一正式写入口，不同时写旧 CatalogStore/Flutter 配置；没有 Android 数据自动迁移。

## 本批施工记录

- Rust 服务、Tauri 命令和 React 页面已落地。成员独立会话、任务关联、项目/职责/已分配任务上下文已接线；当前仅文本 API，任务仍为草案。
- 实际运行 `cargo test --workspace`：230 项通过，其中新服务 10 项；`cargo clippy --workspace --all-targets -- -D warnings`、两处 Rust 格式检查和 `npm run build` 通过。
- HTTP 使用本地真实测试服务器验证请求/回复和错误脱敏；未使用真实 DeepSeek/OpenAI 凭证，未宣称服务商验收。
- 独立代码审查发现结果落盘失败会锁住对话，已补内存结果保留与只重试保存的 UI/Host/服务链，并用 SQLite 写入故障注入验证恢复。旧快照不覆盖新 revision；请求未被接收时保留输入草稿。
- 本环境 Linux Host 检查因缺少 pkg-config/glib/WebKit 系统依赖而停止。Windows Host 由相关 PR 的桌面工作流验证编译，结果以对应提交的实际 Actions 为准；未运行 Windows 凭证库或实机验收，不构建安装包。
- 决策：当前 app identifier `app.wex.pc`、图标是开发工程占位，不代表正式安装包、签名或最终 UI 定稿。
- 决策：已返回但落盘失败的结果暂存于本进程内存，允许原请求重试保存而不重复调用 API；关闭进程后失去尚未落盘的结果，重启显示中断，不伪造恢复成功。
- 完整多 Agent 并发施工、工具权限/执行、任务发布/依赖/范围互斥、验证审查/修复流转与 Android 模拟器均保留为 0.1.1 后续批次，不因本批基础服务完成而关闭。
