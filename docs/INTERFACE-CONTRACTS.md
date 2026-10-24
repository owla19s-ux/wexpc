# WEXPC 工程接口契约

状态：工程D域的契约入口、版本登记与写作格式，建立于2026-10-25。不是产品需求替代品；本文件存在不等于接口已冻结、模块已实现或已验证。协作/分工/日志与裁决规则统一见[开发操作](DEVELOPMENT-WORKFLOW.md)。

## 登记与版本

| 契约ID | 当前版本/状态 | 生产者／消费者 | 依赖及实际状态 |
| --- | --- | --- | --- |
| API-PC-01 | DRAFT-1；执行边界与类型提案，尚未形成可编译契约 | 大肥鱼计划实现`wex-api`；Work负责Core/桌面适配 | 独立crate、类型/签名、可运行起点和双方采用记录均未完成；不得据此开始依赖未定签名的实现 |

版本与状态分开：`DRAFT-n`是草案修订，不是产品版本；首次本批采用后以`1.0`等契约版本登记。兼容扩展可递增次版本，破坏性变化必须给出新版本与迁移方案；这些编号只描述工程兼容关系，不改变WEXPC 0.1.1范围。每次引用同时带本文件所在Commit，不能只说“按最新文档”。

状态采用“草案 → 双方已核对 → 本批采用 → 已替代”。从“双方已核对”到“本批采用”需有生产者/消费者分别追加采用记录，明确版本、起点和影响范围；不涉及用户新增需求且无争议时由Work按已授权技术职责登记。涉及产品/权限变化或双方有分歧时先进入待用户裁决，不直接采用。

“本批采用”仅约束当前任务的接口，不是永久冻结。提出变化先在共同日志登记CHANGE并说明受影响任务，Work整合正文；大肥鱼不在自己的分支静默改同一份契约。旧使用方未迁移前保留准确的旧版本Commit与替代关系，不靠覆写正文隐藏历史。

## 每份可编码契约必须包含

```text
契约ID／版本／状态／文档Commit：
对应需求依据及适用批次：
生产者、消费者、维护者及文件范围：
数据结构：字段、类型、必填/可空、编码与单位、约束和含义
函数/trait或命令：准确签名、输入输出、调用与返回方式
事件：类型、顺序、正常终态、错误终态、重复/缺失事件处理
状态权威：谁写正式状态，谁只返回意图或事实，谁判定权限
生命周期：并发、超时、取消、在途结果、重试/幂等边界
凭证/敏感数据：谁提供、生命周期、不得进入哪些输出
兼容：旧版本使用方、变更影响、迁移与回滚条件
验证：成功/失败/边界用例、测试命令、目标环境、未验证项
实际接线：依赖、适配、配置、调用、结果与界面责任
双方采用记录：各自REC与源码SHA；未收到则填待确认
争议/用户裁决：关联REC，范围及当前状态
```

测试覆盖不等于双方采用，双方采用不等于真实服务/Windows通过。实现交接仍须按对应Commit报告证据；声明依赖、下载源码或演示调用不等于完整接入。

## API-PC-01 的现有依据与待补内容

已确认的架构边界：Rust服务＋Tauri/React；Core管理连接规则、权限、任务/操作状态与可信回执；API执行器负责请求、流式和协议事件，工具调用先作为意图交回应用层，不能自己授权或执行。Windows凭证由Host管理，独立API模块不依赖Windows Vault或桌面数据库。完整API产品目标继续保留，本批优先DeepSeek/OpenAI-compatible Chat Completions；成熟基础组件优先复用。详细复用来源与本批验收范围见[仓库结构](REPOSITORY-STRUCTURE.md)。

尚待双方技术核对并填写：公开类型与签名、同步/异步及事件消费方式、流终态与工具参数聚合、取消/超时分级、错误类型与状态映射、重试边界、代理配置、资源上限、reasoning续传隔离、依赖选择和手机测试命令。当前没有已收到的双方反对意见；这些是尚待补齐的技术项，不伪造为用户待决争议。

可运行起点由Work准备并登记实际Commit，生产者确认接口与手机可测条件后再接手。契约具体内容、代码实现与共同日志各司其职；下方DRAFT-1提供待核对的签名与语义提案，不冒充已采用或已实现。

## API-PC-01 / DRAFT-1：执行边界与类型提案

状态：Work技术提案，尚未收到大肥鱼核对/采用，不替代正式产品基线。以下名称用于准备可运行契约起点，不表示执行器已经实现；有不同意见先CHANGE/待决，不同时按两套接口实现。

### 调用方式

建议使用异步执行器，网络等待不占用桌面数据库锁。协议层通过有界事件通道向消费者返回事件；取消令牌由Host/应用层持有，调用取消不等待下一片网络数据。具体成熟HTTP/SSE库先检查手机目标兼容性，依赖与锁文件由Work统一登记；不自行实现HTTP栈。

拟公开入口：`Executor::start(request: ApiRequest, credential: Credential, options: RequestOptions) -> Result<Generation, ApiError>`。此函数只进行本地参数检查并启动请求，成功返回不代表服务端已接受或回复完成。`Generation`包含`events`事件接收端、`cancel`取消句柄及`join`最终结束句柄；消费者必须观察最终结果，不以通道EOF判成功。具体通道/句柄的Rust底层类型在契约起点中明确后才作为可编码签名采用。

### 数据结构

