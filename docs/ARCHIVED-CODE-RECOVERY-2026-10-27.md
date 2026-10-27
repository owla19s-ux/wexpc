# WEXPC 归档成果回收：第一轮代码差异审计

状态：**进行中；尚未开始回填业务代码**。工程 D 域的代码收敛记录，不修改已确认的 0.1.1 产品基线。当前比较目标：`main`，归档来源：`refs/tags/archive/wexpc-20261027/**`；最终以回收时最新 `main` 提交再次校验。

## 审计原则

- 归档标签能恢复独有提交，但**归档、文件存在、源码差异、功能已实现**是四种不同概念。
- 先检查现行调用链和对应的测试，再判断是否提取补丁；不得将旧版完整文件覆盖新版主线。未完成真实验证一律为 NOT_RUN。
- 分类：A=原样覆盖、B=已有更强实现需要验证、C=可能缺失需逐项提取、D=过时/待决定；未完成检查标记“待复核”，不冒称已集成。
- 本文不创建 PR 或新分支；下一批按调用链选择 C 类必要修改，以小批次直接提交 main 并执行受影响验证。

## 第一轮已核查项目

| 归档原分支 | 实际核查文件或变更 | 初步结论 | 下一步 |
| --- | --- | --- | --- |
| `fix/pc-sqlite-v5-upgrade-fence-20261025` | `infrastructure/sqlite.rs`；旧归档的迁移逻辑与现行主线不同，主线已含版本 8 保存与迁移测试 | B：新版演进，不能倒灌 v5 | 针对旧数据损坏、迁移失败原子性复核主线测试 |
| `test/pc-tool-id-reuse-20261025-2232` | `tests/tool_call_id_reuse.rs`；主线文件比归档长，含失败 pending checkpoint、已完成/重复 ID 拒绝测试 | B：主线覆盖更多用例，语义待核对 | 运行对应测试并检查工具重复调用防线 |
| `fix/dispatch-parent-child-leases-20261026` | `wex-core/src/dispatch.rs`、`dispatch_leases.rs`；归档含父子作用域相交/根目录占用测试，主线存在不同实现 | C 候选：重点风险，暂未认定缺失 | 逐项对照父子、根路径、重复范围、共享 lease，并增加缺失用例 |
| `fix/pc-terminal-drain-20261025` | `terminal_dispatch.rs`；归档与主线不一致；主线拥有后续排版/链路修改 | B/C 待分辨 | 以 stdout/stderr 排空、超时、非零退出及取消 UNKNOWN 为重点核查 |
| `fix/api-final-round-dispatch-guard-20261025` | `tool_round.rs` 单文件 130 行左右变更 | C 待分辨 | 检查模型最终轮次是否仍能派发副作用及断点持久化 |
| `fix/pc-git-fsmonitor-exec-fence-20261025` | `git_dispatch.rs` 单文件差异 | C 待分辨 | 核对外部 Git 配置和 hooks/fsmonitor 的执行风险 |
| `feat/pc-dispatch-eligibility-batch-20261026` | `dispatch.rs`、`dispatch_leases.rs`、工具恢复测试与集成说明 | C 待分辨 | 对齐主线 Task、scope、lease 和实际派发调用 |
| `fix/pc-organization-revision-20261025` | 12 个文件，涉及 revision、任务记录、执行回执和组织持久化 | C 候选（高优先级） | 先拆解接口/数据库迁移/测试，防止旧快照覆盖新版数据 |
| `integration/pc-api-tool-reply-recovery-20261025-1932` | 9 个文件涉及前端回复、保存与重启恢复 | C 待分辨 | 检查正式 Chat 调用点与断点恢复，避免只补单元原语 |
| `fix/pc-tool-chain-verified-20261025` | 37 个文件，跨审批、执行策略、Host/UI、存储 | D：大范围混合提交需切片 | 不整支合并；优先对照撤权、目录、回执、工具链测试 |
| `fix/ui-github-credential-revoke-20261025` | `github_config.rs`、`main.rs`、`vault.rs`、`Connections.tsx` | C 待分辨 | 检查撤权后实际调用是否阻止且 UI 回执可信 |
| `test/tool-catalog-github-visibility-20261026-2144` | Host `main.rs`、`tool_approvals.rs`、`tool_catalog.rs` | C 待分辨 | 核查工具能力发现与 GitHub 授权可见性 |
| `fix/ui-task-duty-selection-20261025` | `Projects.tsx`、`TaskEditor.tsx` | C 待分辨 | 与目前岗位/身份独立的 0.1.1 基线比对 |
| `fix/ui-process-request-evidence-20261026` | `ProcessOutput.tsx` 与 SavedToolResults UI/测试 | C 待分辨 | 核对审批请求参数、结果以及保存证据是否一致 |
| `docs/connection-contract-review-20261026` | 9 份 proposals：已在上一批确认文件与 main 一致 | A：仅源码文件等价，提案非已批准设计 | 保留研究性质，不自动改产品架构 |

## 下一批建议优先级

1. **安全/幂等**：Git fsmonitor 执行防线、最终轮次派发、Tool Call ID 和 pending checkpoint。
2. **调度/持久化**：父子作用域 lease、Task revision/执行回执与恢复。
3. **Host/UI 真实链**：撤权、工具目录可见性、任务分配和证据呈现。
4. **文档/历史参考**：不让旧提案回退当前主线基线。

## 当前验证

本轮仅通过 GitHub 连接器读取 43 个归档标签、对比上述源码和文件差异并写入审计记录。Rust/Host/前端/Windows 测试均 **NOT_RUN**；未修改生产代码、未触发 Actions，不能声称归档代码已全部回收。
