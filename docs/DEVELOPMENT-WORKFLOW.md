# WEXPC 开发及 GitHub 操作

状态：当前开发 WEXPC 的工程规则（D 域）。依据 [PC 基线](PC-PRODUCT-AND-TECH-BASELINE.md)；旧 WEX 的 Android GitHub 操作指南仅作为历史来源。产品内项目主 AI、施工 AI、审查 AI 和 Dispatcher 属 M 域，不是当前开发工作的强制流程。

## 每批开工

1. 核对用户最新明确要求、PC 基线相应章节与原 WEX 文档的适用范围；标明已确认、讨论/候选、已实现、已验证和废弃。冲突指出具体位置，不能选择容易实现的一版。
2. 查 WEXPC 最新 main、开放 PR/分支、目标源码和相关 Actions。确认当前 HEAD 与工作分支、已有独有成果和真实差距；不得按旧交接总结重复施工。旧 WEX 只读追溯，不把改动提交到旧仓库。
3. 说明本批依据、实际差距、交付后用户可操作行为和验收方式。常规实现细节可自主决定；范围、权限、UI、模块选型与阶段目标不得暗改。长任务可分批，但保留完整目标和剩余项。
4. 代码变更核查实际依赖、配置、调用路径、权限、结果及 UI 接线；下载源码、声明依赖、模拟响应、单文件测试都不等于真实集成或完整能力。

## PC 仓库施工与 GitHub

- WEXPC main 是唯一当前产品仓库主线；原 WEX 只用于核对业务语义与历史证据。文档按用户已授权的范围可直接改 main；代码、权限、接口和运行路径的改动通常走主题分支与 PR。用户明确要求直接在 main 做某批时按其要求执行，记录提交与未验证事项。
- 继续既有 PR 前核对 base/head、开放状态、独有提交和 Actions；合并前检查最终 diff、对应提交的检查与未覆盖风险。不要强推覆盖 main；清理分支前核对未合入成果。Squash/merge 后 main SHA 与 PR SHA 不同，引用检查时要说清适用的树和提交。
- 文档直接修订权威正文与冲突入口，不新建平行规则。小批次工作在已有授权和方向内持续推进；需要用户决定的产品范围、架构或权限变化才升级，不要求用户每隔几分钟输入“继续”。
- 旧 WEX 开发期的 Android APK、签名、Flutter UI 及 0.1.1 阶段规则不再定义 PC 的施工/验收。PC 本地代码环境、Windows Host、真实 API 与 GitHub 连接及桌面交互分别按 PC 基线验收。

## Actions 触发与成本

| 变更或交付 | 默认检查 | 何时扩大验证 |
| --- | --- | --- |
| 仅 `docs/**` / Markdown | 不自动启动 runner；核对正文、链接、状态及 GitHub 回读 | 影响代码生成、工作流或运行契约时，按实际影响另跑 |
| Rust/Bridge/平台源码 PR | Linux fmt/clippy/test 与 Windows cargo/平台契约检查 | Windows 实际文件/进程/安装行为另做运行验收 |
| 仅迁入的 Flutter 参考资产 | 不作为 PC 必经自动门禁；需要复用或改动时手动运行参考 Flutter 检查 | 真正移植到 PC 后以 PC 调用路径和 UI 验证 |
| 工作流、依赖、桌面 Host、权限关键路径 | PR 相应自动检查，并审查触发范围与失败结果 | 按影响增加 Windows 构建、真实服务、安装与恢复验证 |
| 需要可安装 Windows 产品 | 明确版本、目标架构和签名/安装策略后集中构建 | Windows 真机启动、升级和数据保留单独验收 |

