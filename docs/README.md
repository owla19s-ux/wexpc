# WEXPC 文档索引

更新：2026-10-26。此页为**导航**，不新增产品承诺。入口：[项目说明](PROJECT-OVERVIEW.md) · [仓库首页](../README.md)。

## 一、建议阅读顺序

1. [项目说明](PROJECT-OVERVIEW.md) — WEX/WEXPC 定位、0.1.1 目标、关键职责与现状边界。
2. [PC 产品与技术基线](PC-PRODUCT-AND-TECH-BASELINE.md) — **产品范围与已确认规则的主要权威来源**；区分已确认项、建议和待验证项。
3. [项目共识](PROJECT-CONSENSUS.md) — PC/Android 关系、用户使用场景与工程/产品边界。
4. [0.1.1 施工计划](WEXPC-0.1.1-IMPLEMENTATION-PLAN.md) — 工作包、依赖和验收条件；实现状态不以计划代替。
5. [接口契约](INTERFACE-CONTRACTS.md) — 具体跨模块约定及草案状态。
6. [架构决策与待确认提案](../DECISIONS.md) — 连接/能力/组合执行的研究结果；**DRAFT 不等于已批准**。

## 二、按用途查找

| 用途 | 文档 | 作用 |
|---|---|---|
| 首版范围/长期方向 | [PC 产品与技术基线](PC-PRODUCT-AND-TECH-BASELINE.md) | 产品权威基线，后续版本建议不等于批准 |
| 执行批次与交付 | [0.1.1 施工计划](WEXPC-0.1.1-IMPLEMENTATION-PLAN.md) | 工程安排与验收 |
| 连接与调度架构 | [DECISIONS.md](../DECISIONS.md) | 决策登记与待研究设计，阅读状态标注 |
| 架构重复/边界审查 | [架构整合 V0.6](proposals/WEXPC-ARCHITECTURE-DEDUP-BASELINE-REVIEW-V0.6.md) | **审查提案**；仅已明确接受的内容由正式基线承接 |
| 代码目录与迁移 | [仓库结构](REPOSITORY-STRUCTURE.md) | 代码模块及历史资产职责 |
| 平台数据存储 | [平台存储](PLATFORM-STORAGE.md) | 存储/授权相关实现说明 |
| 团队共同理解 | [项目共识](PROJECT-CONSENSUS.md) | 非独立需求清单 |
| 接口设计 | [接口契约](INTERFACE-CONTRACTS.md) | 契约记录与状态 |
| 开发/GitHub/CI | [开发流程](DEVELOPMENT-WORKFLOW.md) | 工程规范与成本控制 |
| AI 施工入口 | [AGENTS.md](../AGENTS.md) | 仓库施工者快速指引 |
| PC 前端 | [Desktop README](../apps/desktop/README.md) | 当前桌面端代码入口 |

## 三、文档状态规则

- **基线/确认**：经过确认且写入正式产品基线的规则；改变产品范围需明确决策。
- **施工计划**：实现步骤与验收方法，不可自行改变 0.1.1 的最低目标。
- **契约**：以文件局部标记识别已确定或草案；不能单凭存在文件断言接口实现。
- **决策登记**：在 `DECISIONS.md` 内区分已确认方向、DRAFT 和待研究点；没有确认不自动晋升。
- **提案/审查**：`docs/proposals/` 的内容仅作为审查和追溯资料，不与基线并列为规范。
- **实现证据**：必须对应具体源码 SHA、平台、实际命令/环境和结果；Windows 未测就保留 NOT_RUN。

新增文档时，在此页添加导航并写明状态、权威来源和适用范围。发现文档冲突，应优先对照产品基线，登记待处理差异；不凭文档日期自动覆盖较早但仍有效的确认要求。
