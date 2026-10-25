# wex-api（API-PC-01 纯协议核，提案）

状态：**提案分支** `feat/api-protocol-proposal`，不是已采用模块，也不是正式交接。
依据：`contracts/api_pc_01.rs`（a600f2cd 类型提案）+ DRAFT-1 语义 + REC-6387016118 的分层提案（Work 已在 REC-6387040600 接受）。

## 边界

- **做**：请求编码、SSE 流式解码与终态校验、错误分类、续传回放。
- **不做**：连接、超时、代理、重定向、取消对 I/O 的解除、背压、自动重试、工具执行、权限判定、续传存储。
- 零 I/O、零网络依赖；唯一依赖 `serde_json`。
- 传输适配器（blocking 或 async）由 Host 实现，两者包同一个 `Decoder` / `encode_request`。

## 结构

```
src/types.rs    请求/消息/工具/事件/错误/续传/选项；Credential 与 Continuation 构造性脱敏
src/encode.rs   encode_request()：body 组装、tools、parameters 合并、续传回放
src/errors.rs   HTTP 状态与 provider 错误体 → 稳定 ErrorKind；不回显正文
src/decoder.rs  Decoder::push(&[u8]) -> Vec<Event>；Decoder::finish() -> Completion
src/lib.rs      Cancellation 标记 + 导出
tests/decoder.rs 30 项纯字节流测试（含逐字节切分不变性）
tests/encode.rs  15 项编码与错误映射测试
```

## 用法

```rust
// 组装请求
let body = encode_request(&request, EncodeOptions::default())?;

// 解码流
let mut decoder = Decoder::new(RequestOptions::default())
    .with_continuation_scope(scope)
    .with_cancellation(cancellation);
for event in decoder.push(&bytes)? { /* TextDelta / Usage / Completed */ }
let completion = decoder.finish()?;

// 错误分类（传输层在收到响应后调用）
let error = error_for_status(status, retry_after);
```

## 保证

- 任意字节分片（含 UTF-8 中间）不产生错误文本；不完整行留在缓冲，不提前解码。
- `[DONE]` 本身不是成功；必须有合法 `finish_reason`；`length`/`content_filter` 直接失败。
- 重复/冲突终态、终态后内容、`[DONE]` 后帧、同一帧重复主 choice、重复工具 ID 均判协议错误。
- usage 尾帧允许晚于终态，且 `Completed` 只发一次。
- 工具调用按 index 聚合；id/name 中途变化、index 跳号、参数非 JSON 对象判协议错误。
- 帧、响应、事件批量、续传 payload 均有上限。
- 请求编码禁止覆盖 `model/messages/tools/stream/authorization/api_key/key`；续传回放校验 connection/conversation/model 与工具 ID 集合。
- 错误映射只输出稳定 kind 与静态文案，不携带服务商正文、凭证或工具参数。

## 未覆盖

- 真实 HTTP/SSE 传输、取消解除、超时、代理、重定向拒绝（传输层，属 Work 范围）。
- 真实 DeepSeek/OpenAI 服务、Windows 运行、桌面接线。
- reasoning 续传的应用层存储与串用拒绝。
