# WEXPC 架构整理与组织基础施工

依据：用户本轮明确调整优先级，先按架构整理仓库、建设核心基础，侧重 Space/Context/轻项目；完整 Agent 工具施工仍保留，不作为本批前置。

实际差距：桌面服务已接线，但 model/service/api 平铺，组织关系未实现；旧组织快照 Store 是参考资产，直接双写会与当前 SQLite 权威存储冲突。

## 本批实施

1. 将实际代码按 domain、application、connectors、infrastructure 职责组织；Tauri 保留 Host，React 保留视图。不建立空模块冒充架构完成，不批量重命名已有 Cargo 包。
2. 新桌面状态保存可扩展命名的 Space、可选 Group、通用 Context，以及独立的组织位置；项目可显式关联 Context，普通 Context 不必成为项目。组织移动不变更 Context ID/资源、成员、工具授权或外部文件。
3. Rust 服务和 Tauri 提供创建 Space/Group/Context、移动 Context、关联/解除项目 Context 的基础接口；UI 后续细化，本批不宣称完整 Space 页面已交付。暂不实现未确定删除/级联规则。
4. 默认空字段兼容首批旧快照，不自动猜测个人/工作/公共分类或旧项目的 Context。当前主要组织位置是一条独立 Placement，未来可迁移；不是通用对象图冻结。

## 验证

- 创建未分组 Context、跨 Space/Group 移动和重启，保持对象、资源和项目关联。
- 错误分组归属/无效项目关联不能部分落盘，修订号不前进。
- 首批旧 SQLite 快照无损读取；不凭迁移自动创建类别或 Context。
- 现有项目/成员/对话/API/结果恢复测试保持通过；Rust 格式/clippy、前端构建和按影响的 Windows Host 检查合批运行。

未覆盖：完整 Space UI、组织删除规则、全量对象关系编辑、多位置、多用户共享、工具权限接线、并发施工、验证审查及模拟器。
