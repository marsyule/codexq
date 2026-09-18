# 当前工作与迭代追踪 (current.md)

本文档记录 CodexQ 当前推进中的工作、正在解决的问题及技术债（Current Truth）。

---

## 1. 当前版本状态 (v1.0.0)

- **核心功能完成度**：
  - [x] 多账号并发额度刷新 (`asyncio` 信号量调度 / Tokio 异步线程池)
  - [x] 基于官方逻辑的剩余额度百分比与重置时间解析 (Remaining-first)
  - [x] 智能自动收纳 `~/.codex/auth.json` 与历史备份 `~/.codex/backups/`
  - [x] 便捷账号引入与凭据导入（终端登录拉起 + UI/CLI `auth.json` 文件/路径无损导入）
  - [x] CodexQ Doctor 环境体检诊断中心（CLI 存在性、存储模式、SQLite WAL、网络连通性）
  - [x] OpenAI OAuth 2.0 自动续期与 401 响应式自愈协议 (RTR)
  - [x] 原子切号防丢保护机制
  - [x] SQLite WAL 存储与快照时间序列记录
  - [x] 账号别名修改、一键清空与全局重置
  - [x] 软删除回收站与物理粉碎支持
  - [x] Tauri 2 + React 19 桌面客户端及系统托盘集成 (纯 Rust 核心，零 Python 依赖单二进制)
  - [x] 全栈中英双语国际化 (i18n) 与系统托盘菜单动态切换
  - [x] 启动自动刷新与后台周期自动刷新（防并发冲突）
  - [x] 定时触发错峰闹钟流水线（$\ge 5$ 小时防重叠拦截，支持「仅一次」自动停用）
  - [x] JIT 到期动态自感知刷新与满血复活原生桌面通知
  - [x] 纯文本透明配置 `~/.codexq/config.json`
  - [x] 开源许可 (MIT) 与 GitHub Actions 跨平台 CI/CD 自动构建流程 (`release.yml`)

---

## 2. 正在推进的任务 (Active Work)

### 任务 1：Agent 友好型工程与文档治理脚手架 (Harness Engineering)
- **目标**：按照《Agent 项目文档治理》规范，建立可读、可导航、可执行、可自纠的研发环境。
- **状态**：已完成
- **检查清单**：
  - [x] 编写根目录 `AGENTS.md`（入口地图、硬性不变量与校验流程）
  - [x] 建立 `docs/` 规范知识库（`index.md`, `PROJECT.md`, `architecture.md`, `work/current.md`, `work/history.md`）
  - [x] 编写零宿主污染的标准库测试套件 (`tests/`)，提供 Agent 机械自纠能力
  - [x] 验证端到端冒烟测试与前端单元测试无破坏性

### 任务 2：Daemon 进程长效健康检测与自愈
- **目标**：优化 Rust Tauri 与 Python `cmd_rpc` 通信的心跳与超时降级机制，防止子进程异常挂起。
- **状态**：已解决（blocking_send 优化及多账号稳定通信）

### 任务 3：JIT 到期动态自感知刷新与满血复活桌面通知
- **目标**：实现账号重置时刻 (+10s 缓冲) 自动精准唤醒同步，刷新完成后注销旧定时器并顺延周期轮询；支持满血复活原生系统通知，全参数独立设置可控。
- **状态**：已完成
- **检查清单**：
  - [x] 更新 `docs/architecture.md` 混合调度模型与生命周期
  - [x] 更新 `docs/work/history.md` 决策记录与避坑细节
  - [x] 扩展 `gui/src/types.ts` 配置定义 (`dynamicResetEnabled`, `notifyOnQuotaRestored`)
  - [x] 优化 `gui/src/utils/settings.ts` 读写与默认值
  - [x] 在 `gui/src/App.tsx` 实现 JIT 定时器动态扫描、挂载、全量旧定时器清除及状态跃迁通知比对
  - [x] 在【设置与关于】扩展对应的 UI 开关与交互说明
  - [x] 构建与测试验证 (前端 build + Tauri release)

