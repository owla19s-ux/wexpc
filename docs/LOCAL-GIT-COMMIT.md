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
