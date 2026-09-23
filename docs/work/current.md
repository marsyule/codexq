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
  - [x] 在 `src-tauri/src/lib.rs` 的 Builder 链首部注册 single-instance 插件（显式限定 `#[cfg(desktop)]`）
  - [x] 按运行环境精细化作用域（Dev 追加 `.dev`、Portable 追加 `.portable`、正式安装版全局互斥），杜绝开发调试与正式运行相互误杀
  - [x] 触发时调用 `window.show()`、`window.unminimize()` 与 `window.set_focus()` 唤醒主窗口并完善 `log::warn!` 可观测性
  - [x] 修复 Portable 首次冷启动识别：增强 `is_portable()` 检测可执行文件名包含 `portable`（不区分大小写），并在便携 ZIP 归档中内置打包 `portable` 标记文件，确保首次运行即可精准定位 `./data` 与 `.portable` 单实例作用域
  - [x] 机械验证：`cargo check`、`cargo test` (12/12) 与 `pnpm run build` 全部通过

### 任务 19：第三方 AI 服务商接入与统一 Runtime Slot 架构 (Third-Party Providers & Atomic Slot)
- **目标**：破除单一官方账号绑定，支持接入任意兼容 OpenAI Responses 规范的第三方端点（如 DeepSeek、StepFun、SiliconFlow、OpenRouter 等）；实现多模型池一键导入与切换；以 AST 级无损方式协同管理 `auth.json` 与 `config.toml`，100% 保持用户现有配置、注释与 MCP 服务；同时保留 CodexQ 独家官方多账号 5H/周度额度高频探针与防损切号能力。
- **状态**：已完成
- **检查清单**：
  - [x] **Rust 核心引擎**：引入 `toml_edit` 库，在 `src-tauri/src/core/provider.rs` 实现 `Provider` 模型、`test_provider_connectivity`（带通用 `/models` 拉取）、`generate_model_catalog`（生成 `models.json` 挂载目录）与 `0600` 权限独立密钥存储（`~/.codexq/providers/<id>/key`）。
  - [x] **SQLite 数据库层**：创建 `providers` 元数据表与完整 CRUD 方法（密钥绝不入库，仅持久化掩码与配置参数）。
  - [x] **原子运行时槽位切换 (`switch.rs`)**：
    - `switch_to_provider`：保存快照、备份当前官方 Token、生成 dynamic `models.json`、无损注入 `config.toml`（`model`、`model_provider`、`[model_providers.<id>]`）与 `auth.json`。
    - `switch_account`：升级为自动清理 `config.toml` 中的 `model_provider` 与 `model`，完美恢复官方 Codex 纯血路由。
    - `get_active_runtime_mode`：读取 `config.toml` 返回当前是官方账号模式还是第三方服务商模式。
  - [x] **前端现代化交互 (`src/components/`)**：
    - 新增 `ProvidersView` 主选项卡视图，支持模糊检索、模型总览与卡片列表。
    - 新建 `ProviderCard` 卡片：支持多模型下拉切换即时激活、连通性延时检测（Ping）、编辑与安全删除。
    - 新建 `AddProviderModal` 通用弹窗：通用 OpenAI Responses 表单，支持一键 `GET /models` 智能解析汇入与手动标签增删，支持“保存”与“保存并立即切换”。
    - 升级 `ActiveHeroCard`：支持第三方服务商模式运行状态展示、生效模型显示与“切回官方账号”快捷按键。
    - 升级侧边栏导航：无缝嵌入「模型服务商」Pill 导航。
  - [x] **全栈国际化 (i18n)**：补齐 `src/i18n/locales/zh-CN.json` 与 `en-US.json` 所有服务商相关翻译。
  - [x] **Python CLI / SDK 伴侣端 (`codexq.py`)**：
    - 严格遵循纯标准库 0 外部依赖约束，实现 `codexq provider list/add/use/test/remove` 子命令及 JSON-RPC 接口。
    - 实现标准库级无损 `config.toml` 与 `models.json` 生成器。
    - 新增 `tests/test_provider.py`，全套 5 个自动化测试全部通过。
  - [x] **全量机械校验**：Rust 单元测试全部通过 (14/14)，Python 单元测试全部通过 (38/38)，前端生产构建 (`pnpm run build`) 零错误。

