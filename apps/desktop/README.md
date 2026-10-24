# WEXPC 桌面首批

## 组织基础批

正式服务已按 `domain` / `application` / `infrastructure` / `connectors` 分责，保持 React → Tauri → Rust 的同一调用路径。Space 与上下文入口支持创建空间、可选分组、通用 Context 及移动位置；项目详情可显式关联/解除 Context。位置独立保存，移动不改变 Context ID/资料、不移动外部文件、不添加工具授权。

SQLite schema 从 1 升级到 2，保留原 payload，旧项目无 `context_id` 时保持未关联，不猜测类别；版本门槛防止旧二进制打开新数据后把组织字段丢掉。基础 UI 未冻结；组织删除/共享/权限继承、Context 自身对话绑定和完整对象图仍未实现。

正式路径：React → Tauri command → `wex-desktop-service` → SQLite / reqwest。普通 API 对话不调用 Flutter FFI。界面为基础功能入口，视觉与后续交互未冻结。

## Windows 启动

安装 Node.js 22 LTS、Rust stable、Visual Studio C++ 构建工具和 WebView2 后，在本目录执行：

```powershell
npm ci
npm run tauri dev
```

创建项目 → 在连接与服务中配置 DeepSeek/OpenAI 兼容 API 及多个模型 → 添加项目主 AI/施工/验证/审查成员 → 打开成员工作对话，或新建不依赖项目的普通对话。

每个成员拥有独立模型绑定和会话；请求附带当前项目目标、职责及分配给它的任务草案。任务卡可保存目标、范围、验收条件和多个成员。本批任务只保存草案，没有程序化派工或文件/终端执行，不能用 AI 的文字回答冒充实际施工、测试或审查。

API Key 保存在 Windows Credential Manager，SQLite 仅存不透明引用。数据位于 Tauri `app_data_dir`，目录由 Host 提供；不自动导入 Android 数据。`app.wex.pc` 为当前工程应用标识，安装包、正式签名及升级交付尚未验证。图标为技术占位资产，非最终视觉稿。

同一会话一次接收一个请求，不同会话的网络调用不持有全局数据库锁。重启后在途请求记为中断，不自动重发。API 已返回但落盘失败时可点击“重试保存结果”，只重试本地保存；关闭进程会失去尚未落盘的内存结果，重启后明确显示中断。

## 本批覆盖与缺口

| 项目 | 本批实际状态 |
| --- | --- |
| 项目、关联资料、成员/模型绑定、任务草案 | Rust 持久化与桌面命令/UI 已接线；Linux 服务测试覆盖恢复和关联校验 |
| 普通 API 文本对话 | Rust HTTP 请求/回复、错误、历史持久化已实现；本地 HTTP 测试覆盖请求格式，不等于真实 DeepSeek 验收 |
| 原 Rust Catalog | 正式服务调用其目录恢复、连接启停、模型/会话选择校验，无双写旧 CatalogStore |
| 原权限/操作生命周期和 Windows 文件后端 | 原样保留，等待本地工具批按接口接入；本批未绕过它们直接给模型文件/命令工具 |
| Windows 凭证库与 Tauri Host | Windows Host `cargo check`、服务测试与前端构建已通过；凭证真实读写和桌面实机运行尚未验证。Linux 无凭证后端时明确拒绝保存，不降级明文 |
| 完整 Agent/Dispatcher | 未实现工具循环、范围占用、并发派工、结果集成、自动验证/审查/修复流转 |
| GitHub、本地文件/Git/终端、Android 模拟器 | 未接通桌面正式路径，继续保留 0.1.1 要求 |
| API 流式、模型目录/参数细化、追加排队、语音、最终 UI | 后续批次；本批文本请求不代表完整 API 模块交付 |

## 验证

```powershell
# 仓库根目录
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
# 本目录
npm run build
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

2026-10-24 已验证代码提交 `1b5319a8b605a66f1be55f63da9100c087b77af6`：Linux 全量 Rust 测试 230 项、clippy、格式检查和前端生产构建通过；[Linux/Windows Rust CI](https://github.com/owla19s-ux/wexpc/actions/runs/40023876374) 与 [Windows 桌面 Host/前端检查](https://github.com/owla19s-ux/wexpc/actions/runs/40023876304) 均通过。Host 本地检查受 Linux glib/WebKit/pkg-config 缺失阻挡，Windows 编译证据来自上述 CI。

这份记录之后的文档提交不改变已验证的源码。未构建安装包，未调用真实 API 服务商，未验证凭证库真实读写或 Windows 窗口体验。桌面 Windows 检查只在相关源码/工作流 PR 或按需手动触发，不在本批构建安装包。