### 任务 4：定时预热流水线与单账号多闹钟调度引擎 (Warmup Scheduler)
- **目标**：实现单账号独立多闹钟错峰预热（间隔 $\ge 5$ 小时防重叠拦截）、全局可配默认问候模型（默认为 `gpt-5.6-luna`，支持标签动态增删）、SQLite WAL 配置与闹钟表持久化、以及无痕子进程调用 `codex exec --ephemeral`。
- **状态**：已完成
- **检查清单**：
  - [x] 更新 `docs/PROJECT.md` 规格规范
  - [x] 更新 `docs/architecture.md` Schema 与 RPC 协议
  - [x] 制定 `implementation_plan.md` 实施方案
  - [x] 在 `codexq.py` 中初始化 `app_settings` 与 `account_alarms` 数据表
  - [x] 在 `codexq.py` 中实现 `warmup_account_async`（带活跃窗口检测与超时保护）
  - [x] 在 `codexq.py` 中增加 CLI 子命令 `warmup` 与 `alarm`
  - [x] 在 `codexq.py` RPC 服务中暴露设置读写、闹钟管理与立即预热接口
  - [x] 在 `gui/src-tauri` 中暴露对应的 Tauri Commands
  - [x] 在 `gui/src` 中新增侧边栏「定时预热」模块与标签动态编辑组件
  - [x] 自动化测试套件扩充与端到端构建验证 (23 项单元测试全通过，TypeScript 编译零错误)
  - [x] 原生 Release 安装包与独立二进制打包完成 (`pnpm run tauri build` 生成 NSIS 安装包、MSI 安装包与二进制 `app.exe`，已附带内置资源打包)
  - [x] UI 视觉精简与术语对齐（「定时预热」更名为「定时触发」，精简冗余说明，重构账号切换栏与闹钟排期布局，触发设置卡片统一归拢至「设置与关于」）

### 任务 5：配置存储从 SQLite 解耦迁移至纯文本 `config.json` 与分级超时保障
- **目标**：将应用设置从 SQLite 黑盒表剥离为透明、手写友好、原子写入的 `~/.codexq/config.json`；同时重构 Rust RPC 客户端超时机制为分级专用超时（测试触发 90s 超时保障 LLM 对话）。
- **状态**：已完成
- **检查清单**：
  - [x] 在 `codexq.py` 中移除 `app_settings` 表并实现旧配置平滑迁移
  - [x] 建立 `~/.codexq/config.json` 纯文本规范与原子替换写入（`_save_config_atomic`）
  - [x] 兼容 dot-notation 键与分层字典双向读写
  - [x] Rust `PyRpcClient` 实现方法分级超时体系（`trigger_warmup` 专属 90s，`refresh` 45s）
  - [x] 手动「测试触发」去除活跃窗口拦截（强制 `force: true`）
  - [x] 补充 `test_config_json_file_persistence_and_human_edit` 单元测试，24 项测试全通过
  - [x] 打包并验证最新 Release 二进制与安装包

### 任务 6：全栈中英双语国际化支持 (i18n Architecture & Localization)
- **目标**：实现以英文为基线与安全兜底 (`en-US`)，配合独立简体中文语言包 (`zh-CN.json`) 的三层多语言国际化体系；支持系统语言自动感知与界面即时热切换；Tauri 托盘与 Python 引擎协同适配。
- **状态**：已完成
- **检查清单**：
  - [x] 更新 `docs/architecture.md` 第 7 节详细规范与分层设计
  - [x] 制定 `implementation_plan.md` 实施方案并获批
  - [x] 安装 `i18next` 与 `react-i18next` 并初始化 `src/i18n/` 架构与系统语言侦测
  - [x] 提取前端 UI（账号管理、定时触发、回收站、设置、所有弹窗、Toast）硬编码文本至模块化 `zh-CN.json` 和 `en-US.json`
  - [x] 在【设置与关于】扩展语言切换控件（跟随系统 / 简体中文 / English），联动更新 `~/.codexq/config.json`
  - [x] Rust 端实现 `set_locale` 命令与系统托盘菜单动态热重绘 (`tray.set_menu`)
  - [x] Python 端增加 `general.locale` 配置支持，零外部第三方依赖，保持标准英文/机器码基线
  - [x] 单元测试套件全部通过 (24/24)，前端生产编译 (`tsc -b && vite build`) 零错误零警告，双语切换热加载验证完成

