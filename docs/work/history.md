# 历史决策与技术复盘 (history.md)

本文档记录 CodexQ 演进过程中的关键架构决策、权衡考量（Trade-offs）与踩坑教训（Historical Knowledge）。

---

## 1. 为什么 Python 核心坚持纯标准库、零外部依赖？

- **决策时间**：v0.1.0 设计初期
- **替代方案**：使用 `FastAPI` / `aiohttp` 做 API，使用 `SQLAlchemy` 做 ORM，使用 `pydantic` 做数据校验，使用 `requests` 做网络调用。
- **决策原因**：
  1. **无门槛随处可用**：Codex 用户环境各异，如果在 Python 端引入第三方依赖，用户在 CLI 运行时必须创建虚拟环境或执行 `pip install`，极易因网络、环境损坏或版本冲突导致失败。
  2. **冷启动与分发速度**：标准库无需安装直接执行，单文件拷贝即用，配合 Tauri 桌面分发时无需打包庞大的 Python virtualenv 依赖树。
- **代价与权衡**：
  - 需要手写轻量的数据校验、JSON 编解码与 SQLite 原生 SQL，但对于该规模工具，其代码负担远低于第三方包维护与依赖膨胀的代价。

---

## 2. 为什么 Rust GUI 与 Python 之间采用 stdio JSON-RPC 长连接？

- **决策时间**：v0.2.0 开发期
- **替代方案**：
  - 方案 A：每次 GUI 点击操作时，Rust 直接通过 `std::process::Command` 启动一次 `python codexq.py <subcommand>`。
  - 方案 B：使用 PyO3 或嵌入式 Python C-API。
  - 方案 C：在 Python 端启动 HTTP REST 服务器，Rust 发起 HTTP 请求。
- **决策原因**：
  1. 方案 A 的每次冷启动需要重新载入 Python 解释器、连接 SQLite 并扫描 auth 文件，在 Windows 下延迟高达 200~500ms，仪表盘刷新明显卡顿。
  2. 方案 B（PyO3）要求强绑定特定 Python 动态链接库，导致跨机器部署和构建极度脆弱，丧失通用性。
  3. 方案 C（HTTP）需要占用随机或固定端口，存在端口被占用、Windows 防火墙弹窗警告等体验问题。
  4. **最终方案**：通过标准输入输出 (stdio) 的 JSON-RPC 2.0 协议维护常驻子进程，兼顾极低延迟（<10ms IPC）、零端口占用与高稳定性；配合 Windows Job Object 确保父进程退出时子进程立即终止。

---

## 3. 为什么必须强制 Profile `cli_auth_credentials_store = "file"`？

- **决策时间**：多账号沙箱机制确立期
- **踩坑现象**：早期尝试直接替换环境变量或临时写入 SQLite，但在某些系统上，官方 Codex CLI 会自动把 Token 注册到系统的 Windows Credential Manager 或 macOS Keychain 中。这导致即使切换了文件，Codex 仍从全局凭证库读取旧账号，切号完全失效甚至互相污染。
- **解决方案**：
  - 为每个 Profile 单独创建沙箱目录，并硬性写入 `config.toml`，配置 `cli_auth_credentials_store = "file"`。
  - 在执行 `codex app-server` 时传入独立的 `CODEX_HOME`，强行切断官方 CLI 对系统凭证库的访问，保证多账号 100% 隔离。

---

## 4. 为什么对齐“剩余可用百分比 (Remaining %)”而非“已使用百分比 (Used %)”？

- **决策时间**：v0.2.0 UI 改版
- **背景**：早期版本直接显示 `primary_used_percent`（例如“已用 100%”）。
- **用户反馈**：当已用达到 100% 时，用户第一反应是不直观，不知道自己“还剩多少”。同时官方 Web/App 界面统一展示的是“Usage Remaining”（剩余 0%、剩余 45%）以及可用的“重置机会 (Resets)”。
- **决策**：数据存储层保留底层原始数据，但在展现层、CLI 以及 GUI 仪表盘上，100% 与官方语义对齐，改为显示剩余额度与可用重置轮次，完全消除用户的认知差异。

---

## 5. 自动刷新配置集中化与冷启动并发防冲突机制

