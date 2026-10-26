# P1 本地 Git Commit：执行契约与接线计划

依据：[0.1.1 施工路线](WEXPC-0.1.1-IMPLEMENTATION-PLAN.md) W1/W2 及 [PC 基线](PC-PRODUCT-AND-TECH-BASELINE.md)。这是既定 P1 的实现切片，不修改版本或产品范围。

## 已有代码及用途

- `connectors/git_commit_policy.rs`：无副作用的消息/精确路径预检。拒绝穿越、Git pathspec/glob、控制字符、Windows 盘符/ADS、设备名称、尾部点/空格、大小写重复与 `.git` 组件；保留 UTF-8 正常路径和多行消息。上限为 128 条路径、每条 1024 字节、消息 4096 字节。此过滤不是文件身份或操作系统隔离证明；Unicode、短文件名、链接、reparse point 等仍必须在 Host 打开资源时检查。
- `connectors/git_commit_evidence.rs`：`PreparedCommit::new` 调用上述预检并保存完整请求与 `RepositoryVersion`；`binding_parts` 为既有 `digest_arguments("git_commit", resource_id, ...)` 提供全部参数，使用现有长度前缀编码，不另造摘要算法。对象不是授权令牌。
- `recheck` 拒绝仓库/Git 目录身份、分支、HEAD、索引或选中文件清单内容版本变化。identity 和 SHA-256 必须由 Host 在资源锁内获取；模型不能提供这些事实。此函数本身不持锁，不能抵抗检查之后的外部改动。
- `parse_commit_object` 解析受限 UTF-8 原始 commit 对象，保留消息末尾换行；不接受缺失 author/committer、重复 tree、混合 OID 格式或非 UTF-8 encoding。`verify_readback` 比较独立记录的 tree、精确消息、父提交、变更路径及当前仓库/分支/HEAD。允许批准范围中存在未变文件，实际变更必须非空且全部在范围内；同时支持 unborn branch 和 SHA-1/SHA-256 OID。

**真实调用者边界**：PreparedCommit 调用预检，测试调用准备/复核/对象解析/回读核验；尚无 Host 生产调用者。`tool_approvals.rs` 的模型工具表仍只有 Git 读取，未新增可执行 `git_commit`。测试夹具运行的 Git Commit 不等于产品已能提交。

## 后续接线顺序（未完成）

1. Host 以授权目录句柄固定 worktree/Git 目录身份，取得写入资源占用；检测裸仓库、嵌套仓库、worktree/gitdir 重定向、合并/rebase 冲突及索引状态。未支持状态明确拒绝。
2. 从真实文件取得选中路径的内容/模式/删除版本清单及索引 SHA-256，构造 PreparedCommit；绑定用户审核的完整消息、路径与版本。由现行执行策略/Core 决定批准、阻止或询问，不由模型、项目关联或职责赋权。
3. 核对当前权限及 authority revision，消费 Core Dispatch；持久保存 InFlight/attempt 和完整恢复材料后才允许副作用。锁内 recheck，权限撤销/快照变化时不得启动。
4. 隔离并保留用户既有 staged 内容，使用 literal paths 和独立索引构建候选 tree；禁止 shell、hooks、filter/textconv、signing、自动维护等仓库/环境控制的程序入口。不能仅依赖 `git commit --only` 或 `git add` 后普通 commit。
5. 用父版本条件更新引用，处理文件/索引/外部 Git 并发；提交前独立保存候选 tree，提交后通过可信 Git runner 读取原始对象、完整 Diff 路径及当前 HEAD，调用上述核验。tree 不能从同一回读结果复制给 expected_tree。
6. 将 operation/attempt、原/新 HEAD、tree、范围、实际结果及正文持久保存后交还正式工具循环。超时、取消、回读失败、引用变化或落盘失败必须保留 UNKNOWN 并停止自动重放；恢复先核对真实结果。工具成功不自动完成任务或独立 Verify。

该顺序保留完整 P1 目标；未通过真实执行和故障注入前不得把 Draft 改为“Commit 闭环完成”。

## 本批验证

2026-10-26：Linux x86_64，临时部署 Rust 1.75.0/rustfmt 与 Git 2.51.1。仅对本 PR 两个无外部依赖模块使用真实源码构造模块 harness：

