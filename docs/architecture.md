# CodexQ 系统架构与技术实现 (architecture.md)

本文档是 CodexQ 项目的权威架构与实现规范（Current Truth）。

---

## 1. 整体架构与数据流

CodexQ 桌面端采用现代化 **纯 Tauri 2 (Rust 原生核心 + React 表现层)** 架构，实现了进程内零 RPC 开销、零外部环境依赖的独立单二进制应用；底层与 **Codex CLI 子系统** 协同运作。其中第三方服务商的协议差异由进程内的**本地协议网关**（仅监听回环地址）承担，Codex CLI 侧永远只见到 `responses` 协议：

```mermaid
flowchart TD
    subgraph UI["桌面表现层 (React + TypeScript)"]
        ReactApp["React 19 Dashboard / Tray UI"]
    end

    subgraph DesktopHost["桌面壳层与原生核心 (Tauri 2 / Rust Core)"]
        TauriCmd["Tauri Native Commands"]
        RustCore["CodexQ Core Engine"]
        ProviderMgr["Provider Manager (core/provider.rs)"]
        Store["Store (rusqlite WAL + Profiles 沙箱)"]
        Scheduler["Tokio 异步错峰闹钟调度器"]
        Probe["Tokio 异步子进程探测池 (Semaphore)"]
        Gateway["Protocol Gateway (core/protocol_proxy)\n127.0.0.1 回环 · Responses ↔ Chat Completions"]
    end

    subgraph FileSystem["本地存储与凭据沙箱 (`~/.codexq`)"]
        DB[(codexq.db)]
        Profiles["profiles/<profile-id>/auth.json\n(强制 cli_auth_credentials_store = 'file')"]
        ProviderSecrets["providers/<id>/key + models.json\n(0600 密钥沙箱)"]
        Backups["backups/<ts>_<reason>/\n(运行时快照, 最多保留 20 份)"]
        Trash["trash/<profile-id>/"]
        ConfigJson["config.json (纯文本原子写入)"]
    end

    subgraph CodexSubsystem["宿主机 Codex CLI 系统"]
        CodexAuth["~/.codex/auth.json (当前活跃凭据)"]
        CodexConfig["~/.codex/config.toml\n(model_provider 路由 + 服务商表)"]
        ModelCatalogs["~/.codex/model-catalogs/\ncodexq-<id>.json (可查阅工件)"]
        AppServer["codex app-server (官方子进程)"]
        CodexExec["codex exec --ephemeral (无痕预热)"]
    end

    subgraph Upstream["第三方模型服务商 (公网)"]
        UpstreamResponses["原生 Responses 端点"]
        UpstreamChat["仅 Chat Completions 端点\n(DeepSeek / Kimi / GLM / Qwen …)"]
    end

    ReactApp <-->|Tauri IPC invoke| TauriCmd
    TauriCmd <--> RustCore
    RustCore --> Store
    RustCore --> ProviderMgr
    RustCore --> Scheduler
    RustCore --> Probe
    RustCore --> Gateway
    Store <--> DB
    Store <--> Profiles
    Store <--> Trash
    Store <--> ConfigJson
    ProviderMgr <--> ProviderSecrets
    ProviderMgr --> ModelCatalogs
    RustCore --> Backups
    RustCore <-->|自动感知 & 原子无损切号| CodexAuth
    RustCore <-->|服务商路由无损注入 (wire_api 恒为 responses)| CodexConfig
    Probe <-->|CODEX_HOME 隔离探测| AppServer
    Scheduler -->|定时触发| CodexExec
    CodexSubsystem -.->|chat 协议服务商: base_url 指向回环网关| Gateway
    Gateway -->|协议转换后转发| UpstreamChat
    CodexSubsystem -.->|responses 协议服务商: 直连不经过网关| UpstreamResponses
```

---

## 2. 存储规范与数据模型

默认工作与存储目录位于 `~/.codexq/`，整体目录物理结构如下：

```text
~/.codexq/
├── config.json               # 全局用户配置（纯文本 JSON，2 空格缩进，直接手写可读可编辑）
├── codexq.db                 # SQLite 数据库（启用 WAL 模式与外键约束，存储账号、额度快照与闹钟）
├── profiles/                 # 各账号的物理隔离沙箱
│   └── <profile-id>/         # 账号 profile_id（基于 identity_key 计算的 SHA256 前 20 位）
│       ├── auth.json         # 账号独立的凭据镜像（chmod 0600）
│       └── config.toml       # 配置文件（强制声明 cli_auth_credentials_store = "file"）
├── providers/                # 第三方模型服务商的密钥沙箱与模型目录
│   └── <provider-id>/        # 服务商 slug（规范化小写，冲突时追加 -2/-3 后缀）
│       ├── key               # 明文 API Key（chmod 0600，绝不写入 SQLite）
│       └── models.json       # 该服务商的 Codex 模型目录产物（chmod 0600）
├── backups/                  # 切换前运行时快照（auth.json + config.toml，最多保留 20 份）
│   └── <YYYYMMDD_HHMMSS>_<reason>/
└── trash/                    # 回收站隔离目录
    └── <profile-id>/
```

### 2.1 SQLite Schema 定义

`codexq.db` 包含六张核心数据表（账号、最新额度、快照历史、回收站、闹钟、第三方服务商）：

