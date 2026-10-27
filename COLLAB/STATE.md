# WEXPC 协作状态

更新：2026-10-27；维护者：GPT Work。此页是当前工程入口，产品范围以 [PC 基线](../docs/PC-PRODUCT-AND-TECH-BASELINE.md) 为准，施工以 [0.1.1 计划](../docs/WEXPC-0.1.1-IMPLEMENTATION-PLAN.md) 及 [现行 GitHub 协作规范](../docs/GPT-GITHUB-COLLABORATION.md) 为准。历史共同日志保留在 [PR #4](https://github.com/owla19s-ux/wexpc/pull/4#issuecomment-6386453733)，不代签其他执行者采用或验收。

## 当前执行约定

- 单线推进；默认 main，常规代码、验证、修复、提交及已核查的 PR/分支整理无需逐项确认。按完整链路持续施工，不在每个小提交后等待“继续”。重大产品方向与权限扩大交用户裁决。
- GitHub 为共享事实来源，优先连接器直接操作；测试按环境与影响选择。Actions 已允许按需触发，旧 10 月底暂停安排失效；不运行无关工作流、不自动恢复全平台检查。
- 0.1.1 前期通用基础 → 中期软件开发模板 V1 → 后期真实执行与整版验收。当前先完成归档成果回收，不把被删除分支或旧提案当作功能完成。

## 本轮可回读事实

| 事项 | 已核实状态 | 未完成边界 |
| --- | --- | --- |
| 分支 / PR | 开工实时查询为 main + `feat/pc-scope-overlap-pure-2026-10-25` 两条；PR 搜索为空。旧批量清理工作流不得直接套用 | 原型未获生产采用验收，继续保留；归档标签保留 |
| 前批 main 增量测试 | 起点 `ccf8cc5` 的调度/租约测试首次实际运行遇到 fmt 失败；已在 `c1ff8d9` 修复，并完成复测 | 测试保护租约/预检，不证明真实持久派工或多 Agent 施工闭环 |
| 当前 Linux 验证 | [#40529672589](https://github.com/owla19s-ux/wexpc/actions/runs/40529672589)，SHA `fc1f6f79ab832a231f96361409ee2b89214e8c35`：fmt、严格 Clippy、workspace 628/628 通过 | Windows 与 Host 使用单独证据；不等于真实服务验收 |
| 当前 Windows 验证 | [#40529779434](https://github.com/owla19s-ux/wexpc/actions/runs/40529779434)，SHA `3a4b2db017b9e2be437e8598dbb8ba19e6010f4f`：workspace 463/463，资源契约 12/12，存储契约 3/3，锁定依赖 Tauri Host check 通过 | 不是 Host 单元测试、安装/窗口交互或真实服务验收 |
| 前端验证 | [#40527806229](https://github.com/owla19s-ux/wexpc/actions/runs/40527806229)，SHA `4f3e5494ea9702bf507ccceb1eff635c9ba25cc4`：npm ci、16 文件/78 测试、TypeScript/生产构建通过 | UI 自动化不等于原生窗口/安装交互验收 |
| 安全归档回收 | 父子租约生产 token 对照相同；最终轮次拒绝派发、工具 ID/断点防重复、Git 环境隔离有当前 Linux 运行证据；详见 [回收审计](../docs/ARCHIVED-CODE-RECOVERY-2026-10-27.md) | 只关闭相应属性，不宣布整个历史分支或全部功能验收 |
| 产品闭环 | 已有 API/工具/任务/连接器等源码，具体链路以现行源码为准 | 可信任务执行回执、持久派工、多 Agent 安全并发、前期基础交互、真实服务及 Windows 整体验收继续保留缺口 |

本批生产修复 `fc1f6f79ab832a231f96361409ee2b89214e8c35`：SQLite 写回保留未知根字段，serde_json 启用 raw_value，新增四项真实数据库回归。修复前四项断言失败，修复后 workspace 628 项通过；失败落盘仍回滚，启动恢复不再抹掉旧集合。原 schema 版本、现行快照 DTO、权限与产品范围未改；旧历史数据不因此升级为可信执行证据。没有新建开发分支或 Release。详见[回收审计第三轮](../docs/ARCHIVED-CODE-RECOVERY-2026-10-27.md)。文档提交不重复运行同源码测试。

## 下一条施工链

已定位并登记 [#83](https://github.com/owla19s-ux/wexpc/issues/83) Task/Dispatch → Host Operation/可信证据接线缺口，以及 [#84](https://github.com/owla19s-ux/wexpc/issues/84) 组织移动/Context 重新绑定缺少修订检查。下一施工先复现并回收现有组织编辑保护，再按当前权限/回执契约接执行证据链；不将旧结构整份覆盖新模型。SQLite 损坏旧数据、事务失败和新历史保真回归已通过，但不代表全部存储故障完成验收。每批重新查 HEAD、相关源码、PR/分支与 Actions；本页快照不能代替实时查询。

## 2026-10-27 #84 完成验收与 #83 施工接续

- main 代码基线 `2b5c45b4a039102ef307573d0fdee8e35d8b9faf`；组织修订保护由 `b8554b5` 和 `2b5c45b` 接通 UI → IPC → Rust/SQLite，并新增 `organization_revision.rs` 与前端回归。
- 同一 HEAD 的 GitHub Actions 三条 **success**：Linux [40532589710](https://github.com/owla19s-ux/wexpc/actions/runs/40532589710)（fmt、严格 Clippy、workspace tests）；Windows [40532590691](https://github.com/owla19s-ux/wexpc/actions/runs/40532590691)（workspace、Windows 资源/存储契约、Host check）；frontend [40532589377](https://github.com/owla19s-ux/wexpc/actions/runs/40532589377)（npm test/build）。不等于 Windows 安装或真实 UI 手工验收。
- [Issue #84](https://github.com/owla19s-ux/wexpc/issues/84) 已按本次源码与三平台验证证据关闭。早期 `b8554b5` Linux fmt 失败已被后续 HEAD 修复；不要继续引用旧失败作为最新结论。CircleCI 独立 error 不应覆盖上述 GitHub Actions 成功事实。
- 下一施工聚焦 [Issue #83](https://github.com/owla19s-ux/wexpc/issues/83)：当前 `apps/desktop/src-tauri/src/tool_approvals.rs` 仍由普通聊天工具回合直接执行，`wex-core/src/task_operation.rs` 是未接 Host 持久生命周期的内存投影。应先接真实 attempt/operation/receipt 身份与持久关联，普通对话 task_id 保持可空；权限与 UNKNOWN 必须由 Host 执行时再校验。不得将领域测试当作已实现派工。