```sh
rustc --edition 2021 -D warnings --test commit_test_harness.rs -o commit-tests
./commit-tests
rustfmt --edition 2021 --check git_commit_policy.rs git_commit_evidence.rs
```

harness 内容为 `mod git_commit_policy; mod git_commit_evidence;`，与两份源码同目录；不替换依赖或业务实现。18 项通过、0 失败，其中一项在独立临时仓库实际 init/add/commit/cat-file/diff-tree，验证原始对象和范围拒绝，结束清理临时目录。路径和消息控制字符测试已在旧实现复现失败，再修正通过；无 author/committer 对象也已复现误接收后修正。

未执行：Cargo workspace/严格 Clippy、正式 Linux Tauri Host、真实模型工具回合、Windows 构建/运行/安装。当前执行区无完整仓库依赖与项目所需工具链；1.75 模块检查不能冒充项目 1.99.1 的全量证据。未运行 Actions，提交使用 `[skip ci]`。

## 后续增量：Core 审批绑定与真实对象读取（2026-10-26）

新增 `git_commit_binding.rs`：`git_commit_intent_digest` 复用现有长度前缀 SHA-256，绑定绝对仓库目录及 PreparedCommit 的所有参数；`verify_git_commit_dispatch` 验证真实 Core Dispatch 的 Write / git_commit / resource / expected HEAD / digest 后，再比较当前 RepositoryVersion。普通 Git 读取 Dispatch 不能作为 Commit 许可；消息、范围、目录、父版本或当前快照变化均拒绝。对象类型、错误及服务函数已通过 crate 根导出，Host 后续可调用；不把目录存在性检查当成句柄身份验证。

新增 `git_commit_reader.rs`：`read_approved_git_commit_object` 用同一 Commit 审批校验，读取已派发操作的原始提交对象；后置读取不能要求旧 HEAD 仍未变化。Git 程序由可信 Host 环境配置选择绝对路径，不接受模型传入的程序；OID 只接受小写 40/64 位十六进制，不接受 HEAD、选项或 revision 表达式。清空继承环境，禁用对象替换、可选锁、lazy fetch、fsmonitor、hooks、自动维护及传输协议；不支持 `--no-lazy-fetch` 的 Git 版本明确失败，不回退到可能取回远端对象的命令。复用既有 PipeReader/CaptureReader 的有界输出和可终止读取线程，15 秒截止，失败路径终止/回收直接子进程；超出 64 KiB、输出不完整、非零退出或对象解析失败均不形成成功证据。

读取后独立计算 Git 对象哈希 `commit <原始字节数>\0<原始正文>`（SHA-1 或 SHA-256）并匹配请求 OID，再解析正文。真实 Git 回归已复现“标签 OID 被剥离成提交”及“损坏 loose object 路径返回其他正文”两个误接收案例，现均拒绝。此处是 Git 既有对象格式校验，不使用 SHA-1 生成新的安全授权摘要。

**调用边界仍保留**：Core 绑定器与对象读取器已有正式服务 API、互相调用及真实 Git/真实 Core 测试，尚未接入 `tool_approvals.rs` 的生产模型工具表。目录/Git 目录身份与选中文件内容版本仍必须由可信平台后端采集，当前没有完整采集器，不用普通路径 open 伪造身份。未实现资源锁、独立索引写入、条件更新引用、持久执行/回执及 UNKNOWN 恢复，所以仍不是可执行 Commit 闭环。

本增量重新部署 Rust/Cargo 1.99.1、rustfmt、Clippy，并从固定父提交恢复完整 Cargo workspace 文本源码及锁文件。上述历史 18 项单模块验证不再作为本增量的全部证据；本次按以下正式命令验证，结果登记在对应 PR/Commit：

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

新增 16 项 Core/Reader 测试，包括实际 Core 审批消费、请求篡改/五类版本变化、真实提交对象与 HEAD 推进、替换引用、损坏对象、标签剥离、超大对象、缺失对象、带成功普通取回对照的本地 promisor 仓库、超时后确认子进程不存在。超时终止测试仅在 Unix 执行，Windows 不外推。全量 fmt 首次发现原有 `wex-core/src/dispatch.rs`、`dispatch_leases.rs` 不符合当前格式，只做 rustfmt 整理，不改 Dispatcher 语义。

独立代码审查发现的对象 OID 身份问题已按失败回归→修复→通过处理，补齐其提出的 HEAD 推进、timeout 与 promisor 覆盖建议。Linux Tauri Host、真实模型回合、Windows 构建/运行/安装仍 NOT_RUN；提交使用 skip ci，不触发 Actions。

