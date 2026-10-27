# WEXPC 归档成果回收与验证记录

状态：**进行中；安全/幂等首组已完成源码对照和主线 Linux 回归，其他归档项仍待核查**。工程 D 域的代码收敛记录，不修改已确认的 0.1.1 产品基线。当前比较目标：`main`，归档来源：`refs/tags/archive/wexpc-20261027/**`；最终以回收时最新 `main` 提交再次校验。

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

## 第一轮验证快照（历史）

第一轮仅通过 GitHub 连接器读取 43 个归档标签、对比上述源码和文件差异并写入审计记录。Rust/Host/前端/Windows 测试均 **NOT_RUN**；未修改生产代码、未触发 Actions，不能声称归档代码已全部回收。

## 第二轮：安全/幂等与调度租约实测闭环（2026-10-27）

工程起点 `ccf8cc5a67bf5ee56ee681ad58a4a2644842da45`。不整支合并历史代码，不改变 0.1.1 范围。

| 归档来源与固定 SHA | 当前源码证据 | 本轮结论 |
| --- | --- | --- |
| `fix/dispatch-parent-child-leases-20261026` / `891400f8e37b48961e272dde2bf36ae492ca8a71` | `wex-core/src/dispatch_leases.rs` 的非测试生产部分在去除注释及空白后 token 序列相同；主线另有独立 registry、旧 generation 释放保护、真实竞争线程测试；`dispatch.rs` 有 reserve 父子冲突及无关范围可领取的接线测试 | B → 此租约生产模块已覆盖且 Linux 验证通过；不需复制旧文件。该结论不等于持久 Dispatcher、跨进程/外部 Agent 或文件系统锁验收 |
| `fix/api-final-round-dispatch-guard-20261025` / `769ef2c52878036cd881f0191fa7b39981d3f55e` | 现行 `connectors/tool_round.rs` 在 checkpoint/dispatch 前执行 `ensure_followup_budget`；最终轮提案不执行、最终轮文本消费回执、pending 保存失败不派发，以及恢复 ID 去重测试均通过 | B：此最终轮安全属性已回收并验证；不覆盖新版恢复与去重代码 |
| `fix/pc-git-fsmonitor-exec-fence-20261025` / `ff86a54818bbb3aa9924c3675583415e70e7abe2` | 现行 `git_dispatch.rs` 保留 `-c core.fsmonitor=false`、diff 的 `--no-ext-diff --no-textconv`，并增加继承/显式 GIT_* 环境清除；真实 Git 仓库重定向隔离测试通过 | B：旧防线已保留，新增环境隔离有运行证据。此次未新增真实恶意 fsmonitor hook 专项，不能以环境隔离测试冒称该专项已执行 |
| `test/pc-tool-id-reuse-20261025-2232` / `d3765e30b63e55659ba93ea44ce940abab68d9ce` | 主线 `tests/tool_call_id_reuse.rs` 四项与 `tool_resume.rs` 恢复 ID、同批重复 ID、历史重复 ID 拒绝测试通过；均检查派发前拒绝 | B：此 ID/断点回归组已运行；不代表真实第三方服务或全部恢复故障已验收 |

### 失败 → 修复 → 复测的真实证据