```sql
-- 1. 账号主表
CREATE TABLE IF NOT EXISTS accounts (
    identity_key TEXT PRIMARY KEY,          -- 唯一主键: f"{user_id}\x1f{account_id}"
    profile_id TEXT NOT NULL UNIQUE,        -- 20位 hex profile 目录名
    user_id TEXT NOT NULL,                  -- chatgpt_user_id
    account_id TEXT NOT NULL,               -- chatgpt_account_id
    email TEXT,                             -- 邮箱（从 id_token 解码）
    plan TEXT,                              -- 订阅类型 (free / plus / team / pro 等)
    alias TEXT,                             -- 用户自定义别名
    org_title TEXT,                         -- 机构/工作空间标题
    first_seen_at TEXT NOT NULL,            -- 首次收纳时间 (ISO 8601 UTC)
    last_seen_at TEXT NOT NULL,             -- 最近活动时间 (ISO 8601 UTC)
    last_credential_update TEXT NOT NULL,   -- 凭据最后变动时间
    credential_sha256 TEXT NOT NULL,        -- auth.json 内容哈希
    credential_path TEXT NOT NULL,          -- 本地 profile 凭据绝对路径
    credential_status TEXT NOT NULL DEFAULT 'active', -- active / expired / error
    reset_credits INTEGER DEFAULT 0,        -- 可用重置机会
    last_error TEXT,                        -- 最近探测异常信息
    UNIQUE(user_id, account_id)
);

-- 2. 最新额度状态表
CREATE TABLE IF NOT EXISTS quota_latest (
    identity_key TEXT NOT NULL,             -- 关联 accounts
    limit_id TEXT NOT NULL,                 -- 额度规则 ID (如 "codex")
    fetched_at TEXT NOT NULL,               -- 刷新时间
    primary_used_percent REAL,              -- 主周期已用百分比 (0.0 ~ 100.0)
    primary_window_minutes INTEGER,         -- 主周期窗口时长 (如 300 分钟 = 5H)
    primary_resets_at INTEGER,              -- 主周期重置时间戳 (Unix Epoch)
    secondary_used_percent REAL,            -- 次周期已用百分比 (如 10080 分钟 = 7天)
    secondary_window_minutes INTEGER,       -- 次周期窗口时长
    secondary_resets_at INTEGER,            -- 次周期重置时间戳
    raw_json TEXT NOT NULL,                 -- app-server 原始返回快照
    PRIMARY KEY(identity_key, limit_id),
    FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
);

-- 3. 额度历史快照表 (时间序列)
CREATE TABLE IF NOT EXISTS quota_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    identity_key TEXT NOT NULL,
    limit_id TEXT NOT NULL,
    observed_at TEXT NOT NULL,              -- 观测时间 (ISO 8601 UTC)
    primary_used_percent REAL,
    primary_window_minutes INTEGER,
    primary_resets_at INTEGER,
    secondary_used_percent REAL,
    secondary_window_minutes INTEGER,
    secondary_resets_at INTEGER,
    raw_json TEXT NOT NULL,
    FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_quota_snapshots_identity_time
    ON quota_snapshots(identity_key, observed_at);

-- 4. 软删除回收站记录表
CREATE TABLE IF NOT EXISTS removed_accounts (
    identity_key TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL,
    email TEXT,
    user_id TEXT,
    plan TEXT,
    display_name TEXT,
    removed_at TEXT NOT NULL
);

-- 5. 账号独立定时预热/触发闹钟表
CREATE TABLE IF NOT EXISTS account_alarms (
    id TEXT PRIMARY KEY,                    -- 闹钟唯一标识 (如 "alm_1726123456")
    identity_key TEXT NOT NULL,             -- 关联 accounts 表
    time_of_day TEXT NOT NULL,              -- 触发时间点 "HH:MM" (如 "08:00")
    days_of_week TEXT NOT NULL DEFAULT '1,2,3,4,5', -- 重复周期/模式 ("1,2,3,4,5" 为工作日, "1,2,3,4,5,6,7" 为每天, "once" 为仅一次)
    enabled INTEGER NOT NULL DEFAULT 1,     -- 开关: 1 启用, 0 禁用 (一次性闹钟触发后自动置为 0)
    model_override TEXT,                    -- 专用模型覆盖 (为空则继承全局 warmup.default_model)
    prompt_override TEXT,                   -- 专用提示词覆盖 (为空则继承全局 warmup.prompt)
    last_triggered_at TEXT,                 -- 上次触发时间 (ISO 8601 UTC)
    last_status TEXT,                       -- 上次状态: success / failed / skipped
    created_at TEXT NOT NULL,               -- 创建时间 (ISO 8601 UTC)
    FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_account_alarms_identity
    ON account_alarms(identity_key);

-- 6. 第三方 AI 服务商元数据表（明文密钥绝不入库）
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,                        -- 服务商 slug（规范化小写，冲突时追加 -2/-3）
    name TEXT NOT NULL,                         -- 展示名称
    base_url TEXT NOT NULL,                     -- OpenAI 兼容端点基址
    wire_api TEXT NOT NULL DEFAULT 'responses', -- 服务商级默认上游线协议 (responses / chat)，仅在 gateway_enabled 时作为模型的缺省协议；永不透传进 Codex 配置
    gateway_enabled INTEGER NOT NULL DEFAULT 0, -- 本地协议网关总开关；0 时全部模型强制 responses 直连并清空协议覆盖
    active_model TEXT NOT NULL,                 -- 当前激活模型 slug
    models_json TEXT NOT NULL DEFAULT '[]',     -- 模型池 JSON 数组
    context_window INTEGER DEFAULT 256000,      -- 工作窗口（硬性保底 256k）
    model_context_windows TEXT DEFAULT '{}',    -- 单模型窗口覆盖 {"model": tokens}
    reasoning_levels TEXT,                      -- 服务商级推理档位 JSON 数组
    model_reasoning_levels TEXT,                -- 单模型推理档位覆盖 {"model": [levels]}
    model_wire_apis TEXT,                       -- 单模型线协议覆盖 {"model": "responses"|"chat"}；缺省继承 wire_api；gateway_enabled=0 时保存过程清空
    notes TEXT,                                 -- 备注说明
    custom_config_toml TEXT,                    -- 高级：自定义注入的 TOML 片段
    custom_auth_json TEXT,                      -- 高级：自定义写入的 auth.json
    key_masked TEXT NOT NULL DEFAULT '',        -- 掩码展示值（如 sk-ab****cdef）
    key_sha256 TEXT NOT NULL DEFAULT '',        -- 密钥 SHA256 校验和（用于变更检测）
    created_at TEXT NOT NULL,                   -- 创建时间 (ISO 8601 UTC)
    updated_at TEXT NOT NULL                    -- 最近更新时间 (ISO 8601 UTC)
);
```

> **跨端 schema 契约**：Rust 与 Python 共用同一份 `codexq.db`，两侧 `CREATE TABLE providers` 必须逐列兼容，新增列一律通过 `ALTER TABLE ... ADD COLUMN` 幂等兜底（含 `DEFAULT`）以兼容旧库。历史上曾因两侧列集不一致触发 `NOT NULL constraint failed: providers.key_masked`，详见 `docs/work/history.md` 第 15 节。

### 2.2 全局配置文件规范 (`~/.codexq/config.json`)

为保持配置的高透明度与可维护性，CodexQ 将全局静态设置从数据库中剥离，集中存储于纯文本 JSON 文件 `~/.codexq/config.json`：

- **单源真实与原子写入**：所有配置写入均通过“临时文件写盘 + `os.fsync` 刷盘 + `os.replace` 原子覆写”完成，防止断电或并发写损坏。
- **纯文本与手写友好**：格式化为 2 空格缩进，用户可直接使用 VS Code、记事本等工具直接查阅与编辑，开箱即懂。
- **配置结构标准定义**：

```json
{
  "version": 1,
  "trigger": {
    "default_model": "gpt-5.6-luna",
    "preset_models": [
      "gpt-5.6-luna",
      "gpt-5.2-preview",
      "gpt-5",
      "gpt-5.1"
    ],
    "prompt": "hi",
    "skip_if_active": true
  },
  "auto_refresh": {
    "enabled": false,
    "interval_minutes": 15,
    "on_startup": true,
    "notify": false,
    "dynamic_reset_enabled": true,
    "notify_on_quota_restored": true
  }
}
```

---

## 3. Profile 沙箱隔离与 Token 安全生命周期

### 3.1 凭据隔离机制
官方 Codex CLI 在默认配置下可能会使用系统的 Keyring / Credential Manager 存储 Token。为了实现完全可控、无冲突的多账号隔离：
- 每个账号拥有专属目录：`~/.codexq/profiles/<profile-id>/`。
- 在每个 profile 目录下初始化 `config.toml`：
  ```toml
  cli_auth_credentials_store = "file"
  ```
- 当以该 profile 查询额度时，指定环境变量 `CODEX_HOME=<profile-dir>`。因此 `codex app-server` 会将自动刷新后的 OAuth Token 写回到该 profile 的 `auth.json`，不影响外部系统。