## 后续增量：句柄目录链及真实版本采集（2026-10-26）

`wex-local-resources::read_relative_file_snapshot` 新增独立入口，保留旧的直接子文件 API。先按现有 Core 目录授权、保护目录及 Host internal roots 检查，再逐层通过 `openat` / `NtCreateFile RootDirectory` 打开目录；不把通过路径预检当成随后普通 path open 的许可。每个目录拒绝 symlink/reparse，限制同 mount/volume，保留完整句柄链；读后重新在同一父句柄下打开各目录名比较身份。保护目录绑定和 Host internal root 的身份也参与句柄检查，防止目录交换后绕过路径保护。返回精确文件字节、Unix owner execute 位及根到父目录的持久身份 stamp；不是后续写入令牌。

`git_commit_snapshot.rs` 提供 `GitCommitSnapshotSource`、`prepare_git_commit_from_registry` 和 `observe_prepared_git_commit`。来源是 Host 持有的注册目录及当前可信 grants；读取 `.git/HEAD`、本地 loose branch ref、真实 index 和请求明确列出的文件，绑定 worktree/Git 目录身份、分支/父版本、索引原始字节以及每个文件的路径/内容/owner execute/父目录身份。两个完整采集结果一致才返回观测。没有使用模型提供的 identity 或内容摘要，也不把双次一致采集称为原子的多文件快照。

`verify_live_git_commit_dispatch` 将采集器接到上一批 Core Commit 绑定器：先检查选中文件当前写入许可，重新采集版本，再从 registry 导出真实根路径并验证 Dispatch 摘要与快照。真实 Core OperationLedger 消费批准后的测试覆盖原样通过、文件变化拒绝、降为只读拒绝、撤权拒绝。Host 调用时仍须持有实时授权/资源占用；函数不执行写入、不自行持久化 InFlight，也不能阻止不合作的外部 Git/文件修改者。

当前采集器的明确支持切片是**普通 `.git` 目录、本地 symbolic branch 的 loose ref、已有 index 和已有选中文件**，每个文件/元数据沿用平台后端 64 KiB 读取上限；索引仅取得真实字节版本，不宣称已完成完整 Git 索引/仓库语义验证。packed refs、unborn/缺失索引、删除项、较大索引/文件、gitfile/linked worktree/子模块、Git 配置及 core.filemode 等执行语义仍需扩展与验收。Windows 没有 Unix owner execute 位，返回 false；最终提交模式须结合 Git 索引/配置处理。以上是当前代码边界，**不缩减 0.1.1 本地开发工具目标**，不向模型暴露尚不可完整执行的 Commit。

新增验证包括：真实嵌套二进制字节、授权优先于缺失路径探测、链接/硬链接/保护边界、internal exclusions/大小限制、Unix 并发 symlink 父目录交换、句柄身份识别被改名的保护/internal 目录、真实 Git 文件/索引/分支变化和 `.git` 目录更换、原生 owner execute 0654→0754 的 Git 模式变化。独立审查指出的保护目录身份遗漏和 execute 位错误均先取得失败回归，再修复。

本批使用正式 Rust/Cargo 1.99.1 工作区执行全量测试、严格 Clippy 与 fmt；对 `wex-local-resources` 增加 `cargo check -p wex-local-resources --all-targets --target x86_64-pc-windows-msvc --locked` 交叉类型检查。该命令编译检查 Windows 后端和测试源码，**没有链接或运行 Windows 测试，不代表 Windows 行为、Tauri Host 或桌面验收通过**。最终测试数量和提交 SHA 见 PR #63。仍待资源锁、独立索引/真实提交/引用条件更新、持久回执/UNKNOWN 恢复以及正式 Host 工具入口；不触发 Actions。

## 后续增量：共享占用与持久准入（2026-10-26）

新增 `git_commit_admission.rs`，复用既有 Core `ResourceLeases` 与 `wex-workspace-store::OperationStore`，没有再建第二套审批账本。桌面服务增加对现有 workspace-store crate 的本地依赖，锁文件仅增加该已存在依赖关系。