### 任务 20：服务商模式缺陷修复、浅色视觉统合与高级配置支持 (Provider Fixes & Advanced Configuration)
- **目标**：解决用户在实际体验第三方服务商时遇到的启动崩溃与体验割裂问题：
  1. 修复切换到第三方服务商后 Codex Desktop / CLI 启动报错“无法加载登录要求”的致命异常；
  2. 重构模型服务商模块所有组件为统一的 Clash Verge 浅色设计系统（淡灰蓝背景、精细 slate 边框、纯白卡片、科技蓝强调色）；
  3. 解耦「测试连通性」与「从上游获取模型」，新增上游候选模型池支持关键词检索与精细选择导入，避免粗暴全量覆盖；
  4. 完整实现「高级设置」折叠面板，支持自定义 `wire_api`、备注说明，并提供直接可编辑的 `config.toml` 与 `auth.json` 专属高级文本框与实时标准模板一键还原能力。
- **状态**：已完成
- **检查清单**：
  - [x] **根因定位与 Codex 启动修复**：经 `codex -c ... doctor` 严密测试，官方 Codex CLI v0.149.1+ 遇到根级未声明的 `model_catalog_json` 配置项时会直接触发致命解析错误（`failed to load Codex config`），进而导致桌面端启动时提示“无法加载登录要求”。彻底移除 Rust 与 Python 端写入 `model_catalog_json` 的逻辑，并在切换时主动清理历史可能残留的废弃键；经 `codex --strict-config doctor` 检验，19 项核心检查全部通过（`0 fail degraded`）。
  - [x] **视觉风格统合 (Clash Verge Light Theme)**：
    - 全面替换 `AddProviderModal.tsx`、`ProviderCard.tsx`、`ProvidersView.tsx` 以及 `ActiveHeroCard.tsx` 中的深色调（zinc-900 / dark 系），统一迁移至与 `AccountCard` 一致的现代浅色设计系统。
    - 统一使用 `border-slate-200`、`bg-slate-50`、科技蓝 `blue-600`、翡翠绿 `emerald-500` 与精巧徽章规范。
  - [x] **连通性测试与上游模型获取解耦**：
    - 将原有合一体检拆分为「测试连接」（专属 Ping 延时测试）与「从上游获取模型」（专用 `GET /models` 接口拉取）两个独立按键。
    - 新增「上游可用模型」候选展示区，支持模型模糊搜索与过滤；单条模型带有 `+` 添加按钮，点击即可单独汇入下方「激活模型池」，同时保留「一键添加全部」快捷操作，杜绝无序全量导入污染模型池。
    - 模型池支持设为默认模型、删除以及手动输入任意非上游私有模型 ID。
  - [x] **「高级设置」折叠面板与双文本框持久化**：
    - 展开面板包含 `wire_api` 协议选择器（默认 `chat`，支持 `completions`）、服务商备注信息。
    - 新增 `config.toml` 预览与编辑框：根据当前配置动态生成即将注入 `~/.codex/config.toml` 的 TOML 代码段，允许高级用户自定义覆写；提供「恢复标准模板」一键还原按钮。
    - 新增 `auth.json` 预览与编辑框：针对非官方账号模式可定制写入凭证文件结构，提供一键还原按钮。
  - [x] **底层持久化与跨端对齐**：
    - SQLite `providers` 表平滑迁移增加 `custom_config_toml TEXT` 与 `custom_auth_json TEXT` 字段。
    - Rust Core `Provider` 模型、Tauri Commands `save_provider` 与 Python Companion `codexq.py` 完全对齐新增字段与切换写入逻辑。
  - [x] **全量机械检验**：Rust 单元测试全部通过 (14/14)，Python 单元测试全部通过 (38/38)，前端生产构建零错误零警告。

