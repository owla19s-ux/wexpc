# WEXPC 协作状态

更新：2026-10-25；维护者：GPT Work。此文件是状态摘要入口，不是第二套需求或流水账。具体变化/证据以[共同日志](https://github.com/owla19s-ux/wexpc/pull/4#issuecomment-6386453733)记录为据；接口以[契约正文](../docs/INTERFACE-CONTRACTS.md)指定版本为据。Work串行整合此文件，鱼在日志提交自己的变化，避免两分支同名文件互不相见。

| 工作 | 负责人 | 现在 | 下一步 |
| --- | --- | --- | --- |
| 项目目标/首版 | Work主笔，双方各自回执 | [共识说明](../docs/PROJECT-CONSENSUS.md)已交付；未代签鱼已同意 | 鱼阅读并回复理解/疑问 |
| 手机环境 | 大肥鱼 | [鱼ENV/VERIFY REC-6387015668](https://github.com/owla19s-ux/wexpc/pull/4#issuecomment-6387015668)：a600f2cd契约4/4通过；workspace 245通过/3失败（exit101）；全workspace Windows交叉check失败，缺Windows SDK头文件 | 保留失败及其Android环境归因；仅纯Rust子集交叉检查可用，不能称全workspace或Windows运行通过 |
| API-PC-01契约 | Work主笔，鱼核对 | DRAFT-1已有语义提案；[std-only类型提案](../contracts/README.md)已编译且4项测试通过，正式crate起点未完成 | 鱼直接核对可运行类型；Work继续准备适配，争议按下表处理 |
| wex-api实现 | 大肥鱼 | **NOT_HANDED_OFF；尚不能按未定接口开工** | 起点和契约采用完成后，Work登记交接SHA与明确开工信号 |
| Core/桌面/集成 | Work | 既有代码保持；业务源码91cafba有历史证据 | 不依赖争议的任务可继续本地施工 |

## 提案处理，不以沉默批准

- D-001：主笔明确为Work，不再写成双方共同主笔。DRAFT-1已提交；完整类型/模块起点仍由Work交付，超时不自动改派。
- D-002（双方方案一致）：鱼在REC-6387016118撤回独立workspace，接受主workspace；Work确认采用主workspace登记方向。交接前Work必须建好crate、依赖/锁文件及模块测试起点，鱼收到SHA后实测，未完成前不冒称可开工。
- D-003（双方语义一致）：API只解析/返回续传，应用层关联和核验。Work接受鱼补充的独立max_continuation_bytes上限（按字节拒绝、不截断）及续传/请求不派生Serialize、日志/Debug不暴露payload；校验代码待实现。
- D-004（核心分层方案一致，传输细节待定）：Work接受鱼的纯解码器＋传输适配器方向。协议核不依赖reqwest或blocking/async；负责确定性分片/聚合/校验与上限。传输层另行验证取消I/O、超时、代理和背压，不要求现在同时交付两套适配器；其具体形式/依赖待对应契约核对，不阻塞纯解码核。若后续仍有不同意见，登记待用户裁决，不以超时批准。

冲突需用户明确决定，不因24/48小时无回复默认实施。定时唤醒未由Work配置；不把它当已上线。PR创建/更新不是Actions，沿用skip与暂停规则，不额外全面禁止PR。

## 最近批次

BATCH：API可运行类型提案。DONE：请求/消息/工具/事件/错误类型、取消标记及续传关联比较；库编译无警告、4项本地测试通过。LEFT：鱼原始证据/回执、正式crate/依赖起点、输入校验/Decoder与传输接线。NEXT：Work建立主workspace协议核起点并补校验契约，随后交鱼实现协议核；传输由Work单独接线。BLOCK：正式crate尚未交付，不让鱼猜不存在的接口。

10月31日结束前不运行Actions，提交含`[skip ci]`；本地不能测则记NOT_RUN，11月1日后再评估。Windows交叉check不证明链接或运行通过；模型审查不替代实际测试。