### 任务 7：「仅一次」排期闹钟与生命周期自闭机制 (One-time Alarm & Auto-disable)
- **目标**：响应单次/临时触发场景，增加「仅一次」(`once`) 排期模式，并完善后台轮询调度协程与触发后自动关停机制。
- **状态**：已完成
- **检查清单**：
  - [x] 更新 `docs/architecture.md` 架构与生命周期定义
  - [x] 扩展 `zh-CN.json` 与 `en-US.json` 双语词条（优化为“触发时间”与“重复周期”，新增“仅一次”）
  - [x] 优化 `SchedulerView.tsx` 表单选择项与列表醒目琥珀色徽章
  - [x] 在 `CodexQ` 类中实现 `check_and_fire_alarms` 方法与 75s 防重保护
  - [x] 在 `cmd_rpc` 中挂载后台 `alarm_scheduler` 常驻协程（每 15s 轮询）
  - [x] 实现 `once` 模式触发后 `enabled = 0` 自动关停与状态落盘
  - [x] 补充单元测试 `test_once_alarm_creation_and_auto_disable`，25 项测试全部通过
### 任务 8：修复 RPC 测试触发未定义变量报错 (Fix `trigger_warmup` missing `prompt` in RPC)
- **目标**：修复在定时触发页面点击「测试触发」时因 Python RPC 服务端缺少 `prompt = params.get("prompt")` 变量解析导致的 `NameError: name 'prompt' is not defined` 异常。
- **状态**：已完成
- **检查清单**：
  - [x] 在 `codexq.py` 的 `cmd_rpc` 中对 `trigger_warmup` 请求正确提取并清洗 `prompt` 与 `model` 参数
  - [x] 补充 `tests/test_rpc.py` 中的 `test_rpc_trigger_warmup_protocol` 契约测试，断言参数处理无 NameError
  - [x] 单元测试套件全部通过 (26/26)，前端构建无任何报错


### 任务 9：全面纯 Tauri 化与工程目录扁平化重构 (Pure Tauri Migration & Directory Elevation)
- **目标**：将工程目录由 `gui/` 提升至工程根目录，桌面端全面重构为纯 Tauri (Rust Core) 架构，废除 stdio JSON-RPC 跨进程守护与 Python 运行时依赖，实现零环境门槛的绿色单二进制分发。
- **状态**：已完成
- **检查清单**：
  - [x] 将 `gui/` 目录提升至工程根目录（`src/`, `src-tauri/`, `package.json`, `vite.config.ts` 等），删除原 `gui/` 文件夹
  - [x] 在 `src-tauri/src/core/` 中纯 Rust 实现 SQLite WAL 存储、JWT 解析、Profile 隔离沙箱、原子切号防丢
  - [x] 在 `src-tauri/src/core/probe.rs` 中纯 Rust 异步实现 `codex app-server` 探测与握手
  - [x] 在 `src-tauri/src/core/warmup.rs` 与 `scheduler.rs` 中实现 5 小时错峰闹钟与后台轮询定时任务
  - [x] 彻底删除 `rpc.rs` 与 Python 守护进程，Tauri 命令直接调用 Rust Core
  - [x] 清理 `tauri.conf.json` 中的 Python 资源打包配置
  - [x] 补充 Rust 原生单元测试并通过全部测试 (5/5)，前端 `pnpm run build` 与 `cargo check --release` 全通过

### 任务 10：项目文档系统全面校准与工程同步 (Documentation Synchronization & Alignment)
- **目标**：全面刷新根目录 `README.md` 与 `docs/` 规范文档，使其准确反映当前最新的双轨分发架构（纯 Rust 核心 Tauri 2 绿色桌面端 + 纯标库单文件 Python CLI/SDK 伴侣），补齐定时错峰闹钟、JIT 到期通知、全栈 i18n、透明 `config.json` 等全部新特性，并彻底移除 `python codexq.py gui` 冗余入口实现完全正交解耦。
- **状态**：已完成
- **检查清单**：
  - [x] 重写 `README.md`，对齐双轨定位、全量新功能特性清单、完整目录树与全量 CLI 命令
  - [x] 原位校准 `docs/PROJECT.md` 规格说明与非目标界定
  - [x] 原位修正 `docs/architecture.md` 中残留路径与调度器说明
  - [x] 彻底删除 `codexq.py` 中的 `cmd_gui` 冗余子命令与解析器，桌面端与 Python 伴侣完全解耦
  - [x] 完成机械验证（Rust 单元测试、前端类型检查与生产构建、Python 单元测试套件全部绿灯）

