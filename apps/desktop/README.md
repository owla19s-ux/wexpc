# WEXPC 桌面首批

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
| Windows 凭证库与 Tauri Host | 已实现，待 Windows 编译和实机验证；Linux 无凭证后端时明确拒绝保存，不降级明文 |
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

本环境已执行 Linux Rust 全量测试、clippy、格式检查和前端生产构建。Host 本地检查受 Linux glib/WebKit/pkg-config 缺失阻挡；未宣称 Windows 可运行、凭证库可用或真实服务通过。桌面 Windows 检查只在相关源码/工作流 PR 或按需手动触发，验证 Host 编译与前端，不构建安装包。