`git_commit_resource_id` 按操作系统和注册根的物理对象 stamp 计算稳定资源 ID，不包含会话、Agent 或目录授权 ID；同一物理工作树即使在不同会话中重新注册，也不能得到独立的写入占用。新入口的 Intent 必须使用此物理 ID，不能自动沿用旧任意 resource_id 的审批。

`begin_git_commit` 先检查当前 Write 许可并取得仓库级共享占用，再按预期 revision 读取真实操作账本。相同物理资源若存在任何 Write 的 InFlight/Unknown，拒绝新的派发；随后核对存储的 capability/resource/父版本/摘要及当前文件版本，通过现有 OperationStore.begin 的 revision/批准/authority 校验并持久保存 InFlight，才进入后置复核。后置文件/权限复核失败时不返回许可，尽力保留 Unknown；即使该状态写入失败，既有 InFlight 仍阻止自动重派。最后再次读取账本，要求 generation、意图及 InFlight attempt 一致，拒绝已被恢复流程隔离或服务会话已替换的尝试。

成功返回非 Clone 的 `GitCommitAdmission`，持有共享 lease、已持久化的 CommittedDispatch 和 PreparedCommit。Host 须一直保留它直到外部结果结算。释放内存 guard **不会清除持久 InFlight/Unknown**；重启、新建授权 ID 或新建内存 lease registry 也不能作为重试依据。旧未知结果必须先核对外部事实，再由受信任恢复流程写入结论。本入口本身不调用 Git 写入命令，不生成虚构成功回执。

**Host 并发契约**：从准入到实际派发持有实时授权所有权；所有协作 worker 共用同一资源占用注册表和 OperationStore 会话；结果结算、隔离及恢复流程也须协调同一仓库 lease，不能在新许可返回后随意变更该尝试。最后一次读取不是对后续状态变化的锁。当前协议只协调同一物理工作树的此类准入，不是任意外部应用的 OS 锁，也尚未作为所有文件/终端工具及重叠/嵌套工作树的统一资源锁。

**实际平台缺口**：本批检查发现，现有 OperationStore 所依赖的 OrganizationStore.open_named 明确拒绝非 Unix。因此本批是域协议和 Linux 持久准入链的验证，**不能据此宣布 Windows 持久执行可用**；Windows 的私有存储后端/适配仍必须完成。未弱化旧账本的权限或恢复检查，也未把 Unix 实现定为 Windows 最终存储方案。Windows 上缺少可用账本时不返回准入许可，不回退到仅内存状态。

新增测试覆盖物理根跨授权 ID 共享、InFlight 真实落盘后才返回许可、占用互斥、drop 后持久阻止重派、冷重开后 Unknown 与新授权 ID 仍隔离、旧文件/意图/authority 不消费审批、账本锁拒绝不返回 Dispatch，以及准入最终阶段的 Unknown/会话替换拒绝。独立审查发现的最终持久尝试状态缺检已以实际 OperationStore 阶段回归先失败再修复。初始测试数据目录不符合旧账本私有权限要求，测试改用由账本自行创建的私有子目录，未放宽生产规则。

实际执行 Rust/Cargo1.99.1 的正式 workspace test、严格 all-targets Clippy、fmt 与 diff 检查；具体数量与提交 SHA 记在 PR #63。Windows 持久准入、Tauri Host、真实模型回合和实际 Commit 写入仍未运行/接通。下一步仍需完成 Windows 持久适配、实际 Git 写入与条件更新引用、资源协议向其它写工具接线、可信回执和 UNKNOWN 核查恢复；不向模型暴露未闭环的提交工具，不触发 Actions。

## 后续增量：可信回读后的持久成功结算（2026-10-26）

`settle_verified_git_commit` 将既有纯证据校验连接到 OperationStore 的成功终态。它借用当前 `GitCommitAdmission`，在整个调用期间继续保留仓库 lease；先验证父提交、独立预期 tree、消息、完整变更范围和当前仓库/分支/HEAD，再读取持久账本，要求 generation、完整 intent 与 InFlight attempt 一致。以该最新 revision 调用现有 Finish/Succeeded，确认保存成功才返回不可自行构造的 `GitCommitReceipt`，包含账本 revision/generation、原操作/尝试 ID、提交 OID 和 `git_commit_<oid>` 引用。允许其它无关操作推进账本 revision，不允许覆盖该尝试的新状态。

