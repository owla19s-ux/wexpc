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

## 2026-10-27 Windows 完整测试后新增证据

- Windows gated validation #40408913676：`00c3b98f1df0bf6aa3b320586555d76ae503652b` 对应 `cargo test --workspace --locked`、Windows local resources contracts、Windows storage contracts、Tauri Host `cargo check --locked` 全部通过。此前 Linux #40403892754、frontend #40405405746 分别成功；各自对应不同 SHA，不能宣称同一 SHA 全平台验收。
- 本轮仓库仍有 83 条远端分支（main + 82 条）。
- 通过 GitHub compare(main...branch) 复核，以下分支 `ahead_by=0`，属于**可进一步核对远端 HEAD 后删除的候选**：`integration/tool-catalog-main-20261027`、`work/process-chain-20261026`、`work/process-pipe-cleanup-20261026`、`integration/process-evidence-ui-20261026`、`integration/task-dispatch-ui-20261026`。
- 下列分支仍有独有提交，**禁止以 closed PR 理由直接删除**：`fix/ui-task-duty-selection-20261025`（ahead 2）、`feat/pc-scope-overlap-pure-2026-10-25`（ahead 39，PR #7）、`docs/connection-contract-review-20261026`（ahead 29）。独有提交不代表应整支合入。
- 当前 GitHub 连接器未提供远端分支删除能力；尚未实际删除分支。其他分支仍需逐批 compare，不得把以上抽样当作 82 条均已审计。

## 第二批审计：按 main...branch 比较（2026-10-27）

本批核查 35 条历史分支（含前批部分重复复核）；数值随 main 前进可能变化，删除前必须重新核对 SHA。以下为明确的 `ahead_by=0` 例子，提交已在 main 历史中：

- `integration/dispatch-core-rebase-20261026`、`integration/p1-lease-scope-fix-20261027`、`integration/pc-git-terminal-consistency-20261025-1723`
- `integration/pc-local-policy-resume-20261025`、`integration/pc-local-reply-recovery-20261025-1930`、`integration/pc-tool-ui-api-combined-20261025-1925`
- `integration/process-evidence-ui-20261026`、`integration/task-dispatch-ui-20261026`、`integration/tool-catalog-main-20261027`
- `integration/ui-github-revoke-recovery-20261025`、`integration/ui-task-duty-revision-20261025`
- `review/process-legacy-capture-status-20261026`、`test/p1-git-ambient-env-regression-20261026`
- `work/durable-evidence-batches-20261026`、`work/process-cancel-20261026`、`work/process-chain-20261026`、`work/process-pipe-cleanup-20261026`

仍有独有提交（`ahead_by > 0`）需区分“已经以新 SHA 收回成果”与“仍缺功能”：

- `integration/pc-api-tool-reply-recovery-20261025-1932`（ahead 1）原新增的 `saved_chat_reply.rs` 及其测试，在目前 main 已存在；需对比内容或行为，不能直接整支合并。
- `integration/ui-tool-resume-continue-20261025-2030`（ahead 3）及 `review/process-poll-stale-state-20261026`（ahead 2）：待差异审查。
- `review/api-pc-01-adversarial-acceptance-2026-10-25`（ahead 3）和 `test/pc-tool-id-reuse-20261025-2232`（ahead 1）：独立测试文件已曾归档到 main，但历史提交仍独立。
- `test/api-tool-transport-mock-20261025`（ahead 4）：原 Python 测试工具路径 `reviews/harnesses/api_chat_mock.py` 和 `test_api_chat_mock.py` 在 main 均已存在；仍需检查内容差异。
- `test/tool-catalog-github-visibility-20261026-2144`（ahead 10）：`apps/desktop/src-tauri/src/tool_catalog.rs` 在 main 已存在，具体实现和测试差异待审查。
- `docs/architecture-dedup-review-v0-6-20261026`（ahead 3）：原新增提案 `docs/proposals/WEXPC-ARCHITECTURE-DEDUP-BASELINE-REVIEW-V0.6.md` 已存在 main；不得因此自动提升为已批准架构。
- `docs/connection-contract-review-20261026`（ahead 29）：多份研究资料已归档 main，仍需历史内容审计。
- `docs/pc-product-baseline-2026-10-24`（ahead 5）：包含旧基线和工程说明，不得用历史版本覆盖现行 PC 基线。
- `docs/architecture-reference-alignment-20261025`、`docs/connection-architecture-review-2026-10-26`、`docs/pc-011-concurrent-agents-emulator`、`docs/pc-011-light-project-multi-agent`、`docs/pc-user-lifecycle-20261025`、`docs/unified-connectivity-semantics-20261025`：仍有独有提交，待确认是否属于新变更或旧文字。

此清单是逐批审计中间结果，不是全部 82 条分支的最终清理许可。本批未删除任何远端分支。
