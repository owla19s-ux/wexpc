# WEXPC 远端分支清理审计（2026-10-27）

状态：进行中，仅作为手工清理依据；不代表测试通过或自动删除授权。

## 绝对保留
- `main`：默认主线。
- `feat/pc-scope-overlap-pure-2026-10-25`：开放 PR #7，独立原型。

## 等待归档复核
- `docs/connection-contract-review-20261026`：部分提案已逐文件迁到 main，其余研究提案与历史提交还需复核；不能直接合并旧正式基线。
- `docs/lightweight-host-architecture-v01-20261026`：架构正文已归档；其余差异仍需复核。
- `circleci-project-setup`：独立 .circleci/config.yml；未决定启用第二 CI，不能擅自将其迁入主线以免增加构建费用。
- `docs/pc-product-baseline-2026-10-24`：历史平台参考文件存在，正式产品基线已更新；需先核对参考资料再删。

## 已归档独立成果（main）
- `docs/proposals/CONNECTION-EXECUTION-CONTRACT-REVIEW-2026-10-26.md`
- `docs/proposals/CONTEXT-COMPOSABILITY-TEMPLATE-LEAKAGE-AUDIT-2026-10-27.md`
- `docs/architecture/WEXPC-LIGHTWEIGHT-HOST-ARCHITECTURE-V0.1.md`
- `docs/proposals/AGENT-DUTY-SCOPE-COLLABORATION-SCENARIO-REVIEW-2026-10-27.md`
- `docs/proposals/AGENT-PROJECT-CONTEXT-COUPLING-AUDIT-2026-10-27.md`
- `docs/proposals/AGENT-WORK-IDENTITY-SCOPE-AUDIT-2026-10-27.md`
- `docs/proposals/CONTEXT-ROLE-CAPABILITY-BOUNDARY-AUDIT-2026-10-27.md`
- `docs/proposals/W2-W3-TASK-EVIDENCE-IMPLEMENTATION-SLICE-V0.md`
- `docs/proposals/WEXPC-0.1.1-FOUNDATION-CROSS-REPO-EVIDENCE-2026-10-27.md`
- `docs/proposals/WEXPC-0.1.1-THREE-PHASE-FOUNDATION-RESEARCH-2026-10-27.md`
- `reviews/API-PC-01-ADVERSARIAL-ACCEPTANCE.md`
- `reviews/tests/api_pc_01_invariants.rs`
- `crates/wex-desktop-service/tests/legacy_api_regressions.rs`
- `crates/wex-desktop-service/tests/tool_call_id_reuse.rs`

以上研究文档属于历史提案，不应自动视为已批准的正式架构；新增测试未运行。

## 第一批允许删除的充分条件
远端分支 HEAD 必须是当前 `main` 的祖先，且删除前远端 SHA 未变化。以 `scripts/cleanup-fully-merged-branches.sh` 在最新仓库环境中重新核查。不得仅凭 PR closed、功能相似或提交数判定安全。

## 第二批差异分支处理规则
若旧分支 HEAD 非 main 的祖先，应逐个核对：独立文件、关键边界测试、后续版本实现和文档归档。即便复制了独立文件，也不得凭这一点认定旧分支可删除。保留来源记录，先完成验收再人工操作。

## 当前远端状态（查询时）
- 83 个远端分支，含 main；开放 PR 仅 #7。
- 本轮尚未删除远端分支，也未主动触发 GitHub Actions。