### 3.2 切号防丢保护流程 (`switch`)
切号操作涉及真实的 `~/.codex/auth.json` 替换，核心逻辑实施**原子保障**：
1. **预检与归档**：检查当前宿主机 `~/.codex/auth.json`。若存在有效登录，计算其最新 Token 哈希，原位同步写回对应账号的 `profiles/<current-profile-id>/auth.json`，确保运行期间刷新的最新 access/refresh token 不被截断遗失。
2. **目标账号主动自愈**：切换前检测目标账号的 `access_token` 有效期；若已过期或即将到期（< 5分钟），自动调用 OAuth 续期协议换取新 Token 并写回目标 Profile，确保切号即用，避免切号后处于 401 故障态。
3. **原子覆写**：从目标账号的 profile 目录读取最新 `auth.json`，通过原子写入临时文件并重命名的方式更新 `~/.codex/auth.json`。
4. **状态标记**：更新数据库中的 `last_seen_at` 记录。
5. **解耦原则**：切号操作仅负责凭据的原子覆写与状态同步，不再强制触发外部进程重启，避免打扰用户当前工作流。

### 3.3 OAuth Token 自动续期协议 (RTR Auto-Renewal)
OpenAI 官方客户端采用 **Refresh Token Rotation (RTR)** 机制维护认证生命周期：
- **认证端点**：主端点 `https://auth.openai.com/oauth/token`（备用容灾端点 `https://auth0.openai.com/oauth/token`）。
- **客户端凭证标识**：`client_id: "app_EMoamEEZ73f0CkXaXp7hrann"`（从官方 JWT `id_token` 的 `aud` 或 `access_token` 的 `client_id` 声明动态解析，保证向前兼容）。
- **换票请求格式**：
  ```json
  {
    "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
    "grant_type": "refresh_token",
    "refresh_token": "<refresh_token>"
  }
  ```
- **双重触发机制 (Proactive & Reactive)**：
  1. **主动预测续期 (Proactive Renewal)**：在进行额度探测、定时预热触发或账号切换前，解码 `tokens.access_token` JWT 载荷中的 `exp` 声明。若当前时间距离过期不足 5 分钟，自动静默触发换票更新。
  2. **响应式自愈重试 (Reactive Fallback)**：若 `codex app-server` 的 `account/rateLimits/read` 接口返回 401 Unauthorized / Token Expired 异常，拦截器自动捕获并执行换票；换票成功后立即自动重试一次额度探测，实现无感自愈。
- **原子凭证落盘**：换票成功后，OpenAI 返回全新的 `access_token` 与轮换后的 `refresh_token`。CodexQ 立即更新 Profile `auth.json` 与 `last_refresh` 时间戳，原子重命名替换文件；更新 SQLite 中的 SHA256 与有效状态；若该账号处于激活态，同步原子刷新宿主 `~/.codex/auth.json`。

### 3.4 历史备份智能吸纳机制 (Bi-directional Backup Sync)
官方 Codex CLI 在登录或认证变动时会在 `~/.codex/backups/<timestamp>/auth.json` 留存凭据快照：
- **无损吸收原则**：扫描全量备份目录，根据 `chatgpt_user_id` 与 `account_id` 分组识别候选凭证。
- **时间序与内容防降级**：
  1. 候选凭证之间比对 `last_refresh` ISO 时间戳（若缺失则回退至 JWT `exp` 声明），仅保留最新的候选备份。
  2. 候选凭证与本地现有 Profile 比对：通过 `is_auth_newer_or_equal(candidate, existing)` 严格阻断旧凭证覆盖新凭证。
- **状态重激活**：当检测到备份中有更新的凭证且 SHA256 发生改变时，自动将凭证同步吸纳至 Profile，并把数据库中的 `credential_status` 重新拉回 `active`、清空 `last_error`。
- **用户显式删除保护**：被用户显式移入回收站 (`removed_accounts`) 的账号被严格忽略，防止因扫描历史备份而意外复活。

### 3.5 第三方服务商密钥沙箱 (Provider Key Sandboxing)
与官方账号凭据同等严格，第三方服务商的明文 API Key 不进入 SQLite：
- **文件即密钥**：明文 Key 仅写入 `~/.codexq/providers/<provider-id>/key`，以 `0600` 权限落盘；数据库 `providers` 表仅保存 `key_masked`（显示掩码）与 `key_sha256`（变更检测校验和）。
- **掩码规则（Rust / Python 一致）**：`sk-` 前缀保留前 5 字符，其他前缀保留前 3 字符，尾部固定保留 4 字符，中间以 `****` 替换；长度 ≤ 8 的密钥整体显示为 `****`。
- **空密钥保留语义**：编辑服务商时若未重填 Key，`upsert_provider` 必须保留原有 `key_sha256` / `key_masked`，避免静默清空校验和（Rust 侧通过 `CASE WHEN excluded.key_sha256 = '' THEN providers.key_sha256` 实现）。
- **删除即抹除**：`delete_provider` 同时移除数据库记录与整个 `providers/<id>/` 沙箱目录，不留明文残余。

> 运行时槽位切换（`switch_to_provider` / 切回官方）与 `config.toml` 注入细节见第 8 章。

---

## 4. 通信契约与指令层

### 4.1 GUI 前端与 Rust 原生核心交互：Tauri Native Commands (In-Process IPC)
- **传输模式**：前端 React 通过 `@tauri-apps/api/core` 的 `invoke` 与 Rust 进程直接通信，**零跨进程 JSON-RPC 管道延迟，零端口占用，零 Python 运行时依赖**。
- **支持的核心 Tauri Commands**：
  - `list_accounts`: 获取全量账号列表与最新额度。
  - `refresh_all(concurrency: Option<u32>)`: 触发多账号并发刷新（基于 Tokio 信号量控制并发池）。
  - `switch_account(target: String, restart: Option<bool>)`: 切换当前账号凭据（默认 `restart=false`，切号不触发重启）。
  - `restart_codex(relaunch: Option<bool>, start_if_not_running: Option<bool>)`: 重启外部 Codex 宿主进程；若未运行且 `start_if_not_running=true`（默认），则直接拉起启动 Codex 桌面端。
  - `set_alias(target: String, alias: Option<String>)`: 更新别名。
  - `reset_all_aliases()`: 重置所有别名。
  - `get_history(target: String, limit: Option<u32>)`: 查询账号额度快照历史。
  - `remove_account(target: String)`: 软删除移入回收站。
  - `list_trash()`: 查看回收站列表。
  - `restore_account(target: String)`: 从回收站恢复。
  - `purge_trash(target: Option<String>)`: 物理彻底清理。
  - `get_app_settings()`: 获取全局偏好配置字典。
  - `set_app_setting(key: String, value: String)`: 写入全局配置项。
  - `list_account_alarms(target: Option<String>)`: 获取指定账号或全量账号的预热闹钟列表。
  - `save_account_alarm(alarm: Value)`: 新增或更新账号预热闹钟（强校验 5 小时非重合约束）。
  - `delete_account_alarm(id: String)`: 删除指定闹钟。
  - `trigger_warmup(target, model, prompt, force, timeout)`: 立即触发单次预热问候请求。
  - `set_locale(locale: String)`: 动态重绘系统原生托盘菜单语言。
  - `list_providers()`: 获取第三方服务商列表（仅掩码，明文密钥永不返回）。
  - `save_provider(payload: SaveProviderPayload)`: 新增或更新服务商；密钥写入独立 `0600` 沙箱文件，DB 仅存掩码与 SHA256。
  - `delete_provider(id: String)`: 删除服务商记录、密钥沙箱与 catalog 产物。
  - `test_provider_connectivity(base_url, api_key, ...)`: 探测 OpenAI 兼容端点 `GET /models`，返回状态码、延时与可用模型列表。
  - `switch_to_provider(provider_id, model_override, restart)`: 原子切换到第三方服务商运行时槽位。
  - `get_active_runtime_mode()`: 读取宿主机 `config.toml`，判定当前处于官方账号模式还是第三方服务商模式。

