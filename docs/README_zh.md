# CodexQ

[English](../README.md) | 简体中文

本地、轻量、高性能的 OpenAI Codex 多账号配额管理、原子切换与定时错峰调度工具。

CodexQ 采用**双轨工程架构**：
- 🖥️ **CodexQ Desktop**：基于 **Tauri 2 (纯 Rust 原生核心 + React 19 + Tailwind CSS)** 构建的现代化桌面应用。内置 `rusqlite` WAL、Tokio 异步探测池与错峰闹钟调度器，**单二进制绿色免安装，零 Python 运行时依赖**。
- 🐍 **CodexQ CLI & Companion SDK** (`codexq.py`)：独立的单文件 Python 脚本（Python 3.10+，**纯标准库零外部依赖**），服务于 Headless 服务器、自动化脚本、CI/CD 与终端极客。

---

## 核心特性

- **官方同款剩余配额 (Remaining-First)**：与 OpenAI 官方仪表盘对齐，直观展示 5 小时与周度剩余配额百分比及可用重置机会。
- **纯 Rust 原生桌面端**：基于 Tauri 2 构建，单二进制绿色免安装（`CodexQ.exe`，约 14 MB），常驻系统托盘，毫秒级响应。
- **便捷账号引入与凭证导入**：支持一键终端拉起登录 (`codex login`)，提供 UI 与 CLI 双重无损 `auth.json` 凭据文件/路径导入。
- **CodexQ Doctor 环境诊断中心**：内置环境健康体检，一键验证 CLI 安装、沙箱配置模式、数据库状态及网络连通性。
- **OAuth Token 自动续期与自愈**：实现官方 Refresh Token Rotation (RTR) 协议，预测到期提前换票，遇 401 自动续期重试，彻底解决 Token 频繁失效。
- **原子无损切号与历史备份吸纳**：切号前自动归档当前 Token 防丢失；自动无损吸纳 `~/.codex/backups/` 最新备份，防止凭证断代。
- **定时触发与多闹钟错峰流水线**：单账号独立配置错峰预热闹钟（强校验 $\ge 5$ 小时防重叠），提前激活 5 小时额度窗口，支持「仅一次」模式。
- **JIT 到期自感知刷新与通知**：配额到期精准自动唤醒刷新，满血复活时触发系统原生桌面通知。
- **安全沙箱隔离**：各账号独立沙箱存储（强制 `cli_auth_credentials_store = "file"`），SQLite 严禁明文存储 Token。
- **纯标准库 Python CLI / SDK**：`codexq.py` 单文件免安装即用，提供完整 CLI、Python Async SDK、REST API 与 stdio JSON-RPC。

---

## 数据目录规范

所有数据与隔离凭据默认保存在 `~/.codexq/`：

```text
~/.codexq/
├── config.json               # 全局用户配置（纯文本 JSON，2 空格缩进，手写友好）
├── codexq.db                 # SQLite 数据库（启用 WAL 模式，存储账号、额度快照与闹钟）
├── profiles/                 # 各账号物理隔离沙箱
│   └── <profile-id>/         # 账号 profile_id（基于 identity_key 计算）
│       ├── auth.json         # 账号独立凭据镜像（chmod 0600）
│       └── config.toml       # 强制 cli_auth_credentials_store = "file"
└── trash/                    # 软删除回收站隔离目录
    └── <profile-id>/
```

---

## 环境要求

- **Codex CLI**：已安装并能运行官方 `codex` 命令行工具。
- **桌面 GUI 客户端**：
  - **Windows**：Windows 10 / 11（直接运行免安装绿色二进制 `CodexQ.exe` 或安装包）。
  - **Linux**：Ubuntu 20.04+、Debian 11+、Arch Linux、Fedora（依赖 `webkit2gtk-4.1` 与 `libayatana-appindicator3`，原生支持 AppImage 与 deb）。
  - **macOS**：macOS 11+（Intel / Apple Silicon 双架构原生支持）。
  - **零外部 Python 环境依赖**。
- **Python CLI / SDK 伴侣 (`codexq.py`)**：
  - **Python 3.10+**（纯标准库，跨平台即拷即用，无需 `pip install` 任何第三方包）。

---

## 桌面 GUI 客户端使用与开发

### 1. 直接运行绿色版或安装包
根据您的操作系统下载对应的安装包或便携程序：
- **Windows**：
  - 免安装单文件：`CodexQ.exe`
  - NSIS 安装程序：`CodexQ_1.0.0_x64-setup.exe`
  - MSI 安装程序：`CodexQ_1.0.0_x64_en-US.msi`
- **Linux**：
  - 通用 AppImage 免安装包：`CodexQ_1.0.0_amd64.AppImage` (`chmod +x && ./CodexQ_1.0.0_amd64.AppImage`)
  - Debian / Ubuntu 安装包：`codexq_1.0.0_amd64.deb` (`sudo dpkg -i codexq_1.0.0_amd64.deb`)