### 任务 21：移除黑盒推断并支持自由配置模型上下文长度（最低保底 256K） (Configurable Context Window & Min 256K Clamp)
- **目标**：解决第三方模型（如具有 1M 上下文的 DeepSeek-flash 等）在 Codex Desktop 侧被误识别为 128k 导致长上下文受限的问题：
  1. 彻底移除 `infer_model_context_window` 字符串黑盒推断逻辑；
  2. 建立硬性保底：默认最低上下文长度为 256k（256,000 tokens），任何低于 256,000 的数值自动 clamp 保底为 256,000；自动压缩阈值 `auto_compact_token_limit` 严格对齐 85%（`context_window * 0.85`）；
  3. 提供全套可视化上下文配置界面：在 `AddProviderModal` 中支持预设药丸按钮（`256K (默认最低)`、`512K`、`1M`、`2M`）与自定义输入框（支持输入 `1m`、`512k` 等单位并实时计算压缩阈值）；
  4. 支持模型池单模型独立上下文覆盖（在模型 chip 上可直接点击切换预设，双击自定义输入，覆写项展示为翡翠绿星标徽章）；
  5. 在 `ProviderCard` 卡片主模型旁与下拉选单中直观呈现当前模型的上下文长度徽章；
  6. Rust Core、SQLite、Python CLI/SDK、动态 `model-catalogs` 生成器全面支持并完成端到端自动化测试验证。
- **状态**：已完成
- **检查清单**：
  - [x] **Rust Core 核心引擎与存储**：
    - 在 `provider.rs` 中定义 `DEFAULT_MIN_CONTEXT_WINDOW = 256_000`，彻底移除 `infer_model_context_window`；
    - `Provider` 结构体新增 `context_window: Option<u64>` 与 `model_context_windows: Option<HashMap<String, u64>>`；
    - `generate_model_catalog` 强制执行 `.max(256_000)` 保底，并计算 85% 压缩阈值；
    - `db.rs` 增补 `context_window` 与 `model_context_windows` 字段迁移与 CRUD；
    - `commands.rs` 增补 `SaveProviderPayload` 字段。
  - [x] **Python CLI / SDK 伴侣端**：
    - 同步移除 `infer_model_context_window`，落地 `DEFAULT_MIN_CONTEXT_WINDOW = 256_000`；
    - `Store.upsert_provider`、`generate_model_catalog` 及 CLI `codexq provider add --context-window` 均支持上下文长度配置（CLI 参数的实际接线于任务 23 补齐）；
    - 编写 `tests/test_provider.py` 自动化测试，验证保底与单模型覆盖。
  - [x] **前端交互与现代化设计**：
    - 在 `types.ts` 中导出 `formatContextWindow` 格式化工具函数；
    - 在 `AddProviderModal.tsx` 中新增上下文长度预设药丸卡片与单位解析自定义输入框；
    - 在模型池芯片中提供即时循环点击与双击定制上下文的专属交互；
    - 在 `ProviderCard.tsx` 中展示模型上下文微徽章并标注下拉菜单；
    - 完善 `zh-CN.json` 与 `en-US.json` 全套国际化词条。
  - [x] **全量机械校验**：
    - Rust 单元测试通过 (15/15)；
    - Python 单元测试通过 (39/39)；
    - 前端生产构建 (`pnpm run build`) 零错误；
    - 本地 `~/.codex/model-catalogs/codexq-newapi.json` 作为 catalog 产物生成；**注意**：根级 `model_catalog_json` 已被 Codex CLI 判定为非法键并移除（详见任务 22），该产物当前仅作为可查阅工件，不参与运行时加载。