### 4.2 Rust 核心与 Codex CLI 交互：`codex app-server`
- Rust 通过 `tokio::process::Command` 启动后台子进程 `codex app-server --stdio`（Windows 平台启用 `CREATE_NO_WINDOW` 0 弹窗闪烁）。
- 通过 stdio JSON-RPC 发送握手指令（`initialize` -> `initialized` -> `account/rateLimits/read`）并监听响应，提取限额窗口 (`window_minutes`)、已使用比率 (`used_percent`) 和重置时间 (`resets_at`)。

---

## 5. 客户端额度调度策略与生命周期 (Client Quota Scheduling & Lifecycle)

为了兼顾数据的实时性、系统的低功耗以及对 OpenAI 官方接口的访问合规性，客户端采用**混合事件调度模型（Hybrid Quota Scheduling）**：

```text
                     ┌──────────────────────────────────────┐
                     │ 刷新事件发生 (手动 / 启动 / 周期 / JIT) │
                     └──────────────────┬───────────────────┘
                                        │
                                        ▼
                   ┌──────────────────────────────────────────┐
                   │ 更新 lastRefreshedTime = Date.now()       │
                   │ (周期轮询定时器自动顺延一个完整周期)       │
                   └────────────────────┬─────────────────────┘
                                        │
                                        ▼
                   ┌──────────────────────────────────────────┐
                   │ 取消并销毁旧的 JIT 到期 Timer (clearTimeout)│
                   └────────────────────┬─────────────────────┘
                                        │
                                        ▼
                   ┌──────────────────────────────────────────┐
                   │ 额度状态比对：检测是否有账号从耗尽恢复满血 │
                   │ (若开启 notifyOnQuotaRestored 则发系统通知)│
                   └────────────────────┬─────────────────────┘
                                        │
                                        ▼
                   ┌──────────────────────────────────────────┐
                   │ 扫描非满血账号，获取全池【下一个最早到期时间 T】│
                   │ 若存在，仅注册 1 个 JIT Timer 于 T + 10s   │
                   └──────────────────────────────────────────┘
```

### 5.1 三阶混合刷新机制
1. **冷启动立即刷新 (Startup Refresh)**：
   - 应用启动先呈现 SQLite 本地秒级快照，随后后台发起冷启动刷新，保证初始数据新鲜度。
2. **常规周期轮询 (Periodic Polling)**：
   - 用户可配置 5m/10m/15m/30m/60m 或自定义周期，作为长效背景保底更新。
3. **JIT 到期动态自感知刷新 (Just-In-Time Reset Refresh)**：
   - 提取各账号返回的 `primary.resets_at`（5小时主周期）和 `secondary.resets_at`（周次周期）。
   - 仅针对当前有额度消耗（`used_percent > 0`）且未过期的账号进行到期监听。
   - 设定在最早到期时间点后的 **10 秒缓冲期**（`resets_at + 10s`）准时触发，彻底吸收官方服务器时钟微差与结算延迟。

### 5.2 定时器清空与防竞态重置 (Timer Invalidation & Cancellation)
- **绝对单源时间戳**：任何刷新完成时，全局基准时间戳更新为当前时间，阻止周期定时器短时间内重复跑。
- **旧定时器完全注销**：每一次刷新后，彻底清除上一轮挂载的 `setTimeout`，根据最新拉回的精确重置时间重新计算全池唯一的最早唤醒时刻。
- **并发互斥锁**：`refreshingRef` 保证任何时刻全局仅有 1 个并发探测任务执行。

### 5.3 满血复活感知与系统桌面通知分级
- **状态差分计算**：对比刷新前后的快照。当账号由“不可用/耗尽（可用额度 0% 或已用 ≥ 90%）”跃迁为“可用/满血（已用清零或大幅恢复）”时，识别为**满血复活事件**。
- **独立设置开关**：
  - `dynamicResetEnabled`: 是否启用 JIT 动态到期唤醒（默认开启）；
  - `notifyOnQuotaRestored`: 是否在满血复活时弹出 Windows 原生系统桌面通知（可配置）；
  - `notifyOnUpdate`: 是否在常规自动刷新后弹出界面 Toast（默认关闭，保持纯静默）。

---

## 6. 定时预热与多闹钟错峰流水线机制 (Scheduled Warmup & Staggered Pipeline)

为了解决开发者 10:00 上班与 5 小时滑动额度窗口错位导致的午后断粮问题，CodexQ 引入了基于纯标准库实现的无痕垫刀与流水线调度体系：

### 6.1 垫刀执行协议 (Execution Protocol)
- **执行方式**：在指定账号 Profile 沙箱下调用官方无头子进程：
  ```bash
  CODEX_HOME="~/.codexq/profiles/<profile_id>" codex exec --ephemeral --skip-git-repo-check -m "<model>" "<prompt>"
  ```
  - `--ephemeral`：不落地会话文件，不污染用户工作区的历史对话树。
  - `--skip-git-repo-check`：允许在任意非 Git 目录下秒级启动执行。
  - `-m <model>`：指定问候模型，默认使用 `gpt-5.6-luna`。
- **防浪费幂等校验（Skip-if-Active）**：
  - 触发预热前先从 SQLite 读取该账号的最新配额；
  - 若检测到当前配额处于活跃且未到期状态（`used_percent > 0` 且 `primary_resets_at > now`），自动跳过本次预热，避免多余 token 消耗。

### 6.2 多闹钟防重叠约束（Non-overlapping Constraint）
- 允许单账号配置多个排期闹钟（如工作日 08:00、13:30）；
- **5 小时安全约束校验**：任何两个启用闹钟之间的时间间隔必须满足 $\Delta t \ge 5\text{ 小时}$（即 300 分钟），在保存和配置时由引擎与前端协同强校验拦截，防止同账号短时间内重复触发导致窗口空耗。

### 6.3 动态常用推荐模型标签管理
- 全局默认模型支持任意文本输入（默认为 `gpt-5.6-luna`）；
- 维护可动态编辑的常用预设数组（序列化为 JSON 存入 `config.json` 的 `trigger.preset_models` 键）；
- 前端支持一键选取、动态新增预设标签和删除已有预设标签。

