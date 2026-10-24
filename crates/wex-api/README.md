# wex-api（API-PC-01 纯协议核，提案）

状态：**提案分支** `feat/api-protocol-proposal`，不是已采用模块，也不是正式交接。
依据：`contracts/api_pc_01.rs`（a600f2cd 类型提案）+ DRAFT-1 语义 + REC-6387016118 的分层反提案（Work 已在 REC-6387040600 接受）。

## 边界

- **做**：OpenAI-compatible / DeepSeek Chat Completions 的流式协议解析与终态校验。
- **不做**：连接、超时、代理、禁止重定向、取消对 I/O 的解除、背压、自动重试、工具执行、权限判定、续传存储。
- 零 I/O、零网络依赖、零全局/跨会话状态；唯一依赖 `serde_json`。
- 传输适配器（blocking 或 async）由 Host 实现，两者包同一个 `Decoder`，协议核不选边。

## 结构

```
src/types.rs    请求/消息/工具/事件/错误/续传/选项；Credential 与 Continuation 构造性脱敏
src/decoder.rs  Decoder::push(&[u8]) -> Vec<Event>；Decoder::finish() -> Completion
src/lib.rs      Cancellation 标记 + 导出
tests/decoder.rs 22 项纯字节流测试（无网络、无时间依赖）
```

## 用法

```rust
let mut decoder = Decoder::new(RequestOptions::default())
    .with_continuation_scope(scope)   // 仅当 provider 返回 reasoning 时需要
    .with_cancellation(cancellation); // 传输层取消标记

for event in decoder.push(&bytes)? {
    match event {
        Event::TextDelta(text) => { /* 仅用于显示 */ }
        Event::Usage(usage) => { /* 可后于终态到达 */ }
        Event::Completed(completion) => { /* 意图，不是已授权执行 */ }
    }
}
let completion = decoder.finish()?;
```

## 保证

- 任意字节分片（含 UTF-8 中间）不产生错误文本；不完整行留在缓冲，不提前解码。
- `[DONE]` 本身不是成功；必须有合法 `finish_reason`。
- `length` / `content_filter` 直接判失败，不当成完成。
- 重复/冲突终态、终态后内容、`[DONE]` 后帧都判协议错误。
- usage 尾帧（`choices: []`）允许出现在终态之后，且只发一次 `Completed`。
- 工具调用按 index 聚合；id/name 中途变化、index 跳号、参数非 JSON 对象都判协议错误。
- 帧、响应、事件批量、续传 payload 均有上限，超限即 `LimitExceeded`。

## 未覆盖

- 真实 HTTP/SSE 传输、取消解除、超时、代理、重定向拒绝（传输层）。
- 真实 DeepSeek/OpenAI 服务、Windows 运行、桌面接线。
- reasoning 续传的应用层存储与串用拒绝。