### 任务 22：服务商模式代码评审修复与跨端一致性加固 (Provider Review Fixes)
- **目标**：修复针对任务 19–21 的全量代码评审发现的阻断级问题：测试污染真实 `~/.codex`、Rust/Python 共用 SQLite `providers` 表结构分叉、以及根级 `model_catalog_json` 回归（经本机 `codex doctor` 实测确认会导致 config 加载失败）。
- **状态**：已完成
- **检查清单**：
  - [x] **P0-1 Python catalog 路径隔离**：`generate_model_catalog` 移除硬编码 `Path.home()/".codex"`，新增 `codex_home` 参数（默认 `~/.codex`）；`switch_to_provider` 传入 `self.auth_path.parent`；`remove_account` 的备份扫描目录同步改为随 `auth_path` 解析。修复了测试写入真实主机目录（违反 AGENTS.md §4）与 portable/自定义路径下 catalog 落错目录的问题；并清理了真实 `~/.codex/model-catalogs` 中的测试残留文件。
  - [x] **P0-2 SQLite schema 跨端统一**：Python 补齐 `key_masked`/`key_sha256` 两列与 ALTER 迁移，并在 `upsert_provider` 中写入；Rust 两列补 `DEFAULT ''` 并增加 ALTER 兜底；`upsert_provider` 在 `key_sha256` 为空时保留原值。机械验证：双向插入/查询均通过，`codexq provider add` 不再触发 NOT NULL 约束错误。
  - [x] **P0-3 根级 `model_catalog_json` 回归修复**：移除 `switch.rs`、`commands.rs` 与前端 `AddProviderModal` 默认模板中的写入，并在切换两个方向时主动清理历史残留键。以隔离 `CODEX_HOME` + `codex-cli 0.149.1` 的 `codex doctor` 实测复现：含该键 → `config could not be loaded`；移除后 → 配置正常加载。
  - [x] **P1-4 保留 Key 哈希**：新增 `db::get_provider_key_hash`，编辑 provider 未重填 Key 时不再清空 `key_sha256`。
  - [x] **P1-5 切换官方时不再误删用户配置**：仅当 `model_provider` 指向第三方 provider 时才移除 `model_provider`/`model`，并清理对应 `[model_providers.<id>]` 表（含明文 bearer token）；`model_provider = "openai"` 与用户自定义 `model` 被保留。
  - [x] **P1-6 快照保留上限**：新增 `MAX_RUNTIME_SNAPSHOTS = 20`，每次切换后裁剪最旧的 `~/.codexq/backups/<ts>_<reason>` 目录，避免明文凭据备份无限增长。
  - [x] **P1-7 官方 provider id 识别**：Rust/Python 的 `get_active_runtime_mode` 均将 `openai` 视为官方路由，不再误判为第三方模式。
  - [x] **P1-8 provider slug 去重**：新建 provider 的 slug 冲突时自动追加 `-2/-3` 后缀，杜绝静默覆盖已有 provider。
  - [x] **P1-9 跨端行为对齐**：Python `mask_api_key` 掩码格式与 Rust 统一；Python 切换到 provider 时不再覆写 `auth.json`（仅当配置了 `custom_auth_json` 时写入），与 Rust 保持官方便凭据无损一致。
  - [x] **自动压缩阈值对齐 85%**：`auto_compact_token_limit` 由 `null` 改为按工作窗口计算 `context_window × 0.85`（256K → 217,600；512K → 435,200；1M → 850,000）；`effective_context_window_percent` 保持 95% 作为 Codex 侧 headroom。依据：Codex 自身在物理窗口前留有 headroom，将压缩点提前到工作窗口的 85% 可避免 stale context 累积、比官方默认的 90% 更干净。Rust/Python 两端与文档取值已统一（原文档声称 85% 但代码为 `null`，本次落地）。
  - [x] **P2 文档卫生与死代码清理**：清理 `provider.rs` 中 `infer_model_context_window` 时代残留的重复 Rustdoc；抽取纯函数 `build_model_catalog`（无 I/O）供单元测试直接断言，并移除已无引用的 `ModelCatalog`/`ModelCatalogEntry`。Rust catalog 测试不再写入真实 `~/.codex`，与 Python 端测试隔离策略对齐。
  - [x] **全量机械校验**：Rust 单元测试 15/15、`cargo check` 零警告、Python 单元测试 39/39（且不再触碰真实 `~/.codex`）、`pnpm run build` 零错误。

