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