### 6.4 「仅一次」排期模式与生命周期自闭机制 (One-time Alarm Lifecycle)
- **重复模式扩展**：排期闹钟支持「仅一次」(`once`)、「工作日」(`1,2,3,4,5`) 和「每天」(`1,2,3,4,5,6,7`)。
- **生命周期自闭**：对于 `once` 模式的闹钟，在到达设定时间触发完成后（无论成功执行还是跳过），调度器自动将该闹钟的开关置为停用（`enabled = 0`），保留历史执行记录且不破坏数据，用户后续需要时可再次手动开启。
- **后台常驻轮询调度器**：
  - **桌面端 (Rust Core)**：在应用启动时由 `src-tauri/src/core/scheduler.rs` 挂载 Tokio 异步后台任务，每 15 秒轮询检查一次满足时钟触发条件的激活闹钟，并具备 75 秒防同分钟重复触发保护；
### 6.5 5小时配额恢复自动续窗机制 (Auto-Rollover on 5h Quota Restore)
- **业务背景**：Codex 5 小时速率限制滑动窗口仅在接收到首个请求时才开始计算到期倒计时。若额度恢复后长时间无请求介入，将浪费宝贵的窗口滚动时间。
- **状态流转与双重条件校验**：
  1. **5小时窗口恢复检测**：账号窗口非活跃（`primary_resets_at <= now` 或未在计时，或最新额度已清零 `primary_used_percent == 0`）；
  2. **周剩余额度保底防线**：用户可为账号配置周最低剩余额度门限（`min_weekly_remaining`，默认为 0.0%）。当前账号周剩余额度 `secondary_remaining = (100.0 - secondary_used_percent)` 必须满足 `secondary_remaining >= min_weekly_remaining`（若门限为 0 则要求 `secondary_remaining > 0`）。若周额度耗尽或已低于保底门限，坚决拦截不触发，防止产生无意义失败或透支宝贵的周额度；
  3. **防循环自锁机制**：触发成功后即时触发 `refresh_one`，使本地 `quota_latest` 更新并进入新一轮 5 小时活跃状态；同时设立 15 分钟最小触发冷却间隔，杜绝网络波动或异常情况下的重复频发。
- **账号级精细化配置与交互**：
  - 配置存储在 `config.json` 的 `trigger.account_rollovers[identity_key]` 中，支持按账号独立开关与设置周最低剩余额度（`min_weekly_remaining`）；
  - 前端界面收敛在「定时触发」页面中，随上方账号选择器切换即时联动当前账号的自动续窗卡片与状态指示；
  - 实时展示当前账号的周剩余额度（如 `75.0%`）及就绪/拦截/耗尽状态标签，直观透明。

---

## 7. 国际化与双语架构设计 (Internationalization & i18n Architecture)

CodexQ 采用工业级多语言分层架构：**英文基线兜底、前端驱动自然语言、后端声明结构化语义**。

### 7.1 全局语言配置真理源 (`~/.codexq/config.json`)
在配置文件根节点维护 `general.locale`：
```json
{
  "$schema_version": 1,
  "general": {
    "locale": "auto"
  },
  "auto_refresh": { ... },
  "trigger": { ... }
}
```
- `"auto"`（默认）：启动时自适应检测宿主操作系统或浏览器语言环境；若为中文则加载 `zh-CN`，其他统一进入默认英文 `en-US`。
- `"en-US"`：强制全系统英文。
- `"zh-CN"`：强制全系统简体中文。

### 7.2 前端 UI 层 (React 19 + `i18next`)
- **基线兜底 (`fallbackLng: "en-US"`)**：任何缺失或未翻译的 Key 自动回退至标准英文，保证 UI 不留白、不泄露原始键名。
- **语言包物理分布**：
  - `src/i18n/locales/en-US.json`：全量英文标准字典（Source of Truth）。
  - `src/i18n/locales/zh-CN.json`：简体中文资源包。
- **即时响应式热切换**：用户在【设置与关于】切换语言后，React 组件树无需重载页面即可实时重绘；并联动持久化至 `config.json`。

### 7.3 系统壳层 (Tauri 2 / Rust)
- **托盘菜单 (System Tray)**：定义 `Locale` 枚举映射双语菜单文本（"Show Dashboard" / "显示主窗口"、"Refresh All" / "刷新配额"、"Quit" / "退出"）。
- **动态重绘**：前端切换语言时调用 `set_locale` 命令，Rust 端就地调用 `tray.set_menu(...)` 完成托盘毫秒级无感热切换。
- **原生通知**：前端调用通知插件时已传入本地化文本；Rust 内部日志与系统级严重异常默认输出英文。

### 7.4 核心引擎层 (Python Core `codexq.py`)
> **冻结维护 (Frozen)**：本层自 v1.0.x 起进入冻结状态，仅接受阻塞级缺陷与安全修复，不再承接新功能。本节与 §8.x 中涉及 Python 的描述均为**历史行为记录**，用于解释现有数据格式；新能力一律落在桌面端，不再要求 Python 端对等实现。

- **零依赖守则**：严禁引入第三方 gettext 或 Babel 编译依赖，维持单文件免安装即用。
- **RPC 机器码解耦**：
  ```python
  # RPC 返回标准化结构
  {
      "success": True,
      "status": "skipped",
      "code": "WINDOW_ALREADY_ACTIVE",
      "params": {"resets_at": "19:40"},
      "message": "Quota window is already active (resets at 19:40). Skipped."
  }
  ```
  前端优先按 `code` 查本地语言包渲染；未匹配时优雅降级展示英文 `message`。
- **CLI 终端输出**：默认输出英文对齐官方 Codex CLI；可选读取 `config.json` 针对终端表格和提示进行轻量字典映射。

---

## 8. 第三方 AI 服务商接入与统一运行时槽位 (Third-Party Providers & Unified Runtime Slot)

### 8.1 设计目标与边界
CodexQ 在保留官方多账号 5H/周度额度探针与防损切号能力的同时，支持接入任意兼容 OpenAI 规范的第三方模型端点（DeepSeek、StepFun、SiliconFlow、OpenRouter 等）：
- **上游线协议与本地路由总开关**：服务商自身可以是 `responses` 原生端点，也可以只提供 `chat/completions`。编辑器顶部提供「本地协议网关」开启/关闭总开关（`gateway_enabled`）：关闭时所有模型统一按 `responses` 直连，提交时将服务商 `wire_api` 归一为 `responses` 并清空 `model_wire_apis`；开启时才显示逐模型协议选择。开启但全模型均为 `responses` 仍直连且不启动监听进程。旧库升级时按旧 `wire_api` / 覆盖值推导一次开关。
- **协议粒度是「模型」而非「服务商」**：同一个 `base_url` + 同一个密钥下，不同模型可能支持不同协议（`opencode-go` 即为典型：Grok / GPT / Muse 走 `/v1/responses`，GLM / Kimi / DeepSeek / MiMo 只在 `/v1/chat/completions`）。因此 `provider.wire_api` 只是**服务商级默认值**，可用 `model_wire_apis` 逐模型覆盖（§8.6）。
- **对 Codex 恒为 responses**：无论上游是什么协议，写入 `~/.codex/config.toml` 的 `wire_api` **永远是 `"responses"`**。Codex 已于 2026 年 2 月彻底移除 chat 线协议（openai/codex discussion #7782），任何其它取值都会让**整份配置**反序列化失败，进而使 `codex exec` / `codex login status` 等全部命令不可用。协议差异由 §8.6 的本地协议网关内部消化。
- **网关严格限于本机回环**：网关只绑定 `127.0.0.1`，拒绝非本机来源请求，不缓存、不落盘任何请求体或响应体；不做多机节点调度、不做公网暴露、不做账号池负载均衡（与 PROJECT.md §3 非目标一致）。
- **互斥的单一槽位**：`config.toml` 顶层只有一个 `model_provider` 槽位，因此「官方账号模式」与「第三方服务商模式」互斥，`get_active_runtime_mode` 是判定当前模式的唯一真理源。
- **直连 vs 网关由总开关与协议共同决定**：总开关关闭，或总开关开启但服务商下没有任何 chat 模型时，`base_url` 保持上游直连且网关不启动；总开关开启且存在任意一个 chat 模型时，该服务商的 `base_url` 统一改写为回环网关地址（因为一个 Codex provider 块只能有一个 `base_url`），其中的 responses 模型在网关内**原样透传**（§8.6）。