- **决策时间**：v0.2.0 配额自动化迭代
- **背景与问题**：
  1. 顶部 Header 放置倒计时胶囊与按钮虽然显眼，但造成顶部导航栏拥挤杂乱，违背简约设计风格。
  2. 用户希望打开软件时立即拉取一次最新配额，但若启动时与周期定时器（Timer）同时触发，易引发底层并发探测冲突或向官方接口发送重复冗余请求。
- **决策与方案**：
  - 将定时刷新开关、刷新间隔周期（预设 5/10/15/30/60m 及自定义输入）、冷启动自动刷新、刷新通知等配置完全收拢至“设置与关于”选项卡中。
  - 顶部导航栏恢复为纯粹的“重启 Codex”与“刷新配额”核心动作，界面清爽聚焦。
  - 引入互斥状态与完成锚点时间戳机制：启动自动刷新与周期定时器统一受 `refreshingRef` 互斥保护，定时器必须在启动刷新完全结束、获得时间戳锚点后才开始累计间隔时间，彻底杜绝冷启动与定时任务的竞态或重叠。

---

## 6. 切号与重启解耦及重启未运行时主动启动拉起机制

- **决策时间**：v0.2.0 配额与进程管理优化
- **背景与问题**：
  1. 早期切号设计默认联动 `restart=True`，导致用户即使只在 CLI 或编辑器中工作，切号也会试图重启宿主；而若此时桌面端未启动，内部只静默清理了后台 daemon，造成“切号弹窗提示已重启 Codex，但实际上并无窗口”的割裂。
  2. 当 Codex 桌面端未运行时，用户在界面主动点击“重启 Codex”时，因内部 `was_codex_app` 为 `False`，拉起逻辑被跳过，导致用户感觉按钮失灵或非启动状态下无法启动 Codex。
- **决策与方案**：
  1. **切号与重启彻底解耦**：切号（`switch_account`）仅负责凭据原子覆写与状态同步（`restart: false`），毫秒级完成，不打扰当前工作流。
  2. **重启支持非运行下拉起**：`restart_codex` / `restart_codex_system` 默认支持 `start_if_not_running=True`。在 Codex 未运行状态下触发重启时，自动调用官方入口拉起 Codex 桌面客户端，并给出明确提示“Codex 桌面应用已启动。”。

---

## 7. 为什么配置存储坚决由 SQLite 迁移至纯文本 `config.json`？

- **决策时间**：v0.2.0 配置架构重构
- **背景与问题**：
  1. 原设计在 `codexq.db` 中建立 `app_settings` 表存储 Key-Value。对于只有 1 份应用设置的客户端系统，存入二进制 SQLite 导致用户无法直接用记事本或 VS Code 查看和修改，设置状态如“黑盒”不可控。
  2. 考虑过 YAML（语法优雅）或 TOML（Rust 常用），但在 Python 生态中：
     - `PyYAML` 是包含 C-Extension 的庞大第三方依赖（约 1.2MB 且需预编译），引入会彻底击穿“Python 端纯标准库零外部依赖”的铁律；
     - Python 3.10 标准库无 `tomllib`（3.11 引入且只读不支持回写）。
- **决策与方案**：
  - 选用 Python 3.10+ 标准库内置的 `json`，采用 `indent=2` 格式化保存至 `~/.codexq/config.json`。
  - 零外部依赖，天然支持层级对象，人手可读可写，支持原子替换（Temp + fsync + replace）防崩溃损坏。
  - 自动化双向兼容：支持老版本数据库配置平滑迁移，向前兼容 dot-notation（`trigger.default_model` 等）。

---

## 8. 定时触发（测试与自动化）差异化超时与强制执行设计

- **决策时间**：v0.2.0 定时触发调优
- **背景与问题**：
  1. 自动化闹钟主要目标是错峰激活 5H 额度窗口，若当前窗口已活跃，出于节省额度考虑应当跳过；但用户在界面点击“测试触发”时，是希望立刻验证网络、认证和模型连通性，此时若被“窗口活跃”拦截会误以为按钮故障。
  2. 桌面端到 Python RPC 原先设置了统一的 12s 超时；而触发真实大模型交互（如 `codex exec`）常耗时 15~40s，导致测试触发频繁被客户端提前切断报错 `RPC request timed out`。
- **决策与方案**：
  - **差异化行为**：手动“测试触发”强制传入 `force: true`，绕过窗口活跃检测直接执行真实问候测试；自动化排期闹钟则默认保留跳过机制。
  - **分级专用超时 (Tiered Timeouts)**：Rust `PyRpcClient` 引入按方法分级超时：常规 CRUD 保持 10s，切号 20s，配额探测 45s，而模型交互 `trigger_warmup` 专属放宽至 90s，从根本上解决超时截断问题。

