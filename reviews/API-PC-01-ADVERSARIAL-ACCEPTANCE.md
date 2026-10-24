# API-PC-01 独立验收审查矩阵（审查输入，非新需求）

状态：REVIEW-ONLY / NOT_RUN。依据 `docs/INTERFACE-CONTRACTS.md` 的 DRAFT-1、`COLLAB/STATE.md` 的 D-002/003/004 与 `docs/PROJECT-CONSENSUS.md`。本文件不更改契约、分工或产品范围，不代表 Work / 大肥鱼已签收，不占用 `crates/wex-api` 施工范围。

## 交接前置条件（阻塞正式实现交接）

- [ ] Work 提交主 Cargo workspace 内可编译的 `crates/wex-api`，包括依赖、锁文件和模块测试入口，登记起点 SHA。
- [ ] 公开类型、函数签名、事件终态、资源上限默认值及拒绝语义在契约正文对应 SHA 登记；双方分别确认采用，争议不因超时视为同意。
- [ ] 纯解码器不依赖 reqwest / 异步运行时 / Windows Credential Manager / SQLite；HTTP 适配由 Work 独立接线。
- [ ] 测试证据明确区分 Android 本地、Linux/纯 Rust 交叉检查、Windows 编译链接、Windows 实际运行及真实 DeepSeek 调用。

## 可直接转为测试的对抗用例

| ID | 输入或情形 | 必须验证的结果 | 责任边界 |
| --- | --- | --- | --- |
| DEC-01 | UTF-8 多字节字符跨网络块，SSE 使用 CRLF、多行 data、注释及空行 | 按帧正确聚合，不提前解码或丢字节 | Decoder |
| DEC-02 | 只有 EOF 或 `[DONE]`，缺少有效 finish_reason | 协议失败；不得产生成功 Completion | Decoder |
| DEC-03 | `finish_reason=length` 或内容过滤；重复或互相矛盾的终态 | 不得当作正常成功；只允许唯一合法终态 | Decoder |
| DEC-04 | 工具参数跨块拆分，末尾不是 JSON 对象、重复 tool ID、空函数名 | 失败，不发布可执行 ToolCall | Decoder |
| DEC-05 | 多 choice 中出现重复主 choice、角色冲突、完成后仍有内容 | 按契约拒绝；不静默覆盖 | Decoder |
| DEC-06 | 帧、总响应、事件缓存分别越界 | 对应 LimitExceeded，内存保持有界 | Decoder / Adapter |
| DEC-07 | continuation 字节数恰好达到和超过 `max_continuation_bytes` | 边界允许，超限拒绝；绝不截断 payload | Decoder |
| DEC-08 | 同一个 continuation 被用于其他 connection / conversation / generation / tool ID 集合 | 应用层拒绝跨会话复用；协议层只提供必要关联数据 | Application |
| DEC-09 | 200 HTTP 内含服务商错误对象，或 401/429/5xx 含敏感正文 | 返回稳定错误分类；日志和错误文本不泄露原文、凭证或工具参数 | Adapter |
| DEC-10 | event channel 已满时取消，网络长时间不发下一块 | 取消及时解除等待；join 明确返回 Cancelled | Adapter |
| DEC-11 | connect / idle / total timeout 分别触发 | 错误分类明确，不能假报成功或自动重试 | Adapter |
| DEC-12 | 显式代理不可用、携凭证 URL、重定向到其他主机 | 拒绝/报告失败；不静默降级或泄露 Authorization | Adapter |
| DEC-13 | 已返回部分文字后发生网络中断 | 允许保留展示片段，但不得记为完成或可重放完整消息 | Application |
| DEC-14 | 生成取消时已有工具写操作在途 | 生成取消不自动声明写入回滚；Core 保留真实结果/未知状态 | Core |
| DEC-15 | 多个 Agent 同时收到工具意图，目标文件范围重叠 | 应用层授权及范围占用先于真实写入；API 不执行工具 | Dispatcher |
| DEC-16 | 工具定义或模型参数包含未知字段 | 明确 UnsupportedParameter 或契约规定的拒绝；不静默丢弃 | Request validation |

## 证据记录格式

每个用例记录：源码 SHA、执行命令、操作系统与目标架构、测试夹具、实际退出码、关键输出（脱敏）、PASS/FAIL/NOT_RUN、剩余风险。纯类型测试、模拟服务测试、真实 API 测试、Windows 安装运行应分别记录。

当前结果：**全部 NOT_RUN**。本文仅提供独立审查用例，不声称修复、实现、验收通过或大肥鱼已收到交接信号。

## 协作边界

Work 继续拥有正式 crate 起点、接口契约和桌面传输适配；大肥鱼在明确交接后实现已采用范围的协议核。本审查分支只包含独立验收材料；不修改 `COLLAB/STATE.md`、`docs/INTERFACE-CONTRACTS.md` 或 `crates/wex-api`。如发现新的权限/产品范围分歧，提交待用户裁决，不以测试便利为由改变基线。