- `rust.yml` 仅对 Rust/Bridge、未来桌面应用、契约测试、Cargo 配置和工作流自身的 PR 自动运行 Rust Linux/Windows 检查，保留 `workflow_dispatch` 以手动跑完整检查；不对合并后的 main 再自动重复一遍。直接写 main 的源码变更没有 PR 检查，必须针对该 SHA 手动触发受影响验证并如实报告。Windows 安装包工作流目前不存在，不把 `cargo test` 称作产品构建。
- 路径过滤是整个工作流级别：一个 PR 同时改文档和受监控源码仍会触发。新增 PC 代码目录时须同步更新触发路径，防止漏检。GitHub 对被路径过滤或 `[skip ci]` 跳过的工作流，若它被仓库设为必需检查，PR 可能停留 Pending；因此不能把跳过当成“检查通过”。当前连接无法读取私有仓库保护规则（403），启用必需检查时需核对配置；必要时改为始终存在的轻量门禁及按条件跳过重任务。
- 先从仓库 Actions 运行列表、Job、失败日志查起，不反复索取 Run ID。区分未触发、排队、取消、环境失败、编译/测试失败。批次内按接口和功能集中提交、集中检查；在途新提交可能取消旧运行，不能用旧 SHA 的通过证明新 SHA。
- 旧 `rust.yml` 同时在 PR 和 main push 上跑 Linux、Windows 与 Flutter，导致一次合入重复耗时；本规则按 PC 优先把 Flutter 参考检查改为手动，main push 自动触发取消。GitHub 私有仓库托管 runner 用量计入账户额度，Windows 与 Linux 分别计费，失败或取消前已运行的时间也不可当免费。预算在 GitHub Billing 设置，仓库文档或工作流不能代替账户预算。
- 节省运行次数不降低必要验证。代码提交、自动化、真实 API/GitHub 操作、Windows 构建、Windows 实机各报各的 PASS/FAIL/NOT_RUN，证据关联实际交付 SHA；只有最终代码树相同或补充必要验证时才复用旧证据。

## 交付记录与失败处理

每批交代需求依据、用户能做什么、改动路径、Commit/PR、运行及链接、未执行项和剩余缺口。失败先读具体日志并修复确定原因，基础设施偶发故障再考虑重试；不为更新文档或“补齐绿色标记”重跑全套。密钥、私人数据和未经审查产物不得进入仓库、模型上下文、日志或 PR 文本。

## 反复出错的边界

| 错误模式 | 本仓库执行规则 |
| --- | --- |
| 把 Android 阶段删减与旧 UI 照搬 PC | PC 基线保持产品完整范围；旧 WEX 文档仅按已确认业务语义追溯。PC-0/PC-1 的分批建议仍待确认。 |
| 把开发任务（D）当产品内施工卡（M） | 当前可直接用 PR/Issue/工程交接开发；M 需要作为产品功能单独设计、接线和验收。 |
| 单仓库/单文件样例固化为 GitHub 功能上限 | 测试切片只证明对应切片；权限按 PC 正式设计核对，不从样例反推额外限制。 |
| 目录关联等同于 AI 授权，Token 等同于会话权限 | OS 权限、连接认证、资源关联、AI 授权、会话工具选择和执行策略各自判定；撤权应阻止后续调用。 |
| SDK/依赖/编译通过就宣布功能完成 | 检查正式对话/任务路径调用成熟组件、真实服务回执、回读核验及可操作 UI；真实操作与模拟分别记录。 |
| 单次工具调用当成持续开发闭环 | 区分任务、执行尝试、审查、验证、持久状态、重启恢复、未知结果和升级决策；不要求用户逐项输入“继续”。 |
| 用旧摘要代替查当前仓库，错误代码倒改需求 | 对照最新基线、源码、提交与 Actions；修正偏离的代码与入口，未决产品取舍再由用户定。 |
| 测试通过或 AI 自述代替真实验收 | 按用户行为验收正常、失败、撤权、状态保存和恢复；证据绑定版本，缺项如实报告。 |

旧指南来源：原 WEX 的 `docs/GITHUB-WORKFLOW-AND-ANDROID-DELIVERY.md`、`docs/DEVELOPMENT-RULES.md`、`docs/architecture/VALIDATION.md` 和 `AGENTS.md`。其中 Android APK、签名、0.1.1 权限简化及“Windows 后续”的条款**不适用于 WEXPC 当前施工**。

## 资料依据

- [GitHub 工作流触发与路径过滤](https://docs.github.com/en/actions/writing-workflows/workflow-syntax-for-github-actions)：路径匹配影响整条工作流的触发。
- [跳过运行的必需检查状态](https://docs.github.com/en/actions/managing-workflow-runs/skipping-workflow-runs)及[按条件跳过 Job](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/use-conditions-to-control-job-execution)：前者可能让必需检查 Pending，后者显示 skipped success。
- [Actions 计费](https://docs.github.com/en/billing/concepts/product-billing/github-actions)与[Runner 费率](https://docs.github.com/en/billing/reference/actions-runner-pricing)：预算和平台用量需按账户实际设置与账单核对。
- [手动运行工作流](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)：`workflow_dispatch` 用于明确需要的批次检查。