---

## 9. 全栈中英双语国际化 (i18n) 架构设计与跨层协作

- **决策时间**：v0.2.0 国际化架构建设
- **背景与考量**：
  1. **基线与兜底策略**：必须坚持英文（`en-US`）为全局第一公民、基线代码和安全兜底（Fallback），确保在任何缺键或未知语言环境下应用绝对稳定可用；其他语言（如简体中文 `zh-CN`）均以独立外挂语言包方式维护。
  2. **配置持久化单一事实来源**：统一在 `~/.codexq/config.json` 的 `general.locale` 字段中存储（`auto`、`zh-CN`、`en-US`），保持人手可读可写。
  3. **Python 零依赖约束**：Python 核心坚决不引入 `gettext` 或 `Babel`，保持标准库纯洁性。底层仅负责配置存取与标准机器状态码输出，所有呈现层与提示语由前端及 Tauri 托盘根据所选语言负责渲染。
  4. **跨层动态响应**：用户在前端设置中切换语言时，前端即时热切换（无需刷新页面），并通过 Tauri 命令 `set_locale` 同步重绘系统托盘菜单（`tray.set_menu`），实现原生托盘与前端界面的无缝一致性。

---

## 10. RPC 服务端 `trigger_warmup` 提示词参数解析修复

- **决策时间**：v0.2.0 定时触发调优
- **背景与问题**：
  在定时触发界面点击「测试触发」时，前端调用 Tauri 命令 `trigger_warmup` 并向 Python RPC 发送请求。由于 Python 侧 `cmd_rpc` 分发 `trigger_warmup` 时遗漏了 `prompt = params.get("prompt")` 的变量赋值，导致直接调用 `await client.warmup(..., prompt=prompt, ...)` 时触发 Python 原生 `NameError: name 'prompt' is not defined`，进而导致前端 Toast 报错“测试触发失败：prompt is not defined”。
- **决策与方案**：
  1. 在 `codexq.py` 的 `cmd_rpc` 中严格提取并清洗 `prompt` 与 `model` 参数（空字符或未传时转为 `None`，以便底层正确回退到全局配置 `warmup.prompt` 与 `warmup.default_model`）。
  2. 在 `tests/test_rpc.py` 中新增真正的跨进程 stdio JSON-RPC 契约测试 `test_rpc_trigger_warmup_protocol`，杜绝此类变量未定义错误再次逃逸到生产。

---

## 11. 为什么全面重构为纯 Tauri 架构并扁平化目录？

- **决策时间**：v0.2.0 桌面端架构重大升级
- **背景与问题**：
  1. 早期桌面端采用双进程架构（Tauri Shell + Python stdio JSON-RPC 守护进程）。这意味着用户运行 CodexQ 桌面端时，本地必须安装有 Python 3.10+ 环境并配置好 PATH。一旦宿主机器缺乏 Python 或版本过低，桌面端完全无法启动。
  2. 双进程模式引入了复杂的 Windows Job Object 进程清理、无缓冲 stdio 管道和心跳重连机制，增加了运行时脆弱点。
  3. 工程结构上，原先将前端和 Tauri 嵌套在 `gui/` 内部，导致常规前端构建命令需额外 `cd gui`，且配置路径层级冗余。
- **决策与方案**：
  1. **目录扁平化**：将 `gui/` 整体提升至工程根目录，形成标准的 Tauri 2 现代化工程结构（根目录 `package.json`、`src/`、`src-tauri/`）。
  2. **纯 Rust 核心引擎**：在 `src-tauri/src/core/` 中使用纯 Rust（`rusqlite` + `tokio` + `chrono` + `sha2` + `base64`）原生实现 SQLite WAL 存储、沙箱隔离、JWT 解析、原子无损切号、`codex app-server` 异步探测与 5 小时错峰闹钟调度。
  3. **彻底脱钩 Python**：Tauri Commands 直接调用内部 Rust Core，完全删除 `rpc.rs` 与 Python 守护进程。产物为 100% 独立绿色的单二进制程序，用户开箱即用，零环境依赖。
  4. **保留 `codexq.py`**：将 `codexq.py` 保留为非 GUI 服务器环境或纯脚本调用的独立工具，但不进入桌面端分发产物。
