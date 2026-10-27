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

## 第三批审计：feat/fix/UI/CircleCI（2026-10-27）

使用 `compare_commits(main, head)` 逐条检查剩余 43 条 feat/fix 和 3 条其他命名空间，审计仅用于筛选，不能代替内容差异及远端 SHA 二次核对。

### 新增明确已合入候选（ahead_by = 0）
`feat/api-protocol-proposal`、`feat/p1-git-commit-preflight-20261026`、`feat/pc-agent-task-state`、`feat/pc-api-desktop-integration-20261025`、`feat/pc-desktop-foundation-2026-10-24`、`feat/pc-desktop-task-core-integration-20261025`、`feat/ui-scoped-settings-20261026-1639`、`fix/api-protocol-boundaries-20261025`、`fix/api-text-completion-role-terminal-20261025`、`fix/p1-git-environment-fence-20261026`、`fix/pc-github-postwrite-readback-20261025`、`fix/pc-task-link-reassignment-20261025`、`fix/ui-await-latest-snapshot-20261026-1232`、`fix/ui-dispatch-recheck-pending-20261026-1130`、`fix/ui-project-chat-navigation-20261025`、`fix/ui-reconcile-failed-mutations-20261026-1025`、`fix/ui-snapshot-freshness-20261026-0825`、`fix/ui-tool-result-after-request-20261025`、`fix/ui-verified-connection-creation-20261026`、`ui-api-connection-freshness-20261026`。

### 仍有独有提交的重点分支

- `feat/connector-capability-catalog-extraction-20261026` ahead 13；`feat/pc-dispatch-eligibility-batch-20261026` ahead 10；`fix/dispatch-parent-child-leases-20261026` ahead 7。这些触及核心能力/调度，不可整支盲目合并。
- `fix/pc-organization-revision-20261025` ahead 72，存在历史分叉；须检查与当前正式服务/事务回执是否重叠，避免用旧链路覆盖新链路。
- `fix/ui-task-dispatch-summary-20261026` ahead 8；`fix/ui-task-footer-evidence-20261026-0723` ahead 4；`ui/dispatch-agent-selection-20261026-0429` ahead 5；这些涉及 UI 文件和交互测试，应同当前 main 差异核对。
- `fix/ui-github-credential-revoke-20261025` ahead 6；`fix/ui-github-status-verified-20261026` ahead 3；`fix/ui-local-create-receipt-20261025` ahead 6；`fix/ui-snapshot-freshness-20261026` ahead 8；需要确认是否均被新实现替代。
- 其余仍有独有提交：`feat/desktop-task-publication-20261026` (1)、`feat/pc-chat-stream-preview-20261025` (1)、`feat/task-operation-bidirectional-index-20261026` (1)、`fix/api-final-round-dispatch-guard-20261025` (3)、`fix/pc-cross-project-task-eligibility-20261025` (3)、`fix/pc-git-fsmonitor-exec-fence-20261025` (3)、`fix/pc-sqlite-v5-upgrade-fence-20261025` (1)、`fix/pc-terminal-drain-20261025` (1)、`fix/pc-tool-chain-verified-20261025` (2)、`fix/ui-process-request-evidence-20261026` (3)、`fix/ui-saved-results-context-isolation-20261026` (3)、`fix/ui-task-duty-selection-20261025` (2)、`fix/ui-task-editor-reload-20261025` (3)、`fix/ui-task-journal-revision-reload-20261025` (1)。
- `circleci-project-setup` ahead 1，只有独立 `.circleci/config.yml`，第二平台 CI 仍属候选；不擅自启用。
- PR #7 的 `feat/pc-scope-overlap-pure-2026-10-25` 仍保留并单独审查。

### 清理原则更新

已合入候选可以进入下一步远端 SHA/PR 状态确认；独有提交仅证明 Git 历史不同，不能直接证明当前 main 缺功能。优先做源码/测试对照而非整个历史分支合并。尚未执行远端删除。

## 第四批专项：高差异分支对照（2026-10-27）

直接比较 `main...branch` 后，对高差异分支核对实际文件存在性：

