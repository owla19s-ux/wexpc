# WEXPC 开发入口（工程 D 域）

先读 [PC 产品与技术基线](docs/PC-PRODUCT-AND-TECH-BASELINE.md)、[0.1.1 施工路线](docs/WEXPC-0.1.1-IMPLEMENTATION-PLAN.md)、[仓库结构](docs/REPOSITORY-STRUCTURE.md) 和 [开发及 GitHub 操作](docs/DEVELOPMENT-WORKFLOW.md)，再核对目标分支、源码、PR、提交与必要 Actions。产品内软件开发模式 M 与当前开发 WEXPC 的工程方法 D 分开。

- WEXPC 是当前施工仓库；原 WEX 仓库只作已确认需求、设计与可复用代码的追溯来源。旧 Android 版本阶段、UI、权限简化和工程规则不能自动覆盖 PC 基线。
- 现有 Rust/Flutter 是迁入资产，需逐模块证明适用性；Windows 后端测试不等于桌面产品完成。Tauri/React 已有相关施工 PR；以目标分支和真实代码/验证证据判断集成状态，不能沿用早期“尚未落地”的快照，也不能将 PR 静态接线当作 Windows 验收。
- 新需求与建议、已确认规范、代码完成、自动化、真实服务、Windows 构建及实机验收分别报告。阶段建议不得冒充批准范围。
- 不为了通过单个测试缩减多项目、多成员、权限、持续任务等完整目标；本批切片要记录未完成项。
- 相关改动合批验证。纯文档提交不自动跑 CI；源码 PR 按影响自动检查，直接 main 源码变更按实际影响手动验证；交付时报告对应提交 SHA 的实际证据，未运行写明未运行。

## 当前 GPT/GitHub 协作执行入口（2026-10-27）

优先遵循 [GPT × GitHub 协作执行指令](docs/GPT-GITHUB-COLLABORATION.md)：GitHub 通用 fetch 查完整 Actions 运行列表、连接器受限请求触发验证，以及**少分支/少 PR、按链路集成后清理**。该文覆盖旧开发流程中的月底 Actions 暂停条款和旧分支安排；仍以 PC 正式产品基线为产品权威。
