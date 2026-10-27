# WEXPC 归档代码回收筛选｜第一批（2026-10-27）

状态：**部分核查完成，尚未集成代码**。本清单仅用于工程 D 域，不改变正式产品基线或授予执行权限。目标主线：`main`；归档引用：`refs/tags/archive/wexpc-20261027/<旧分支名>`。**旧分支曾被归档删除，不意味着内容已进入 main。**

## 已读取和初筛

| 归档分支 | 旧分支相对现行 main | 本批初判 | 下一步 |
| --- | --- | --- | --- |
| `fix/pc-organization-revision-20261025` | 72 ahead / 580 behind；12 个文件涉及组织、任务记录和回执；旧版 `application/execution_receipts.rs` 在 main 同路径不存在 | **高优先级待提取审查**，不是直接整支合并 | 将旧执行者归属校验、重复 invocation 防护、任务快照及修订一致性与 main 现存回执/权限路径逐项比较；先补测试后迁移必要行为 |
| `fix/dispatch-parent-child-leases-20261026` | 7 ahead / 248 behind，`dispatch.rs`、`dispatch_leases.rs` 有差异 | **需要语义比较** | 核对父子资源覆盖、路径别名拒绝、申请原子性、释放和并发竞争；main 已有新版租约代码，禁止旧文件覆盖 |
| `feat/pc-dispatch-eligibility-batch-20261026` | 10 ahead / 367 behind；调度准入和租约源码在 main 已存在但不完全相同 | **新版已存在，需检查独有逻辑** | 比较失败场景与测试覆盖，保留已加强的新权限/调度约束 |
| `integration/pc-api-tool-reply-recovery-20261025-1932` | 1 ahead / 371 behind；旧 `saved_chat_reply.rs` 与 main 版本不同 | **新版已存在，需检查恢复语义** | 对比重启恢复、回执幂等、UNKNOWN 禁止自动重放和旧测试 |
| `fix/api-final-round-dispatch-guard-20261025` | 3 ahead / 391 behind，涉及 `tool_round.rs` | **待核对安全差异** | 审查终轮工具派发是否被拒绝，复用旧回归样例 |
| `fix/pc-sqlite-v5-upgrade-fence-20261025` | 1 ahead / 391 behind，旧 SQLite 文件有 2 行改动 | **待核对迁移安全** | 核对现行 schema 升级屏障与前后兼容，不回退新版本 |
| `fix/ui-github-credential-revoke-20261025` | 6 ahead / 391 behind，涉及凭证撤销、Tauri 与界面 | **待核对撤权** | 检查撤权后的后续调用是否仍可执行与敏感数据残留 |

## 明确的整合原则

1. 先核查现行 main 的等效功能和测试，再识别老分支独有且仍符合 0.1.1 基线的行为；候选状态不等于“需合并”。
2. 对于已经演进的生产文件只做必要补丁或回归测试迁移，不使用整支 merge、覆盖或未审核 cherry-pick。
3. 权限、凭证、SQLite 迁移、UNKNOWN 工具副作用和调度互斥列为高风险；未核验现行接口和测试前不修改运行行为。
4. 每一项回收完成后更新为 **已在 main / 已移植 / 不采用 / 待决定**，关联提交 SHA 和测试结果；未实际执行的标记 NOT_RUN。
5. 所有归档标签保留；待整个回收和主线审计完成后再考虑处置。

## 本批验证与未完成

- 实际检查：GitHub 比较接口读取 7 条高优先级归档分支的相对差异和文件清单；读取调度准入、租约、工具回复恢复的两侧源码，以及旧执行回执/部分旧任务记录测试。
- 尚未执行：对 43 个归档标签逐项完成等价/功能性审计；源码合入；编译、Rust/前端测试、Windows 实机测试（均 **NOT_RUN**）。
- 下一批先做“执行回执/任务绑定”和“调度资源范围租约”的新旧语义/测试对照，再决定具体的最小补丁。
