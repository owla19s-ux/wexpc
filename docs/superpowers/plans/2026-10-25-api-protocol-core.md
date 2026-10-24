# API-PC-01 纯协议核 验证证据

分支：`feat/api-protocol-proposal`（基于 `01b190c4f8c99f7b7ac2d27091410c369c4fae6e`）
日期：2026-10-25  执行者：大肥鱼（手机端 DSH） 状态：**提案，未交接**

## 环境

```
Linux aarch64（Android，类 Termux 前缀 com.dsharnessmobile.shell）
rustc 1.99.1 (58cf8f9c3 2026-10-13) host=aarch64-linux-android
cargo 1.99.1 (5f94df478 2026-08-27)
rustfmt 1.10.0 / clippy 0.1.99 / clang 21.1.8 / Node v24.18.0
可写临时目录 $HOME/tmp（/tmp 不可写）
```

## 实际执行与结果

```sh
cargo test -p wex-api
# test result: ok. 23 passed; 0 failed

rustfmt --edition 2021 --check crates/wex-api/src/*.rs crates/wex-api/tests/*.rs
# exit 0

RUSTFLAGS="-D warnings" RUSTC_WORKSPACE_WRAPPER=$PREFIX/bin/clippy-driver \
  cargo check -p wex-api --all-targets
# exit 0，0 警告

RUSTC_BOOTSTRAP=1 cargo check -p wex-api --all-targets \
  --target x86_64-pc-windows-msvc -Z build-std
# exit 0 —— 纯协议核可做 Windows 类型检查（无 ring/rusqlite）

cargo test --workspace --no-fail-fast
# 289 passed / 3 failed；3 项均为已知 Android 环境限制，非本次改动引入
#   fifo_registration_is_rejected_without_opening（current_exe -> linker64）
#   hard_link_alias_of_personal_file_is_not_accepted_as_general_data（EACCES）
#   symlink_directory_and_hard_link_cannot_be_read（同上）
```

## 设计要点（相对 a600f2cd 类型提案的补强）

0. 自审（智谱 glm-4.7）后修正：`[DONE]` 之后继续消费缓冲，后续 data 帧立即判协议错误；工具调用数量上限收紧为 64，去掉 off-by-one。

1. `Decoder` 状态机：SSE 分片、UTF-8 边界、多行 data、注释、`[DONE]`、index 聚合、终态、上限。
2. `ApiRequest::validate()`：必填字段、消息角色/工具字段一致性、工具参数必须是 JSON 对象。
3. `validate_parameters()`：禁止覆盖 `model/messages/tools/stream/authorization/api_key/key`。
4. `ApiError` 增加脱敏 `detail` 与 `retry_after`；不使用服务端正文。
5. `RequestOptions` 明确默认值（连接 20s / 空闲 60s / 总计 300s / 帧 1MiB / 响应 16MiB / 事件批 1024 / 续传 256KiB）。
6. `Proxy::explicit()` 构造期校验（拒绝 userinfo、空白、非白名单 scheme）。
7. `Continuation` / `Completion` 手写 Debug 且不派生 Serialize；payload 有上限。
8. usage 尾帧（choices 为空）允许出现在终态之后，且 `Completed` 只发一次。

## 与 Work 文件的唯一交叠

根 `Cargo.toml` 增加了一行 workspace 成员（`crates/wex-api`）。这是 Work 所有权文件；若由 Work 自行登记成员，可直接丢弃该行，crate 本身不依赖它。

## 未验证 / NOT_RUN

- 真实 HTTP/SSE 传输、取消解除、超时、代理、重定向拒绝（属传输层，本提案未实现）。
- 真实 DeepSeek/OpenAI 服务。
- Windows 链接与运行（本机只能类型检查）。
- 桌面接线、Tauri、安装包。

## 复现

```sh
git fetch origin feat/api-protocol-proposal
git checkout feat/api-protocol-proposal
cargo test -p wex-api
```