> **为什么不在写 `config.toml` 时就决定协议？** 因为 Codex 允许用户在会话中随时用 `/model` 切换模型，而 CodexQ 只在**显式切号**时改写配置。一个 `base_url` 要同时服务混合协议的服务商，协议就必须在**每次请求**时按 `model` 字段解析——这正是 §8.6 的 `resolve_active_route_for_model` 存在的原因。


### 8.2 运行时槽位切换状态机
`core/switch.rs` 以 `ActiveRuntimeMode`（`Official` / `Provider`）为中心实现双向原子切换：

**切到第三方（`switch_to_provider`）**：
1. `ensure_host_codex_config()` 确保宿主机 `config.toml` 声明 `cli_auth_credentials_store = "file"`；
2. `create_runtime_snapshot("to_provider_<id>")` 快照当前 `auth.json` + `config.toml`；
3. 若当前处于官方模式，`auto_sync_current()` 先把活跃 Token 归档回其 Profile，防止切走时丢失刷新后的凭据；
4. 载入服务商元数据与沙箱密钥，解析生效模型（`model_override` 优先，否则 `active_model`）；
5. 生成模型目录产物；
6. 以 `toml_edit` 无损注入 `config.toml`（详见 8.3）；
7. 仅当配置了 `custom_auth_json` 时才覆写活跃 `auth.json`，否则**保留官方凭据原样**（不破坏官方登录态）。

**切回官方（`switch_account`）**：
- 仅当顶层 `model_provider` 指向第三方服务商时才清理活动路由：移除顶层 `model` / `model_provider`；若该 provider 由 CodexQ 管理，则保留同 id 的最小占位表（非空 `name`、`wire_api = "responses"`、不可连接的 `http://127.0.0.1:1/v1`，不含密钥或自定义字段），让已保存的 Codex Desktop 会话仍能解析 provider id，同时避免旧会话在官方模式下继续访问第三方；未知 provider 仍删除其表；并清理 CodexQ 自己生成的 `model_catalog_json`；
- 桌面端启动时，根据 SQLite 中已登记的服务商补齐/脱敏所有非活动 provider 占位表，修复旧版本已删除 provider 表的安装；当前活动 provider 保持原样；
- 用户手工配置的其它 `model_catalog_json` 路径保持不变；
- 若 `model_provider` 缺失、为 `"openai"` 或指向未知 id，则视为官方模式，**保留用户自定义的 `model` 与其它配置**；
- 之后按 §3.2 的原子流程写入目标账号凭据。

> **Python 端冻结差异**：`codexq.py` 不新增历史会话兼容能力。桌面端的 provider 占位表与网关路由只由 Rust Core 管理。

**快照有界**：`MAX_RUNTIME_SNAPSHOTS = 20`，每次切换后裁剪最旧的 `~/.codexq/backups/<ts>_<reason>`，避免明文凭据备份无限增长。

**官方 id 归一**：`is_official_provider_id()` 将 `openai` 视为官方路由，避免把官方默认配置误判为第三方模式（Rust / Python 行为一致）。

### 8.3 `config.toml` 无损注入与模型目录
- **Rust 端**：使用 `toml_edit::DocumentMut` 就地修改承载用户配置的文档树，完整保留注释、缩进与未触碰的表（含 MCP 服务声明）。
- **Python 端**：受 Python 3.10+ 纯标准库约束（`tomllib` 只读），采用逐行解析实现等价语义：顶层键就地替换/插入，`[model_providers.<id>]` 表整表重写；`lift_codex_config_provider()` 反向操作时按表头精确匹配（`model_providers.<id>` 及其 `.<id>.` 子表）整表删除，不影响相邻服务商表，其余行原样透传。
- **标准注入内容**：
  ```toml
  model = "<active-model>"
  model_provider = "<provider-id>"
  model_catalog_json = "<absolute-path-to-codexq-catalog>"

  [model_providers.<provider-id>]
  name = "..."
  base_url = "https://..."        # 该服务商若存在任意 chat 模型，此处为 http://127.0.0.1:<proxy-port>/v1
  wire_api = "responses"          # 恒为 responses，不接受任何其它取值
  experimental_bearer_token = "<plaintext-key>"
  ```
- **`base_url` 两种形态**：`gateway_enabled = 0`，或 `gateway_enabled = 1` 且服务商下**全部模型均为 responses** 时写上游直连地址；只要总开关开启且**存在任意一个 chat 模型**（服务商级 `wire_api` 为 chat，或 `model_wire_apis` 中任一条目为 chat）就写本机回环网关地址（§8.6）——因为一个 Codex provider 块只能有一个 `base_url`，而混合协议服务商需要网关在场才能服务其 chat 模型；其中的 responses 模型由网关原样透传。无论哪种形态，`wire_api` 都恒为 `"responses"`。
- **归一化兜底**：注入前对 `config.toml` 中**所有** `[model_providers.*]` 块做一次全量扫描，把任何非 `"responses"` 的 `wire_api` 改写为 `"responses"`，并校验 provider id 未占用保留名（`openai` / `ollama` / `lmstudio`）且 `name` 非空。Codex 会在启动时校验每一个 provider 块——包括已不被任何 profile 引用的僵尸块——因此只处理当前生效块是不够的。
- **高级覆写**：服务商可携带 `custom_config_toml`，此时以自定义片段**替代**标准 provider 表注入；`custom_auth_json` 则用于覆写活跃 `auth.json`。
- **模型目录路径**：`model_catalog_json` 是 Codex 支持的用户级配置项；CodexQ 写入生成目录的绝对路径，避免相对路径按当前工作目录解析失败。切回官方时只清理 CodexQ 自己的 `codexq-*` 目录引用，不覆盖用户目录。