这是 **Host 内部可信输入接口**：expected_tree 必须在写入前从独立精确范围索引取得；CommitReadback 的对象认证、全量变更列表和当前物理仓库观察由受信任 Host 提供，不能取自模型文字、工具参数或直接照抄待比较对象。该函数不会自己运行 Git、认证原始对象或采集变更列表，也不取代现有对象读取器。实际 writer 和这些读取步骤尚未串接，不能宣布可执行 Commit 闭环完成。

错误证据、不可用账本、过期服务会话、已转 Unknown/终态的尝试均不返回成功；错误后绝不据此重新执行 Git 写入。结算接口不自动对冷启动 Unknown 作出恢复结论。内存 lease 由调用方在结果处理后释放；持久隔离不由 drop 清除。成功后重复调用被拒绝，不产生额外终态 revision。账本当前只持久化提交 OID 引用与原操作意图，**并未持久化完整回读/验证载荷，也未接入对话工具回执或 AI 续答**；独立证据保存和恢复查询仍待完成。

本批新增 5 项 Unix 结算测试，以合成可信回读夹具验证错误证据不改变账本/占用、成功落盘与冷重开保留终态、无关 revision 变化、重复结算拒绝、锁不可用、残留临时快照拒绝及冷重开 Unknown、恢复流程提前隔离和服务会话替换。测试不冒充真实提交写入，残留临时快照用例也不冒充底层 fsync 故障注入。实际命令与数量记在 PR #63。工作区曾被维护清理，本批从固定远端 17f4920 恢复源码并重新安装工具链；旧测试结果未套用到新代码。

Windows 原生私有权限、替换/同步语义和持久后端仍是阻塞；未移除 Unix 限制或回退内存账本。下一步继续完成真实执行、可信证据持久载荷、Windows 存储适配与 Host 接线；Windows/桌面/真实模型验收均待执行，保持 Draft 和 `[skip ci]`。

## 后续增量：认证 tree 变更与实际回读结算（2026-10-26）

新增 `git_tree_evidence.rs`。`read_approved_git_commit_changes` 先核对批准摘要，读取并认证候选提交、批准父提交和两侧所有 tree 对象，从二进制 tree 条目独立构建文件清单，比较 `(mode, blob OID)` 得到完整变更路径。包含内容、执行位、新增和删除；重命名作为两端变化，不能漏掉未批准的旧路径。拒绝父版本不符、空变更、超出批准范围等结果。`GitCommitChanges` 字段私有，只能经读取器产生；它仍不证明当前物理仓库/分支/HEAD 或独立预期 tree。

复用原有有界子进程捕获器，固定 `cat-file commit/tree` 类型和十六进制 OID，保留干净环境、禁 replacement/lazy fetch/协议/维护的设置。每份原始对象独立按实际类型重新计算 SHA-1/SHA-256，拒绝 Git 读出的内容与请求 OID 不符，包括 tree 被替换或损坏。候选、父提交和 tree 读取共用 15 秒期限；两侧合计最多 64 次 tree 访问、深度最多 32、每份原始对象最多 64 KiB、每侧最多 4096 个文件、路径最多 1024 字节。遍历重复共享 tree 也计入预算，不能用树形 DAG 扩大无界工作量；超限拒绝，不回传部分路径。

当前 tree 支持普通目录与 `100644/100755` 文件，拒绝符号链接、gitlink、非 UTF-8/歧义名称和大小写别名；即使这类条目未变化也会拒绝整份清单。未读取 blob 内容，不代替 blob 完整性检查或 Git fsck；清单只包含文件，人工构造的空子目录条目不单列为路径变化，仍须以写前独立预期 tree 约束完整结构。该有界切片不缩减产品对真实仓库的最终支持目标，后续须扩展后再向模型开放。

新增 `read_and_settle_git_commit`，把真实读取器接到既有准入和持久成功终态：确认当前持久尝试，核对授权源物理 resource，从 registry 导出真实根，采集当前仓库/文件版本并要求 HEAD 指向候选 OID，读取认证对象和全量变更，再次采集并要求前后观察一致，最后构造回读交给既有树/消息/范围/身份校验与 revision CAS 结算。Host 只需提供程序配置、候选 OID 和**写入前独立取得的 expected_tree**，不再手工拼装消息、parents、变更列表与当前仓库身份。前后观察不是原子快照或 OS 锁，继续遵守共享 lease/实时授权和外部修改者限制。