### 任务 11：OAuth Token 自动续期与双引擎备份同步对齐 (Token Auto-Renewal & Dual-Engine Alignment)
- **目标**：解决用户反馈的频繁 401 过期问题，实现 OpenAI OAuth 2.0 自动续期协议 (RTR)；打通历史备份智能吸纳 (`~/.codex/backups/`)，确保已有账号能无损接收最新凭证；实现 Rust Core 与 Python 伴侣的双端一致性对齐。
- **状态**：已完成
- **检查清单**：
  - [x] 分析根因：`auto_sync_backups` 遗漏已有账号，只读探测接口不触发官方 Auth0 续期
  - [x] Rust Core 引入纯 Rust TLS `reqwest`，实现 `refresh_oauth_token_for_profile` 与 `is_access_token_expired`
  - [x] 修复 Rust Core `auto_sync_backups` 阻断逻辑，基于时间序与防降级安全更新已有账号并重置 `credential_status`
  - [x] 在额度探测 (`refresh_one`)、预热 (`trigger_warmup`) 与切号 (`switch_account`) 流程中挂载主动预测续期与 401 响应式自愈
  - [x] Python 伴侣端以严格纯标准库（`urllib.request` + `asyncio.to_thread`）对齐实现 OAuth 续期与防降级更新
  - [x] 补充两端自动化测试套件：Rust 9 项测试通过，Python 29 项测试通过，前端构建零错误
  - [x] 同步更新 `architecture.md`、`PROJECT.md` 与 `current.md` 架构文档

### 任务 12：CodexQ v1.0.0 正式版发布就绪与工程闭环 (v1.0.0 Release Readiness)
- **目标**：完成 CodexQ v1.0.0 首个正式发行版发布所需的所有关键工程闭环，涵盖账号便捷引入工作流、环境诊断中心 (Doctor)、品牌 Logo 升级、布局滚动卡顿性能优化、开源合规与跨平台自动化发布 CI/CD。
- **状态**：已完成
- **检查清单**：
  - [x] **布局滚动卡顿彻底根治**：锁定 `html, body, #root` 根容器高度与 `overflow: hidden`，解除外层泄露滚动；侧边栏独立锁定，移除顶部导航栏高开销 `backdrop-blur-md`，全量恢复 60fps 丝滑滚屏。
  - [x] **账号快速引入 (Onboarding)**：桌面端实现「添加账号」弹窗（内置终端登录引导 `codex login` 与免插件内存直读 `auth.json` 凭据导入）；Python 伴侣端提供 `codexq import <path>` 统一 CLI 命令与 30 项测试覆盖。
  - [x] **环境诊断中心 (CodexQ Doctor)**：Rust Core 深度集成 `doctor` 模块，从 CLI 探测、沙箱配置模式、SQLite WAL 存储到 OpenAI Auth 连通性四维全面体检，设置页提供一键体检与刷新能力。
  - [x] **品牌标识与视觉定制**：定制设计专属 SVG Favicon/Logo（Q 型额度环规 + 闪电穿透），全端视觉统一。
  - [x] **开源规范与版本统一**：根目录新增 MIT 开源 `LICENSE`；全量配置 (`package.json`, `Cargo.toml`, `tauri.conf.json`, `codexq.py`) 统一同步版本号至 `1.0.0`。
  - [x] **自动化发布工作流**：配置 `.github/workflows/release.yml`，打通 Windows (`.exe` / `.msi`)、macOS (`.dmg`)、Linux (`.AppImage` / `.deb`) 跨平台多架构全自动编译与 GitHub Release 产物发布。

### 任务 13：彻底消除 Windows 终端控制台黑窗口闪烁 (Silent Execution & Process Sandboxing)
- **目标**：解决在切换侧边栏标签或运行预热/体检探测时 Windows 弹出的 transient CMD 黑色控制台闪烁问题。
- **状态**：已完成
- **检查清单**：
  - [x] **Doctor 模块静默化**：`doctor.rs` 引入 `std::os::windows::process::CommandExt`，强制附加 `creation_flags(0x08000000)` (`CREATE_NO_WINDOW`)，重定向 `stdin(Stdio::null())`，并优先直接调用官方可执行文件，彻底杜绝 `cmd.exe /c` 弹窗。
  - [x] **Codex 路径直连解析**：优化 `find_codex_bin()` 在 Windows 下自动解析 `AppData/Local/Programs/OpenAI/Codex/bin/codex.exe` 与 WinGet 绝对路径，规避 shell 中转。
  - [x] **Warmup 预热执行隔离**：`warmup.rs` 为 `codex exec` 明确绑定 `stdin(Stdio::null())` 与管道重定向，阻断进程尝试依附控制台或等待输入。
  - [x] **后台进程与终端登录保护**：`process.rs` 与 `commands.rs` 全面加固 `CREATE_NO_WINDOW` 与空输入流。
  - [x] **构建并重新打包**：完成 Release 单二进制与安装包重新打包（零弹窗、零警告）。