### 8.4 模型目录与上下文窗口 / 自动压缩策略
- **产物路径**：`~/.codexq/providers/<id>/models.json`，并镜像一份到 `<codex_home>/model-catalogs/codexq-<id>.json`。
- **产物定位**：provider 激活时通过 `config.toml` 的 `model_catalog_json` 加载目录；同时通过 `model` / `model_provider` 完成路由。
- **上下文窗口解析**：按 `单模型覆盖 (model_context_windows)` → `服务商默认 (context_window)` → `DEFAULT_MIN_CONTEXT_WINDOW = 256_000` 依次回退，最终统一执行 `.max(256_000)` 硬性保底（低于 256K 一律向上 clamp）。
- **目录条目关键字段**：`context_window = max_context_window = 解析后的工作窗口`，`effective_context_window_percent = 95`。
- **自动压缩阈值**：`auto_compact_token_limit = 工作窗口 × 85 / 100`（整数除法）。实测取值：256K → 217,600；512K → 435,200；1M → 850,000。
- **取值依据**：Codex 官方内置目录默认在物理窗口约 90% 处压缩，CodexQ 把压缩点提前到**用户声明的工作窗口**的 85%，以减少 stale context 长期累积；`effective_context_window_percent = 95%` 保留为 Codex 侧的 headroom，在 85% 已生效时通常不参与决策，仅在单回合突发（如一次性读取大文件）时起保护垫作用。

### 8.5 交互入口
- **桌面端**：`ProvidersView` 选项卡（侧边栏 `nav.providers`）承载服务商列表与模糊检索；`ProviderCard` 提供多模型下拉即时切换、连通性 Ping、编辑与安全删除；`AddProviderModal` 提供通用表单、上游 `GET /models` 候选池检索导入、上下文长度预设（256K / 512K / 1M / 2M）与自定义单位解析、以及 `config.toml` / `auth.json` 高级文本框；`ActiveHeroCard` 展示服务商模式运行状态并提供「切回官方账号」快捷入口。
  - 编辑器在模型池上方提供「本地协议网关」开启/关闭总开关；关闭时不显示逐模型协议选择，开启时每个模型可选择 `Responses` 或 `Chat`。界面必须明确提示「Codex 侧恒为 responses，chat 协议将由本地协议网关自动转换」。
- **Python CLI（冻结）**：`codexq provider list | add | use | test | remove` 子命令，遵循纯标准库零依赖约束；自 v1.0.x 起进入冻结维护，不再跟进网关等新能力。
- **JSON-RPC**：`list_providers`、`switch_to_provider`、`get_active_runtime_mode` 等标准方法（供宿主与脚本长连接调用）。

### 8.6 本地协议网关 (Local Protocol Gateway)
当用户开启本地协议网关且服务商下至少有一个模型声明为 Chat Completions 时，CodexQ 在**进程内**启动一个仅监听回环地址的 HTTP 网关，把 Codex 发出的 Responses 请求翻译成上游能理解的 Chat Completions 请求，再把响应（含流式 SSE）翻译回 Responses 形态。总开关关闭或全模型均为 Responses 时，网关不启动，`base_url` 直连上游。对 Codex CLI 而言，上游永远是标准的 Responses API。

**模块布局**（`src-tauri/src/core/protocol_proxy/`）：

| 文件 | 职责 |
|------|------|
| `mod.rs` | 网关生命周期（启动 / 停止 / 状态查询）、回环地址与端口解析、路由注册 |
| `server.rs` | axum 路由、请求体读取、上游转发、错误映射 |
| `translate.rs` | Responses → Chat Completions 请求转换；Chat Completions → Responses 响应转换 |
| `stream.rs` | Chat Completions SSE → Responses SSE 事件流转换 |
| `route.rs` | 依据当前活跃服务商 + 请求中的模型解析上游 `base_url` / 协议 / 密钥 |

**监听与安全**
- 仅绑定 `127.0.0.1`，端口默认 `17871`；优先级：环境变量 `CODEXQ_PROXY_PORT` > `~/.codexq/config.json` 的 `proxy.port` > 默认值。
- 端口可在「设置 → 本地协议网关」中修改并即时探测可用性。环境变量存在时输入框禁用并显示「由环境变量决定」，而不是让改动静默失效——改了没反应是最难排查的一类故障。
- 网关是**进程级单例**：一个 CodexQ 进程只有一个监听端口，切换服务商只是复用同一监听。因此端口是**全局设置**（`proxy.port`），不是 per-provider 属性；服务商编辑器只读显示该端口与状态，不提供独立端口输入。

**端口可用性判定（三态）**

只区分「开着 / 没开」不足以定位问题，实际按三态判定（`probe_port`）：

| 状态 | 判定方式 | UI 文案 |
|------|----------|---------|
| `running` | 本进程 `RUNTIME` 记录的端口即该端口 | 运行中 |
| `free` | `TcpListener::bind((127.0.0.1, port))` 成功（随即释放） | 可用 |
| `occupied` | 绑定失败 | 被占用 |

占用者**不做身份识别**（不去猜是不是残留的旧 CodexQ 进程）：判定只需 bind 一次，引入 `GET /health` 反查会把同步的 `status()` 变成异步，收益只是文案更细。UI 用提示文案覆盖「可能是残留进程」这一情形即可。

**端口变更的唯一入口**

`set_gateway_port` 是**唯一**允许改端口的入口，按序执行，任一步失败即回滚到旧端口，绝不留下半配置状态：

1. 拒绝 `0`；环境变量覆盖生效时返回**显式错误**而非静默 no-op；
2. 探测目标端口；`occupied` 直接报错，**不尝试 `port+1` 之类的隐式漂移**——隐蔽的端口漂移会让 `config.toml` 里的地址与用户预期脱节；
3. 持久化 `proxy.port` 并重启监听；
4. **重写活跃服务商的 `base_url`**：`config.toml` 里的回环地址是上次切换服务商时写死的，只重启监听而不重写，会留下「配置是新端口 / 监听是新端口 / Codex 仍打旧端口」的三方不一致；
5. 任一步失败：恢复旧端口、重启监听、回写旧地址，返回错误。

这与「绑定失败绝不写不可达地址」是同一条原则的两面：网关不可用时宁可停在直连，也不写一个连不上的回环 URL（`AGENTS.md` §3 不变式 5）。自动选端口（绑不上就顺延）因此**不是默认行为**，只能作为显式开关的后续增量。
- 连接来源非回环地址一律拒绝（不是仅靠绑定实现，而是在处理函数中显式校验）。
- 不缓存、不落盘任何请求体与响应体；不记录会话内容到日志（仅记录方法、路径、状态码与耗时）。
- 网关是**进程内 Tokio 服务**，不依赖任何外部代理、`npx` 辅助进程或旁挂二进制。

**端点**

| 端点 | 行为 |
|------|------|
| `POST /v1/responses` | 主入口：按**请求中 `model` 解析出的协议**转换（chat）或原样透传（responses） |
| `GET /v1/models` | 透传上游模型列表（供候选池检索与连通性 Ping）；无模型可依，使用服务商默认协议 |
| `GET /health` | 就绪探针，返回网关版本与当前路由服务商 id |

**路由解析（按模型）**
`config.toml` 顶层只有一个 `model_provider` 槽位，因此网关**不需要靠 path 区分服务商**：它在每次请求时解析**当前活跃第三方服务商**（与 `get_active_runtime_mode` 同源），取得其 `base_url` / `wire_api` / `model_wire_apis` / 沙箱密钥。