| 分支 | ahead/behind | 本批处理结论 |
| --- | --- | --- |
| `fix/pc-organization-revision-20261025` | 72/536 | 旧分支独有 `application/execution_receipts.rs`，main 当前没有该文件；但旧文件直接引用旧版 `DesktopService`、`ExecutionReceipt`、`snapshot.execution_receipts` 和 Agent/Task 关联模型，与 main 现行职责/结构不同，**禁止直接复制进入现行模块**，须单独核对当前任务回执闭环是否包含其安全属性与测试断言 |
| `feat/connector-capability-catalog-extraction-20261026` | 13/204 | 旧分支新增 `src-tauri/src/tool_catalog.rs`，main 已有该文件；下一步按逻辑和用例对照，而非整支合并 |
| `feat/pc-dispatch-eligibility-batch-20261026` | 10/323 | `wex-core/src/dispatch.rs`、`dispatch_leases.rs` 和 Host 集成文档 main 均已存在；需对比当前调度门禁和冲突恢复语义 |
| `fix/ui-task-dispatch-summary-20261026` | 8/173 | `TaskDispatchControls.tsx`、`TaskDispatchStatus.tsx` 已存在 main，旧分支新增并不证明主线缺 UI，须按测试/行为差异审查 |
| `feat/pc-scope-overlap-pure-2026-10-25` | 39/536 | PR #7 的 `prototypes/scope-guard/**` 是独立原型，继续保留待明确验收和采用决定，不自动转入生产调度层 |

注意：旧 `execution_receipts.rs` 的可读代码包含对任务成员绑定、调用 ID 去重、成功/失败回执的测试；这些是可用的**审查检查点**，但不是可直接复用的现行实现。此批没有将旧源码直接合并到 main，也没有删分支。现行 Windows 自动化最新成功 #40408913676；它不能证明旧原型已兼容现行接口。

## 第五批：执行回执语义对照（2026-10-27）

检查来源：旧 `fix/pc-organization-revision-20261025` 的 `application/execution_receipts.rs`，以及最新 main 的 `application/mod.rs`、`application/task_records.rs`、`application/tool_reply.rs`、`application/execution_policy.rs`。

| 旧约束 | 目前 main 证据 | 结论 |
| --- | --- | --- |
| 成员必须属于任务且已被分派 | `task_member_assigned`、派工记录版本验证以及 `claim_dispatch_attempt` 的成员绑定复核 | 派工路径有约束，但不是旧 ExecutorOutcome 直存接口的一比一替代 |
| 调用标识不能重复登记 | 现行派工依据任务和 `publication_epoch` 去重，并校验 dispatch_attempt id；旧版对 `invocation_id` 去重 | **标识层次不同**：派工去重不直接证明外部工具调用 ID 的统一去重 |
| 失败回执仍要保留、不得称为成功 | 现行 `dispatch_attempts` 的 InFlight/OutcomeUnknown 冷恢复失败封闭；`task_records.rs` 明确只接收用户来源的任务记录 | 已有不确定执行保护，但不能以用户任务记录取代可信执行回执 |
| 回执关联操作前任务快照、revision、agent、tool | 现行派工存 task、project、agent、epoch、status、recorded_revision；工具执行证据需继续对照 Core ledger/Host 工具层 | 字段结构与旧版不同，不能宣布完全覆盖 |
| 不允许外部模型伪造可信执行结果 | 旧接口 `ExecutorOutcome` 是 `pub(super)` 的受限结构；主线工具回执需继续查 Host 实际调用链而非仅应用服务 | **待专门验收** |

处置决定：旧 `execution_receipts.rs` 与当前数据结构不兼容，**不直接迁入 main**；对旧分支保持“待语义证据确认”，不因为 72 条独有提交而直接合并。后续针对 Core ledger、工具回执、主 AI 续答分别采集现行测试文件/调用路径证据，再决定是否要新增最小回归测试。该表是结构化静态核查，不是新运行的测试或安全验收。

## 第七批：现行连接器/调度源文件逐一校验（2026-10-27）

