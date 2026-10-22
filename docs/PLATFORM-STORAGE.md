# Windows 存储后端

`wex-access-store` 保存的是授权记录，因此它的目录/文件必须**只对当前用户**可读，替换快照必须**原子**。两套平台语义如下，均已实现。

| 能力 | Unix | Windows |
|---|---|---|
| 根目录 | `DirBuilder::mode(0o700)` | SDDL `D:P(A;;FA;;;<当前用户SID>)` 生成安全描述符，`CreateDirectoryW` 传入 |
| 私密文件 | `OpenOptionsExt::mode(0o600)` | 同一 SDDL 描述符，`CreateFileW` 传入 |
| 权限校验 | `mode() & 0o077 != 0` 即拒绝 | `GetNamedSecurityInfoW` 取 DACL，逐个 ACE 检查 |
| 原子替换 | 写临时文件 → `rename` → 目录 `sync_all` | `MoveFileExW(REPLACE_EXISTING \| WRITE_THROUGH)` |

## 为什么没有用默认 ACL 凑合

删掉平台判断、让文件以继承 ACL 创建，代码能跑、测试也容易过，但“只有当前用户可读写”这条性质会**静默消失**——而这正是 `Store` 的前提。按 WEX 的规则（不得为跨平台削弱现有安全语义），必须显式实现。

## Windows 的权限校验具体在查什么

Windows 没有 `group/other` 位，判定规则是：

1. DACL 为空（NULL）→ **拒绝**（等价于 Everyone 全通）
2. 任何允许型 ACE 的 SID 属于 `Everyone` / `BUILTIN\Users` / `Authenticated Users` / `INTERACTIVE` / `ANONYMOUS LOGON` → **拒绝**
3. 其余情况通过（当前用户 SID、SYSTEM、Administrators 保留）

测试 `broad_ace_on_the_root_is_rejected` 用 `icacls /grant *S-1-1-0` 人为放宽权限，验证 `Store::open` 失败关闭。

## 已知差异（必须在契约里写明）

| 项 | 差异 |
|---|---|
| 崩溃一致性 | `MOVEFILE_WRITE_THROUGH` 只保证重命名元数据刷盘，**不等价于** Unix 的目录 `fsync`；Windows 上断电窗口略大 |
| 加锁 | 仍用 `grants.lock` 的独占创建（`CREATE_NEW`），与 Unix 同构；不是跨进程字节锁 |

## 测试

`crates/wex-access-store/tests/windows_store.rs`：

- `open_creates_a_private_root` —— 建根后重开，`verify_root` 仍通过（证明没有继承宽泛 ACE）
- `set_persists_through_atomic_replacement` —— 连续两次写入，第二次走 `MoveFileExW` 覆盖既有快照
- `broad_ace_on_the_root_is_rejected` —— 放宽 ACL 后失败关闭