- **macOS**：
  - 磁盘镜像安装包：`CodexQ_1.0.0_x64.dmg` / `CodexQ_1.0.0_aarch64.dmg`

### 2. 本地二次开发与打包构建
工程为现代化纯 Tauri 2 架构，已扁平化至根目录，直接使用 pnpm 与 Rust 编译：

```bash
# 1. 安装前端依赖
pnpm install

# 2. 启动桌面端热重载开发环境
pnpm tauri dev

# 3. 编译发布单二进制绿色程序与安装包
pnpm tauri build
```

---

## CLI 常用命令手册 (`codexq.py`)

### 1. 查看所有账号与剩余额度（自动感知新登录）
```bash
python codexq.py list
```

输出带彩色状态高亮，与官方 UI 剩余额度完全对齐：
```text
ACCOUNT                   PLAN  5H REMAIN  5H RESET     WEEK REMAIN  WEEK RESET   RESETS  STATUS
------------------------  ----  ---------  -----------  -----------  -----------  ------  ------
  user1@163.com           free  -          -            -            -            0       active
* user2@gmail.com         plus  0%         09-09 14:26  53%          09-15 17:07  2       active
  team@company.com        team  0%         09-09 16:25  38%          09-15 10:18  3       active
```

导出 JSON 数据：
```bash
python codexq.py list --json
```

### 2. 导入外部凭据文件
```bash
# 导入外部已登录的 auth.json 文件
python codexq.py import path/to/auth.json
```

### 3. 一键切换活动账号
```bash
python codexq.py switch team@company.com
python codexq.py switch team
```

### 4. 重启外部 Codex 客户端
```bash
python codexq.py restart
```

### 5. 账号别名管理
```bash
# 设置别名
python codexq.py alias user2@gmail.com main

# 清空单个账号别名
python codexq.py alias user2@gmail.com

# 一键重置所有别名
python codexq.py alias --reset
```

### 6. 并发刷新所有账号额度
```bash
python codexq.py refresh
python codexq.py refresh --concurrency 10
```

### 7. 手动立即触发预热问候
```bash
python codexq.py warmup main --force
```

### 8. 定时错峰预热闹钟管理
```bash
# 查看所有闹钟排期
python codexq.py alarm list

# 为指定账号添加工作日 08:00 错峰预热闹钟
python codexq.py alarm add main 08:00 --days 1,2,3,4,5

# 添加一次性闹钟（触发后自动停用）
python codexq.py alarm add main 09:30 --days once

# 启用 / 停用 / 删除指定闹钟
python codexq.py alarm enable <alarm_id>
python codexq.py alarm disable <alarm_id>
python codexq.py alarm delete <alarm_id>
```

### 9. 账号软删除与回收站
```bash
python codexq.py remove old_account -y
python codexq.py trash
python codexq.py restore old_account
```

### 10. 额度历史回溯
```bash
python codexq.py history main --limit 20
```

### 11. 启动本地 REST API 或 stdio JSON-RPC
```bash
python codexq.py serve --port 8765
python codexq.py rpc
```

---

## Python SDK 调用示例

可以直接将 `codexq` 作为标准库 Python SDK 导入：

```python
import asyncio
from codexq import CodexQ

async def main():
    q = CodexQ()

    # 1. 获取所有账号列表（默认 auto_sync=True，自动感知当前登录）
    accounts = q.list_accounts()
    for acc in accounts:
        print(acc["display_name"], acc["plan"], "剩余:", acc["primary"]["remaining_percent"])

    # 2. 并发刷新所有账号配额
    results = await q.refresh_all(concurrency=5)
    print("刷新结果:", results)

    # 3. 切换当前活动账号
    ok, msg = q.switch_account("main")
    print(msg)

    # 4. 触发轻量问候垫刀
    warmup_res = await q.warmup("main", force=True)
    print("预热结果:", warmup_res)

if __name__ == "__main__":
    asyncio.run(main())
```

---

## 安全与沙箱规范

- **Token 零泄露**：SQLite 数据库只存储账号元数据、Profile ID 与凭据 SHA256 哈希，不存储任何明文 Token。
- **文件系统权限保障**：Linux / macOS 下自动将 `~/.codexq` 与各 Profile 目录权限设为 `0700`，凭据文件设为 `0600`。
- **凭据物理隔离**：每个 Profile 独立 `config.toml` 强制 `cli_auth_credentials_store = "file"`，查询配额时仅限独立沙箱环境，不污染系统全局凭据库。
- 切勿将 `~/.codexq/profiles/*/auth.json` 或 `~/.codex/auth.json` 提交至公共 Git 仓库。