- 新增工具回执回归测试已在 Linux Actions #40501142317 通过：格式、严格 Clippy、工作区测试全部 Success。对应 CI HEAD `c84803879601357dcc9066caaaa5a9d983cbaf24`。
- 旧 `feat/connector-capability-catalog-extraction-20261026` 的 `apps/desktop/src-tauri/src/tool_catalog.rs`、`main.rs`、`tool_approvals.rs` **逐字节对照均与当前 main 相同**。该分支 `ahead_by>0` 是 Git 提交历史差异，不能认定功能缺失；此三文件不需重新复制或合并。其他上下游行为仍依赖已有自动化与后续验收。
- 旧 `feat/pc-dispatch-eligibility-batch-20261026` 的 `crates/wex-core/src/dispatch.rs`（旧 7323 字符，现行 7848）和 `dispatch_leases.rs`（旧 5267，现行 7772）内容不同，而 `docs/PC-DISPATCH-HOST-INTEGRATION-20261026.md` 完全相同。主线逻辑已有进一步修改；不可将旧分支覆盖现行调度实现。
- 旧 `fix/ui-task-dispatch-summary-20261026` 的 `TaskDispatchControls.tsx`（旧 3679，现行 4314）不同；主线有后续变化。应审查独有测试与行为，而非覆盖文件。
- **当前清理分类**：连接器能力目录旧分支进入“可作成果已回收的差异分支候选”（仍须核查 HEAD/其他文件及 PR）；调度/UI 旧分支保持“需语义审查”。没有删除远端分支。

## 第八批：旧调度/UI 回归覆盖核对（2026-10-27）

按旧分支源文件和当前 main 的实际测试名逐个提取比对（静态覆盖对照，不是新测试运行）：

- `fix/ui-task-dispatch-summary-20261026` 中的 `apps/desktop/src/TaskDispatchStatus.test.tsx` 与 main **逐字相同**；`TaskDispatchControls.test.tsx` 旧版有 5 个命名用例，main 有 10 个，**旧版 5 个用例名均已包含**，主线另外补有缺失派工证据、项目/发布版本不匹配、不可用的调度账本、旧预览失效和 IPC 错误未对账等 5 项检查。旧测试文件不必覆盖主线。
- `feat/pc-dispatch-eligibility-batch-20261026` 的 `crates/wex-desktop-service/tests/tool_resume.rs` 旧版 10 个函数名用例，main 为 13 个，**旧版 10 个均有同名覆盖**，主线多有续答时复用历史工具调用 ID、同批重复 ID、历史恢复回合重复 ID 三项测试。不能据此宣布语义完全相同，但没有看到因不合旧分支而丢失旧命名用例。
- 当前仓库 Linux 最新全工作区回归 #40501142317 已通过，新加入回归也在内。Windows #40408913676、frontend #40405405746 是此前成功记录，不代表本轮修改后同一 HEAD 的三平台完整验证。

操作结论：`fix/ui-task-dispatch-summary-20261026` 与 `feat/pc-dispatch-eligibility-batch-20261026` 相关**旧回归测试已经在 main 找到覆盖**；不重新复制旧测试，不整支合并旧代码。剩余有独有历史提交，删除前仍需按全部相关文件和当前 HEAD 完成复核。本批未删除分支。

## 已实际删除分支的回执（2026-10-27）

此前的“没有远端删除能力”已经由 **受保护的 GitHub Actions 清理工作流**解决，不再适用。入口：`.github/workflows/branch-cleanup-gated.yml`，只允许名单内分支，执行前重新检查分支祖先关系、最新远端 SHA 和开放 PR，并以 `--force-with-lease` 限定对应 HEAD；不碰 main 和独立 PR #7 原型。

| 批次 | Actions run | 真实删除数 | 执行后远端总分支数 | 状态 |
|---|---|---:|---:|---|
| 第一批 | [#40502922720](https://github.com/owla19s-ux/wexpc/actions/runs/40502922720) | 5 | 78 | success，已重新查询 |
| 第二批 | [#40503273974](https://github.com/owla19s-ux/wexpc/actions/runs/40503273974) | 15 | 63 | success，全部 15 条消失 |
| 第三批 | [#40503548696](https://github.com/owla19s-ux/wexpc/actions/runs/40503548696) | 17 | 46 | success，全部 17 条消失 |

**合计真正删除 37 条**，远端从 83 条降为 46 条（含 main），剩余非 main 45 条。剩余分支不可据此默认可删除，需区分独有提交和已经被重写/迁移的等价实现；尤其独立 PR #7 的 `feat/pc-scope-overlap-pure-2026-10-25` 保留。上述状态以本轮 Actions 与分支列表实时读取为准。