协议本身**按模型解析**，而非按服务商：

| 解析顺序 | 来源 | 说明 |
|---------|------|------|
| 1 | `model_wire_apis[model]` | 精确匹配 trim 后的 slug |
| 2 | 同上，忽略大小写 | 兼容手工输入的 slug 大小写 |
| 3 | `provider.wire_api` | 服务商级默认值 |

取值统一经 `normalize_wire_api` 归一化：`chat` / `chat_completions` / `chat-completions` / `completions` / `completion` 全部折叠为 `chat`，其余一律回落 `responses`（写入未知值严格劣于静默使用标准协议）。

因此**同一个 `base_url` 可以同时服务两种协议**：`protocol == "chat"` 走 §8.6 的转换路径，`protocol == "responses"` 走**原样透传**路径。判定入口是 `resolve_active_route_for_model(model)`；`GET /v1/models` 这类无模型端点的请求使用 `resolve_active_route()`（等价于传空模型，即服务商默认值）。

解析失败时返回显式 `502` 错误体，**绝不静默回落到其它上游**——静默回落会把用户请求发往错误的服务商并产生计费。**同理，本网关不做「先试 Responses、失败再退回 Chat」的自动探测**：那属于同类的静默降级，且会把参数错误误判为协议不匹配、重复计费。协议归属必须由用户在 UI 中显式声明（见 `AGENTS.md` §4）。

**请求转换（Responses → Chat Completions）**

| Responses 输入 | Chat Completions 输出 |
|----------------|------------------------|
| 顶层 `instructions` | 首条 `role: "system"` 消息 |
| `input[].type == "message"`（`input_text` / `output_text`） | `role: user` / `role: assistant` 消息 |
| `input[].type == "function_call"` | `assistant.tool_calls[]` 条目 |
| `input[].type == "function_call_output"` | `role: "tool"` 消息（含 `tool_call_id`） |
| `input[].type == "reasoning"` | 从 `content[].reasoning_text` 提取文本；缺少时回退到 `summary[].summary_text`，并附加到后续 assistant 消息的 `reasoning_content`；不回放不透明的 `encrypted_content` |
| `tools[].function`（扁平 `name`/`description`/`parameters`） | `{ type: "function", function: { … } }` |
| `tool_choice` / `parallel_tool_calls` | 直译 |
| `max_output_tokens` | `max_tokens` |
| `reasoning.effort` | 上游思考参数；仅支持「思考开关」的上游**不注入等级**，避免请求被拒 |
| 多模态 content block（`input_image` 等） | Chat 的 `image_url` 形态 |

**响应转换（Chat Completions → Responses）**
- **非流式**：`choices[0].message.reasoning_content` → `output[]` 中的 `reasoning` item；`message.content` → `message` item；`tool_calls[]` → `function_call` item；`finish_reason` → `status`；`usage` 字段重映射（`prompt_tokens` → `input_tokens`，`completion_tokens` → `output_tokens`）。
- **流式**：Chat SSE 的 `delta.reasoning_content` 通过 `response.reasoning_summary_text.delta` 转发，并在结束时完成 reasoning item；可见文本与工具参数继续映射到对应 Responses 事件。**终止事件恰好发出一次**，终止后不再发出任何事件。
- **历史回放**：同一轮 assistant 可在 Responses `output[]` 中拆成 reasoning、文本和工具调用 item。转回 Chat 时，reasoning 附到 assistant 消息；若文本和 `function_call` 相邻，则合并为同一条含 `content`、`reasoning_content`、`tool_calls` 的 assistant 消息，相关工具结果仍紧随其后。
- **id 规约**：`chatcmpl-*` → `resp_*`；message item id 必须以 `msg_` 为前缀（直接拼接会产出上游拒收的 id 形态）。

**请求头透传（Header Forwarding）**
协议改写**不等于**改写 HTTP 信封。Codex 是「把网关当成服务商」在说话，它为该服务商发出的头必须照样抵达上游。踩过的真实故障：OpenCode Go 对缺少 `x-opencode-session` 的请求直接回 `400 MissingSessionID`——该头用于上游路由与 prompt 缓存，其文档明确要求代理「preserve the session header when forwarding requests」。

策略是**默认透传、黑名单剔除**（deny-list，而非 allow-list）：

| 处理 | 头 | 原因 |
|------|-----|------|
| **透传** | 除下列之外的全部入站头 | 服务商专有的 session / affinity / organization / beta 头必须原样存活；用白名单会随上游新增头而反复漏配 |
| **剔除** | 逐跳头：`host`、`content-length`、`connection`、`keep-alive`、`transfer-encoding`、`upgrade`、`te`、`trailer`、`proxy-*`、`accept-encoding` | 描述的是「Codex ↔ 网关」这一跳；`host`/`content-length` 转发后必然错误，`accept-encoding` 会让上游回压缩流，导致网关无法解析 SSE |
  | **剔除** | 网关自控头：`authorization`、`content-type`、`user-agent` | 入站 `authorization` 先与当前服务商密钥核对，防止旧会话误路由；随后用沙箱密钥重新设置上游认证头。请求体由网关重新序列化 |
| **重写** | `authorization` = `Bearer <沙箱密钥>`，`content-type` = `application/json` | 凭据在网关侧替换，最终用户密钥不出本机 |
| **补全** | `user-agent` | 上游要求客户端「Identify itself with its own user agent … rather than a generic SDK or HTTP-library name」。优先透传 Codex 的 UA，缺失或为 `reqwest` 时回落到 `codexq/<version>`，**绝不泄漏 HTTP 客户端库身份** |
| **合成** | `x-opencode-session`（仅限需要该头的上游） | 优先镜像客户端原生会话头（按名字形态识别：含 `session` 或 `conversation_id`），客户端未发才由 `prompt_cache_key` → `conversation.id` → `SHA256(model + instructions)` 兜底 |

兜底摘要**刻意排除 `input`**：该数组随对话轮次增长，参与哈希会导致每轮生成新 session id，从而击穿该头存在的意义（路由优化与 prompt 缓存复用）。

**上游状态码映射**
上游 `401` / `403` **必须改写成 `502`**：Codex 会把它们理解为**自己的**凭据被拒，进而升级为官方账号重新登录——而实际上被拒的是第三方服务商密钥，这会静默把用户切到另一个账号。其余 4xx **原样透传**（`429` 需要保住 Codex 的退避；`400` 需要保住可诊断信息），5xx 统一为 `502`。

**降级与回滚**
- 端口绑定失败（被占用 / 落入 Windows 动态端口排除区间）时**不接管**：保持 `base_url` 指向上游直连并在 UI 报错，绝不写入一个指向不可达网关的配置。
- 关闭本地路由总开关、停止网关、切回官方或清除所有 chat 模型时，`base_url` 自动回滚为上游直连地址；应用启动时也只在当前活跃服务商确实需要网关时才拉起监听进程。
- 服务商处于网关模式下被删除或密钥失效时，网关对该 provider 的请求返回显式错误，且下一次切号会自动清理路由。