### 任务 14：全栈 Linux 系统兼容性深度适配与构建加固 (Linux Compatibility & Hardening)
- **目标**：实现 CodexQ 在 Linux 系统（Ubuntu、Debian、Arch Linux、Fedora）上的完全原生兼容，包括 POSIX 权限沙箱、精准进程控制、多终端模拟器适配、路径探测与 CI/CD 跨平台打包。
- **状态**：已完成
- **检查清单**：
  - [x] **POSIX 权限隔离 (0700/0600)**：在 `auth.rs` 中为沙箱目录与凭据文件注入 `#[cfg(unix)]` 权限控制（目录 `0o700`，凭据文件 `0o600`，常规配置 `0o644`）。
  - [x] **进程查杀安全防自杀**：重构 `process.rs` 中的 `restart_codex_unix`，废弃粗暴的 `pkill -f codex`，精准过滤 `codex.*app-server`、`codex-code-mode-host` 与官方桌面进程，彻底避免误杀 CodexQ 自身。
  - [x] **多终端模拟器智能拉起**：在 `commands.rs` 中为 Linux 端实现终端命令分发矩阵，兼容 GNOME Terminal (`--`)、Konsole (`-e`)、XFCE (`-x`)、Alacritty、Kitty、Foot、Tilix、Terminator 及 shell 降级。
  - [x] **Linux CLI 路径自动探测**：在 `probe.rs` 中扩展 `find_codex_bin()` 自动检测 `~/.local/bin/codex`、`/usr/local/bin/codex`、`~/.cargo/bin/codex` 等常见位置，规避 GUI 桌面会话 PATH 缺失。
  - [x] **Linux 打包配置与 CI/CD**：在 `tauri.conf.json` 中配置 Linux 桌面类别与描述元数据；在 `.github/workflows/release.yml` 中添加 `libayatana-appindicator3-dev` 保证 Linux 托盘图标顺利编译生成 `.AppImage` 与 `.deb` 包。
  - [x] **全套测试与文档更新**：通过 Rust 单元测试、Python 单元测试与前端生产构建，同步更新中英双语文档。

### 任务 15：界面语言切换通知状态同步与响应式国际化提示 (Reactive Toast Localization)
- **目标**：解决切换语言时，提示横幅展示旧语言（如从中文切到英文仍显示“界面语言切换成功”）的闭包时序问题。
- **状态**：已完成
- **检查清单**：
  - [x] **根因分析**：组件闭包内 `t` 函数在事件触发时刻仍绑定上一渲染周期的语言上下文；且 `showToast` 过去仅接收静态字符串写入状态，无法在 i18n 变更后动态重新求值。
  - [x] **类型扩展 (`ToastPayload`)**：在 `types.ts` 中定义 `ToastPayload = string | { key: string; params?: Record<string, any> }`。
  - [x] **响应式渲染**：`toastMessage` 状态升级为支持结构化键值存储，在 JSX 渲染期通过 `toastMessage.key ? t(toastMessage.key, toastMessage.params) : toastMessage.text` 进行动态惰性求值。
  - [x] **调用链统一**：`handleLanguageChange` 改为派发 `{ key: 'toasts.languageChanged' }`；同时梳理更新所有账号、排期与设置开关的 Toast 为响应式 key。
  - [x] **全量验证**：前端 TypeScript 严格检查与 Vite 生产构建通过，Release 二进制打包验证无误。