| 类型 | 字段与约束 |
| --- | --- |
| ApiRequest | request_id、connection_id、conversation_id、model为非空String；endpoint为已校验URL；messages为有序Vec<Message>；tools为Vec<ToolDefinition>；parameters为JSON对象，不允许覆盖model/messages/tools/stream/认证字段 |
| Message | role为System/User/Assistant/Tool；content为可空String；assistant可有tool_calls，tool必须有tool_call_id；不能用普通assistant文字代替工具回执 |
| ToolDefinition | name、description、parameters（JSON Schema对象）；模型收到定义不等于应用层已经授权执行 |
| ToolCall | id、name为非空String；arguments为完整JSON对象；只在正常tool_calls终态验证后输出，片段不是可派发调用 |
| RequestOptions | connect_timeout、idle_timeout、total_timeout均为正Duration；proxy为System/Disabled/Explicit(URL)；max_frame_bytes、max_response_bytes、event_capacity为正整数；具体默认值与手机兼容行为需在起点明确 |
| Credential | Host提供的独立秘密值，只用于认证；禁止Debug/Serialize和进入事件、错误、普通数据库；drop不被当作可保证物理内存清零 |
| Completion | text、tool_calls、usage（可选）、continuation（可选不透明续传数据）；usage缺失不当作0，也不虚构费用 |

消息/工具结构允许后续扩展，不把首批文本消息当作永久多模态上限。完整Provider、参数/Descriptor、原生协议等目标仍按基线保留，首批未知参数应明确报UnsupportedParameter，不能静默丢弃。

### 事件与终态

事件建议为`TextDelta(String)`、`Usage(Usage)`、`Completed(Completion)`；失败通过`ApiError`结束，不发送Completed。一轮调用至多一个成功终态；消费者同时以join结果确认生命周期结束。文本增量供显示，不自动成为可重放的完整assistant历史。

- 主回复按choice index=0处理；重复主choice、非法角色、重复/冲突终态、终态后内容均协议失败；协议允许的usage尾帧例外需专门测试。
- 正常文本须finish_reason=stop；工具须finish_reason=tool_calls，并完成唯一ID、函数名及JSON对象参数校验。length/filter/EOF/[DONE]不能当完整成功。
- SSE处理任意网络分片、UTF-8边界、CRLF/多行data、空行和注释；帧/响应/事件缓冲必须有界。取消和总超时也必须在通道背压期间生效。
- Completed携带的ToolCall是意图，应用层复核当前连接/授权/范围后才可派发；协议模块不调用文件、终端、GitHub或更新任务完成状态。
- 工具续答由应用层组织下一轮请求；DeepSeek reasoning等续传数据与准确的连接、会话、生成及完整工具ID集合绑定，不能跨请求全局缓存串用。续传需要原样保留，但不默认作为可显示内容或日志。

### 错误、取消与重试

ApiError建议含稳定kind、可选HTTP状态及脱敏message；kind区分InvalidRequest、CredentialUnavailable、Authentication、PermissionDenied、RateLimited、ProviderUnavailable、Network、ConnectTimeout、IdleTimeout、TotalTimeout、Cancelled、Protocol、LimitExceeded、UnsupportedParameter。不得附服务端原始正文、认证头、工具参数原文或带凭证URL；429可带合法Retry-After提示，不自行重发。

取消必须解除网络/通道等待并结束本轮；应用层保留已经显示的文字，是否有外部工具在途及真实回执由Core另行管理。取消生成不等于撤销已经派发的外部写操作。请求发出后网络失败不声称“服务端未执行”。本批不自动重试生成；用户重发或未来重试策略须与应用层协商，不得连带重放工具写入。

连接/Endpoint只允许HTTPS，显式本机回环测试可HTTP；禁止URL内凭证、query/fragment携密钥及携认证请求重定向。代理配置错误明确报告，不悄悄忽略；Android系统代理不能凭设置项存在就声称支持。

### 起点与验收交接

Work下一步把本提案收敛为可编译类型/签名、确定通道/超时/上限默认值、工作区和模块测试起点；大肥鱼核对手机可用依赖及取消/背压行为后分别登记采用。上述未明确项意味着目前还不能把DRAFT-1宣布为可编码契约完成。

首批验收包括：正常文本/工具、分片与UTF-8、缺失/冲突终态、非法工具参数、HTTP及200内错误、取消/背压/超时、各项上限、禁止重定向、代理失败、会话/续传隔离、事件与错误脱敏。模拟服务、真实DeepSeek和Windows接线分别记结果；10月31日前只本地验证。现有非流式连接器在新适配取得证据前保持可用，不通过删除旧路径掩盖集成缺口。


## DRAFT-1 核对补记（2026-10-25）

主笔与起点责任固定为Work；大肥鱼负责手机可测性、接口反馈与交接后的实现，不能以超时自动改派。用户转交的手机实测证明reqwest 0.12/rustls及SQLite可在该环境运行；准确SHA/日志待其共同日志回执，不能据此冻结同步/异步选型或宣布Windows运行通过。

reasoning隔离职责明确：API不拥有全局/跨会话缓存，只解析并返回不透明续传数据及必要关联标识；应用层按连接、会话、生成和工具ID集合存储/核验，再显式提供给下一轮请求。协议模块验证单轮消息/工具结构，应用层测试串用拒绝与生命周期。此职责澄清等待鱼确认，不代签采用。工作区归属和blocking/异步分歧见[当前状态](../COLLAB/STATE.md)，依赖项在裁决前不实施。


可运行类型提案见[contracts/README.md](../contracts/README.md)。库本地编译及4项测试通过；尚无正式Executor签名和HTTP实现，不代表已采用或可正式交接。取消标记不等于网络取消已接线，JSON字符串不等于已校验，续传比较不等于应用层隔离已集成。