本批测试包含二进制 parser 的 RED→GREEN、真实 SHA-1/SHA-256 tree、重命名两端、仅执行位变化、替换引用、损坏 loose tree、两侧共享遍历预算。另有 Unix 真实仓库夹具：测试代码先用独立 index 取得 expected_tree，再创建 commit 对象、条件更新分支；验证 HEAD 未指向候选时拒绝、预期 tree 不符时拒绝且保留 InFlight、正确结果经真实对象/tree/registry 回读后持久成功，真实工作索引未改动。**写入命令仅在测试夹具中，不是产品 Git writer**；候选产生、独立索引执行约束、完整证据载荷、Windows 后端、正式 Host/模型工具与对话续答仍待接线。实际全量命令和数量登记 PR #63，不触发 Actions。

## 后续增量：批准字节的独立树规划（2026-10-26）

新增 `git_tree_plan.rs`，将测试中外部提供的 expected_tree 推进为可复用的生产准备步骤。`prepare_admitted_git_tree` 要求现有持久准入和实时 Host 授权/仓库占用，认证父提交全部 tree，从父清单仅覆盖批准文件的原始字节和 Unix owner execute 模式，按 Git 目录名带 `/` 的排序规则编码 blob/tree 并计算 SHA-1/SHA-256。构造完全在内存中进行，**不写对象库、真实 index 或 ref**，不把未批准的暂存变化并入目标树。无变化返回 GIT_NOTHING_TO_COMMIT，不伪造一次成功提交。

采集器新增内部 `read_prepared_files`：由**同一次原生读取**同时返回文件 bytes/mode/父目录 stamp 和版本摘要，要求该摘要与 PreparedCommit 一致，再做稳定版本复核。避免只在前后采样，却将中间未批准字节送入 blob。规划前后复核当前 Write 许可、真实仓库版本和持久 generation/intent/attempt；检查失败不返回计划，不释放持久 InFlight。由于准入已先落盘，无变化或规划失败也不会自动改成“未执行成功”或清除隔离；后续 Host 必须按确证的未派发事实处理，不得盲目重派。

父清单先重新编码并要求根 OID 与认证父树完全一致，拒绝清单不能表达的空子目录、非规范结构等，不能默默丢弃。继承既有 4096 文件、1024 字节路径、32 层目录、每 tree 64 KiB 上限，并限制父/目标两侧合计 64 次 tree 访问及计划对象总量 16 MiB；文件/目录前缀冲突和大小写别名拒绝。此规划不读取或证明未变化 blob 的可用性，不是 fsck。

`GitCommitTreePlan` 私有字段绑定原操作/attempt/generation/intent digest/父提交，并提供只读树 OID 和待持久化对象数据。`read_and_settle_planned_git_commit` 先核对计划属于当前准入，再用计划里的独立预期树完成既有真实回读/持久结算，减少任意字符串当作 expected_tree 的调用机会。计划是数据，不是写入许可；未来对象写入、提交对象创建、条件更新 ref 和完整证据保存仍需受同一派发协议约束。

**执行语义边界**：这批明确为 Unix 原始文件规划切片，不运行 Git clean/smudge filters、attributes、编码或行尾转换，也不实现 core.filemode 等完整 Git 配置语义；不能把它直接当作普通 git add 的替代品或已批准的最终 writer 策略。Windows 的模式和私有持久后端仍未支持，本入口非 Unix 拒绝。unborn、删除、gitfile/linked worktree、symlink/gitlink 等旧缺口仍保留，尚不对模型开放。

新增测试包括纯树排序/模式、前缀/别名/非法 OID、深度/访问次数/对象大小拒绝，以及真实 SHA-1/SHA-256 准入：已存在未批准暂存变化且选中文件配置必定失败的 required clean filter 时，原生规划不运行该过滤器，原始二进制/CRLF 字节与 Git hash-object --no-filters 结果一致；规划后 index、HEAD、对象库逐字不变。随后只有测试夹具持久化计划对象、创建提交和条件更新 ref，再通过计划绑定的真实读取器结算成功；提交不带入未批准暂存内容，真实 index 仍原样。无变化、批准后文件变化和降为只读均不产生可用计划。初始夹具曾在配置失败过滤器后执行 git add 导致索引刷新触发 filter，已调整夹具顺序，未改变生产实现以绕过失败。具体全量结果登记 PR #63，生产 writer/Windows/Host/模型及对话续答仍待完成，不触发 Actions。