- 首次验证 [#40527579681](https://github.com/owla19s-ux/wexpc/actions/runs/40527579681)，HEAD `b279bf8997717a82578be22109ca5ec19c3203b1`：fmt FAIL；两个新增测试文件的四处换行不符合 rustfmt，Clippy/测试未执行，不能计为通过。
- 修复提交 `c1ff8d9e83c920fb3a531926e5dc6aa47039f805` 仅修正 `crates/wex-core/src/dispatch.rs`、`dispatch_leases.rs` 的格式并更新 Linux 请求；没有修改生产行为、授权或数据结构。
- Linux [#40527701505](https://github.com/owla19s-ux/wexpc/actions/runs/40527701505)，Job `122271653337`，HEAD `c1ff8d9e83c920fb3a531926e5dc6aa47039f805`，Ubuntu 24.04 x86_64 / Rust 1.99.1：`cargo fmt --all -- --check` PASS；`cargo clippy --workspace --all-targets --locked -- -D warnings` PASS；`cargo test --workspace --locked` **624 passed / 0 failed / 0 ignored**（日志各 test result 汇总）。
- 前端 [#40527806229](https://github.com/owla19s-ux/wexpc/actions/runs/40527806229)，Job `122272026118`，HEAD `4f3e5494ea9702bf507ccceb1eff635c9ba25cc4`，Node 22：`npm ci` PASS；`npm test` **16 文件 / 78 测试通过**；`npm run build` PASS。覆盖先前 main 已有的发布 epoch=0 不得显示已核实派工的修复。与上述 Linux HEAD 的差别仅为前端请求文件，源码相同；仍分别保留两个实际 SHA。
- NOT_RUN：本批 Tauri Host 专用编译/测试、Windows 构建/测试、真实 API/GitHub 产品调用、桌面窗口、安装及实机。workspace 不包含独立 Host 的完整验收。

剩余优先项：旧组织 revision/可信执行回执、SQLite 迁移失败与损坏数据、撤权和 Host/UI 调用链继续逐项回收。保留全部归档标签与独立范围冲突原型；本轮没有删除远端分支，不声称全部归档成果已集成。

## 第三轮：旧历史数据丢失修复与 Task 可信证据链核查（2026-10-27）

起点 `e7dfe846ac3525c8c67081f376bd60abb0c20a99`，生产修复 `fc1f6f79ab832a231f96361409ee2b89214e8c35`。本轮不恢复旧可信回执 API，不新增产品数据模型、权限或调度器。

### 已复现并修复：旧集合会在后续写入中静默丢失

归档 `fix/pc-organization-revision-20261025` / `73cf1fc828213bdc3cc913a87dafd2c224b9b69f` 的 schema5 快照包含 `execution_receipts`、`agent_work_reports`；现行 DesktopSnapshot 不包含这两个字段。原迁移只保证升级当下原 body 未被改写；随后 DesktopStore::update 将 typed snapshot 重新序列化，普通修改和启动恢复都会抹掉未识别的根字段。不能将“升级测试保留了 body”推导为“后续使用也能保留历史”。

修复：
- `crates/wex-desktop-service/src/infrastructure/sqlite.rs`：在既有 Immediate 事务内保留原始根字段，通过当前 typed snapshot 覆写所有已知字段，再原子提交；修订号仍由当前模型递增，失败仍回滚。
- `crates/wex-desktop-service/Cargo.toml`：仅启用既有固定版本 serde_json 的 `raw_value` feature。未知根字段用 RawValue 保存，避免先转换成浮点数而损坏大整数；未增加 crate 或改变锁定版本。
- 新增 `tests/legacy_snapshot_preservation.rs` 四项真实 SQLite 回归：schema1～8 升级后普通修改及重开；Running 对话启动恢复；触发器模拟写入失败、回滚及重试；超 u64 历史数字保真。测试没有请求外部 API。
- 旧集合只是兼容保存，仍不出现在现行快照 DTO，不能被当作当前可信 Verify、工具回执或有效授权。对已知字段内部的未识别子字段不作本轮兼容承诺；也不承诺旧二进制降级写入会保留这些字段。

### 真实验证

| 阶段 | SHA / Run / Job | 结果 |
| --- | --- | --- |
| 初始测试格式修正 | `f163436134ea5df53b55c6cf711f130c553b96cf` / #40529226726 / 122277113704 | fmt FAIL，测试未运行；按日志调整测试数组格式，未改生产逻辑 |
| RED 复现 | `2139c87a35e52b1ff68e0ed185392593f52d5145` / [#40529427118](https://github.com/owla19s-ux/wexpc/actions/runs/40529427118) / 122277845091 | fmt/严格 Clippy PASS；四项新增测试均在历史字段被删除的断言处 FAIL（exit 101），不是编译失败 |
| GREEN 完整 Linux | `fc1f6f79ab832a231f96361409ee2b89214e8c35` / [#40529672589](https://github.com/owla19s-ux/wexpc/actions/runs/40529672589) / 122278720108 | Ubuntu x86_64 / Rust 1.99.1；fmt PASS、严格 Clippy PASS、workspace **628 passed / 0 failed / 0 ignored**；四项回归全部通过 |

Windows [#40529779434](https://github.com/owla19s-ux/wexpc/actions/runs/40529779434)，Job `122279103420`，HEAD `3a4b2db017b9e2be437e8598dbb8ba19e6010f4f`：`cargo test --workspace --locked` **463 passed / 0 failed / 0 ignored**；`cargo test -p wex-local-resources --test windows_contract --locked -- --nocapture` **12/12**；`cargo test -p wex-access-store --test windows_store --locked -- --nocapture` **3/3**；`cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --locked` PASS。四项新增数据保真回归在 Windows 均通过。与 Linux 源码完全相同，差异仅为 Windows 请求文件；两个实际 SHA 分别登记。两平台的 cfg 用例集合不同，未编译的另一平台用例不算已执行，也不将重复契约重跑累计为独立 workspace 用例。

NOT_RUN：Windows 安装/窗口交互、真实模型和 GitHub 产品调用、实机撤权、真实任务 worker 派工；未运行 Windows Host 专用单元测试（本轮只做 Host check）。前端源码未变，未重复执行前批前端检查。

Linux 命令：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`。本轮也重新运行了旧损坏数据不改版本/正文、schema5 不凭空生成授权、任务记录写入失败无半记录、策略撤销/旧修订拒绝等现有测试。

### 仍未回收的真实链路与修订保护

- [Issue #83](https://github.com/owla19s-ux/wexpc/issues/83)：现行 DesktopService 持久 Task/DispatchAttempt；Host 工具使用临时 OperationLedger 和独立 conversation/turn marker，但没有发现真实 Host worker 调用 claim_dispatch_attempt，也没有 TaskOperationIndex 与该工具路径的持久关联。Core 原语、手动 TaskRecord 和旧内部 ExecutorOutcome（带 allow(dead_code)）都不能冒充真实 Task → Operation → Receipt 闭环。后续按当前授权和回执生命周期接线，无 Task 普通对话保留。
- [Issue #84](https://github.com/owla19s-ux/wexpc/issues/84)：旧 organization.rs 已有 move_context_checked、bind_project_context_checked、bind_conversation_context_checked；main 的现行服务和 Host 仍调用无 expected revision 的接口，Organization.tsx 移动提交也不传 revision。已有 rename/update 保护不代表上述操作已覆盖。本轮仅完成静态定位，未修改其接口或新增运行复现。
- 撤权核查：现行服务策略检查比较当前会话/能力/策略修订；工具恢复发送前复核连接/模型/凭证绑定。Host 本地读写通过当前目录权限检查，GitHub 总开关/凭证每次读取，revoke 先禁用后删凭证。Host 源码证据与 workspace 已跑的策略测试分开，不将静态 Host 调用核对冒称 Windows 撤权交互验收。

原组织分支仍归 C：只修复了兼容数据保存，任务证据接线与三个组织修订门禁未全部回收，不能宣布整支成果完成。