### 任务 16：5小时配额恢复自动续窗与周配额安全拦截 (Auto-Rollover on 5h Quota Restore)
- **目标**：实现 5 小时配额恢复可用且 1 周内尚有配额时的自动续窗（自动垫刀）功能，解决空闲期窗口中断错位问题，同时严密拦截周限额耗尽场景。
- **状态**：已完成
- **检查清单**：
  - [x] **架构与配置设计**：在 `~/.codexq/config.json` 的 `trigger` 节点扩展 `account_rollovers: HashMap<String, AccountRolloverConfig>` 配置定义，支持按账号精细化独立配置。
  - [x] **Rust Core 核心引擎**：在 `scheduler.rs` 中实现 `check_and_fire_auto_rollover`，每 30 秒轮询检查已恢复账号；对齐 5 小时恢复状态与周剩余额度保底门限（`min_weekly_remaining`，默认为 0.0%）；注入 15 分钟防死循环冷却与成功后即时触发 `refresh_one` 刷新新窗口。
  - [x] **Python Companion 伴侣端**：在 `codexq.py` 中同步支持 `get_account_rollover` 与 `set_account_rollover`，保持纯标准库零外部依赖与向前兼容性。
  - [x] **UI 与双语国际化**：将自动续窗配置收敛至【定时触发】(`SchedulerView.tsx`) 页面中，随当前选中的账号即时联动，支持“启用自动续窗”与“周最低剩余额度 ≥ [ 0 ] %”设定，并实时展示当前账号周剩余额度状态；移除全局设置中的粗粒度开关；补齐 `zh-CN.json` 与 `en-US.json` 全套双语翻译。
  - [x] **端到端测试与构建**：Rust 单元测试全部通过 (10/10)，Python 单元测试全部通过 (32/32)，前端生产构建 (`tsc -b && vite build`) 零错误。

### 任务 17：Tab 切页内存保活与绿色免安装便携版 (DOM Keep-Alive & Windows Portable EXE)
- **目标**：解决切换标签页导致「定时触发」选中账号被重置回当前活跃账号的体验问题；遵循零 LocalStorage 内存保活要求；并在 Windows 平台提供免安装绿色单二进制可执行文件 (`CodexQ-v1.0.0-portable.exe` 与便携压缩包)。
- **状态**：已完成
- **检查清单**：
  - [x] **Tab 内存保活 (CSS Keep-Alive)**：将 `App.tsx` 中的条件挂载渲染 (`currentTab === ... ? ... : null`) 改造为桌面端标准的 CSS `hidden` 常驻显隐，使得应用运行期间 Tab 切换 0ms 瞬间显示且选中状态、输入框等内存状态全程保持；应用完全退出冷启动时，组件首次挂载自然重置回当前系统活跃账号，避免 LocalStorage 跨会话脏状态。
  - [x] **Windows 免安装便携版 (Portable EXE)**：
    - 在 `src-tauri/src/core/paths.rs` 引入便携模式支持：检测到当前可执行文件同级目录存在 `portable` 标记文件或 `data/` 目录时，自动将数据与配置沙箱重定向至 `./data`，实现真正的“U盘随身携带、不留宿主痕迹”。
    - 编写 `scripts/package-release.mjs` 与 `scripts/build-portable.mjs`，在 `release/` 输出 `CodexQ-v1.0.0-portable.exe`（单文件免安装）与 `CodexQ-v1.0.0-windows-x64-portable.zip`（便携包，压缩后仅 5.2MB）。
    - 在 `package.json` 中配置便捷命令：`pnpm run build:portable` 与 `pnpm run package`。
  - [x] **全套测试与验证**：Rust 单元测试全部通过 (11/11)，Python 单元测试全部通过 (32/32)，前端生产构建通过。

### 任务 18：桌面端全局单实例运行与重复唤醒 (Single-Instance Application Lifecycle & Window Wakeup)
- **目标**：解决重复打开 CodexQ 导致多进程并发启动、托盘图标重复与 SQLite 锁竞争的问题；实现类似官方 Codex 的全局单实例运行，重复运行可执行文件时自动唤醒、取消最小化并置顶聚焦已有的 CodexQ 窗口。
- **状态**：已完成
- **检查清单**：
  - [x] 在 `src-tauri/Cargo.toml` 中引入 `tauri-plugin-single-instance = "2"`
  - [x] 在 `src-tauri/src/lib.rs` 的 Builder 链首部注册 single-instance 插件，监听二次启动事件
  - [x] 触发时调用 `window.show()`、`window.unminimize()` 与 `window.set_focus()` 唤醒主窗口
  - [x] 机械验证：`cargo check`、`cargo test` 与 `pnpm run build` 全部通过

## 3. 已知技术债与待优化项 (Tech Debt)

1. **单文件维护性**：`codexq.py` 目前约 3500 行代码，保持单文件标准库免安装即用的同时，通过详尽的 29+ 项单元测试套件保证稳定性。
2. **测试覆盖率**：持续增强核心业务在跨平台（Windows / Linux / macOS）沙箱中的自动化断言。