### 任务 23：Current Truth 文档与代码同步核查 (Docs/Code Sync Audit)
- **目标**：核查任务 19–22 落地后，`docs/architecture.md` 与 `docs/PROJECT.md` 这两份 Current Truth 文档是否与代码同步；修复核查中发现的文档缺失与代码/文档契约落差。
- **状态**：已完成
- **检查清单**：
  - [x] **缺口定位**：`docs/work/current.md` 与 `docs/work/history.md` 已随任务 19–22 更新，但 `docs/architecture.md`（最后修改 09-16）与 `docs/PROJECT.md`（09-14）完全没有第三方服务商相关内容 —— 违反 AGENTS.md §5 与 `docs/index.md` §3「系统架构或业务逻辑变更时必须原位更新 Current Truth 文档」。
  - [x] **architecture.md 补齐**：
    - §1 架构图新增 `Provider Manager`、`providers/<id>/` 密钥沙箱、`backups/` 运行时快照、`~/.codex/config.toml` 路由与 `model-catalogs/` 工件节点及连线；
    - §2 目录结构补齐 `providers/<provider-id>/{key,models.json}` 与 `backups/<ts>_<reason>/`；
    - §2.1 数据表由「五张」更新为「六张」，新增 `providers` 表完整 DDL 与跨端 schema 契约说明；
    - 新增 §3.5「第三方服务商密钥沙箱」（0600 落盘、掩码规则、空密钥保留语义、删除即抹除）；
    - §4.1 补齐 6 个 provider Tauri Commands；
    - 新增 §8「第三方 AI 服务商接入与统一运行时槽位」（切换状态机、`config.toml` 无损注入与 `model_catalog_json` 非法键禁令、上下文窗口 256K 保底与 85% 压缩策略、交互入口）；
    - 修正 §7.2 语言包路径事实错误（`src/locales/` → `src/i18n/locales/`）。
  - [x] **PROJECT.md 补齐**：项目定位补入模型路由能力；核心功能规格新增「第三方模型服务商接入」条目；非目标「不做流量转发」条目补充澄清（CodexQ 仅写 `config.toml`，模型请求由 Codex CLI 直连服务商）。
  - [x] **用户手册补齐**：`README.md`（英文）与 `docs/README_zh.md`（中文）在核心特性、数据目录与 CLI 命令手册中补入第三方服务商条目（`provider add/list/use/test/remove` 示例），并同步 `docs/index.md` 的权威入口对照。
  - [x] **代码/文档契约落差修复**：文档曾声称 CLI 支持 `--context-window`，但 `build_parser` 中该参数从未注册、`cmd_provider` 也未把 `context_window` 传给 `upsert_provider`。本次补齐参数注册与接线，并在 `tests/test_provider.py` 新增 `test_cli_provider_add_supports_context_window` 覆盖「生效值 + 256K 保底 clamp」。
  - [x] **全量机械校验**：Python 单元测试 40/40 通过（原 39 + 新增 1）；真实 `~/.codex/model-catalogs` 与 `~/.codexq` 未被测试写入。

### 任务 24：Python 端 Provider → 官方切换残留修复 (Python Provider Lift Parity)
- **目标**：修复 `8af470c` 引入第三方 Provider 后，Python CLI（`python codexq.py switch <official-account>`）切回官方账号时遗留 `[model_providers.<id>]` 表与明文 `experimental_bearer_token`、且与 Rust/Tauri 行为不一致的问题。
- **状态**：已完成
- **检查清单**：
  - [x] **根因**：`lift_codex_config_provider()` 仅清理顶层 `model` / `model_provider` / `model_catalog_json`，未删除第三方 Provider 表，导致明文 API Key 残留。
  - [x] **语义对齐 Rust**：重写 `lift_codex_config_provider()`，读取顶层 `model_provider`；非官方（非 `openai` 且非空）时移除顶层 `model`/`model_provider`，并按表头精确匹配删除 `[model_providers.<id>]` 及其 `.<id>.` 子表；始终清除根级 `model_catalog_json`。
  - [x] **不误删用户配置**：`model_provider = "openai"` 或缺失时仅做 `model_catalog_json` 清理，保留用户官方 `model`；注释、MCP 表、`cli_auth_credentials_store` 与无关 Provider 表逐行透传不动。
  - [x] **辅助函数抽取**：新增 `_toml_table_header` / `_toml_top_level_key` / `_toml_top_level_string` 纯函数，`read_codex_config_active_provider()` 复用同一解析逻辑，消除顶层键匹配的松散行为。
  - [x] **回归测试**：`tests/test_provider.py` 新增 Case 1–4：第三方→官方清除表与密钥、保留其他 Provider、官方 `openai` 配置不误删、遗留 `model_catalog_json` 无论模式均清除。
  - [x] **全量机械校验**：Python 单元测试 44/44 通过；Rust 单元测试 15/15 通过（`cargo test`）。

## 3. 已知技术债与待优化项 (Tech Debt)

1. **单文件维护性**：`codexq.py` 保持单文件标准库免安装即用，通过详尽的 39 项单元测试套件保证稳定性。
2. **测试覆盖率**：持续增强核心业务在跨平台（Windows / Linux / macOS）沙箱中的自动化断言。
