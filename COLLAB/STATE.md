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
| main 增量测试 | 起点 `ccf8cc5` 的调度/租约测试首次实际运行遇到 fmt 失败；已在 `c1ff8d9` 修复，并完成复测 | 测试保护租约/预检，不证明真实持久派工或多 Agent 施工闭环 |
| Linux 验证 | [#40527701505](https://github.com/owla19s-ux/wexpc/actions/runs/40527701505)，SHA `c1ff8d9e83c920fb3a531926e5dc6aa47039f805`：fmt、严格 Clippy、workspace 624/624 通过 | 独立 Tauri Host、Windows 和真实服务不在此结论内 |
| 前端验证 | [#40527806229](https://github.com/owla19s-ux/wexpc/actions/runs/40527806229)，SHA `4f3e5494ea9702bf507ccceb1eff635c9ba25cc4`：npm ci、16 文件/78 测试、TypeScript/生产构建通过 | UI 自动化不等于原生窗口/安装交互验收 |
| 安全归档回收 | 父子租约生产 token 对照相同；最终轮次拒绝派发、工具 ID/断点防重复、Git 环境隔离有当前 Linux 运行证据；详见 [回收审计](../docs/ARCHIVED-CODE-RECOVERY-2026-10-27.md) | 只关闭相应属性，不宣布整个历史分支或全部功能验收 |
| 产品闭环 | 已有 API/工具/任务/连接器等源码，具体链路以现行源码为准 | 可信任务执行回执、持久派工、多 Agent 安全并发、前期基础交互、真实服务及 Windows 整体验收继续保留缺口 |

本批源代码范围仅两个 Rust 测试文件的格式修复；另添加/更新受限 Linux、前端 CI 请求与工程状态文档。没有扩大产品、权限或数据模型，没有新建开发分支或 Release。文档提交不需要重跑同源码测试。

## 下一条施工链

继续对照归档组织 revision/可信执行回执与现行 Task → Dispatch → Operation → Evidence 实际调用，核查 SQLite 损坏/迁移失败原子性及撤权路径；缺失属性按主线接口提取实现并验证，避免旧结构覆盖新模型。每批重新查 HEAD、相关源码、PR/分支与 Actions；本页快照不能代替实时查询。
