# CodexQ

English | [简体中文](docs/README_zh.md)

A lightweight, local, high-performance OpenAI Codex multi-account quota manager, atomic account switcher, and staggered warmup scheduler.

CodexQ adopts a **dual-track engineering architecture**:
- 🖥️ **CodexQ Desktop**: Modern desktop application powered by **Tauri 2 (Pure Rust Core + React 19 + Tailwind CSS)**. Features in-process `rusqlite` WAL persistence, Tokio async probe pool, and warmup alarm scheduler. **Self-contained single-binary with zero external Python or runtime dependencies**.
- 🐍 **CodexQ CLI & Companion SDK** (`codexq.py`): Standalone single-file Python script (Python 3.10+, **strictly standard library only, zero third-party packages**) for headless servers, automation scripts, and terminal power users.

---

## Key Features

- **Remaining-First Quota Display**: 100% aligned with OpenAI's official dashboard, displaying remaining quota percentages (5H REMAIN / WEEK REMAIN) and available reset credits.
- **Pure Rust Native Desktop App**: Built on Tauri 2 as a standalone executable (`CodexQ.exe`, ~14 MB) with system tray residency and millisecond-level IPC responsiveness.
- **Easy Account Onboarding & Import**: One-click terminal login launch (`codex login`) and lossless `auth.json` credential file/path import via UI or CLI.
- **CodexQ Doctor Diagnostic Center**: Built-in environment diagnostic suite verifying CLI installation, config sandbox store mode, database integrity, and OpenAI network connectivity.
- **OAuth Token Auto-Renewal & Self-Healing**: Full implementation of OpenAI's Refresh Token Rotation (RTR) protocol. Proactively renews tokens before expiration and auto-heals on 401 errors.
- **Lossless Atomic Account Switcher**: Automatically archives active tokens before switching to prevent credential loss; seamlessly absorbs backups from `~/.codex/backups/`.
- **Staggered Warmup Scheduler**: Configures non-overlapping alarms per account ($\ge 5$ hour guard) to pre-warm rolling 5-hour quota windows via ephemeral pings.
- **JIT Dynamic Refresh & Desktop Notifications**: Auto-refreshes quotas precisely when reset windows expire and sends native OS notifications upon full quota restoration.
- **Profile Sandboxing & Security**: Enforces isolated profile sandboxes (`cli_auth_credentials_store = "file"`); SQLite never stores plaintext tokens.
- **Third-Party Model Providers**: Attach any OpenAI-compatible endpoint (DeepSeek, StepFun, SiliconFlow, OpenRouter, ...) with a managed model pool and per-model context windows (256K floor, auto-compaction at 85%). Plaintext keys live in a `0600` sandbox, and `~/.codex/config.toml` is edited losslessly (comments and MCP blocks preserved), so official accounts and third-party providers share one atomic runtime slot.
- **Zero-Dependency Python Companion**: Single-file Python script offering a full CLI, Python Async SDK (`from codexq import CodexQ`), local REST API, and stdio JSON-RPC.

---

## Directory Structure

All application data and isolated profiles reside in `~/.codexq/`:

```text
~/.codexq/
├── config.json               # Global configuration (plain-text JSON, human-editable)
├── codexq.db                 # SQLite WAL database (accounts, quota snapshots, alarms)
├── profiles/                 # Isolated account sandboxes
│   └── <profile-id>/         # Unique profile directory
│       ├── auth.json         # Sandboxed credentials (chmod 0600)
│       └── config.toml       # Enforces cli_auth_credentials_store = "file"
├── providers/                # Third-party provider key sandboxes & model catalogs
│   └── <provider-id>/
│       ├── key               # Plaintext API key (chmod 0600, never stored in SQLite)
│       └── models.json       # Generated Codex model catalog artifact
├── backups/                  # Pre-switch runtime snapshots (auth.json + config.toml, max 20)
│   └── <YYYYMMDD_HHMMSS>_<reason>/
└── trash/                    # Soft-delete recycle bin
    └── <profile-id>/
```

---

## Requirements

- **Codex CLI**: Official `codex` executable installed and accessible via `PATH`.
- **Desktop GUI Application**:
  - **Windows**: Windows 10 / 11 (run standalone `CodexQ.exe` or installer).
  - **Linux**: Ubuntu 20.04+, Debian 11+, Arch Linux, Fedora (`webkit2gtk-4.1`, `libayatana-appindicator3`).
  - **macOS**: macOS 11+ (Intel & Apple Silicon).
  - **Zero Python runtime dependencies**.
- **Python CLI & SDK Companion (`codexq.py`)**:
  - **Python 3.10+** (standard library only, cross-platform, no `pip install` required).

---

## Desktop GUI Application

### 1. Run Pre-Built Binary or Installer
Download or run the compiled standalone executable or package for your OS:
- **Windows**:
  - Standalone Binary: `CodexQ.exe`
  - NSIS Installer: `CodexQ_1.0.0_x64-setup.exe`
  - MSI Installer: `CodexQ_1.0.0_x64_en-US.msi`
- **Linux**:
  - Portable AppImage: `CodexQ_1.0.0_amd64.AppImage` (`chmod +x && ./CodexQ_1.0.0_amd64.AppImage`)
  - Debian Package: `codexq_1.0.0_amd64.deb` (`sudo dpkg -i codexq_1.0.0_amd64.deb`)
- **macOS**:
  - Disk Image: `CodexQ_1.0.0_x64.dmg` / `CodexQ_1.0.0_aarch64.dmg`

### 2. Build from Source
```bash
# 1. Install frontend dependencies
pnpm install

# 2. Run in development mode with hot-reload
pnpm tauri dev

# 3. Build optimized standalone executable and installer
pnpm tauri build
```

---

## CLI Reference (`codexq.py`)

### 1. List Accounts & Remaining Quota
```bash
python codexq.py list
```

Displays accounts with colorized remaining quota matching the official UI:
```text
ACCOUNT                   PLAN  5H REMAIN  5H RESET     WEEK REMAIN  WEEK RESET   RESETS  STATUS
------------------------  ----  ---------  -----------  -----------  -----------  ------  ------
  user1@163.com           free  -          -            -            -            0       active
* user2@gmail.com         plus  0%         09-09 14:26  53%          09-15 17:07  2       active
  team@company.com        team  0%         09-09 16:25  38%          09-15 10:18  3       active
```

Output machine-readable JSON:
```bash
python codexq.py list --json
```

### 2. Import Account Credentials
```bash
# Import an auth.json credential file directly
python codexq.py import path/to/auth.json
```

### 3. Switch Active Account
```bash
# Supports email, username, custom alias, or profile ID prefix
python codexq.py switch team@company.com
python codexq.py switch team
```

### 4. Restart Codex Desktop App
```bash
python codexq.py restart
```

### 5. Manage Account Aliases
```bash
# Set alias
python codexq.py alias user2@gmail.com main

# Clear alias for an account
python codexq.py alias user2@gmail.com

# Reset all aliases
python codexq.py alias --reset
```

### 6. Concurrent Quota Refresh
```bash
# Default concurrency: 5
python codexq.py refresh

# Custom concurrency
python codexq.py refresh --concurrency 10
```

### 7. Trigger Immediate Warmup Ping
```bash
# Force execution bypassing active quota window checks
python codexq.py warmup main --force
```

### 8. Manage Staggered Alarms
```bash
# List all scheduled alarms
python codexq.py alarm list

# Add weekday alarm at 08:00
python codexq.py alarm add main 08:00 --days 1,2,3,4,5

# Add one-time alarm (auto-disables after triggering)
python codexq.py alarm add main 09:30 --days once

# Enable / Disable / Delete an alarm
python codexq.py alarm enable <alarm_id>
python codexq.py alarm disable <alarm_id>
python codexq.py alarm delete <alarm_id>
```

### 9. Account Recycle Bin & Soft Deletion
```bash
python codexq.py remove old_account -y
python codexq.py trash
python codexq.py restore old_account
```

### 10. Quota History & Inspection
```bash
python codexq.py history main --limit 20
```

### 11. Start Local REST API or stdio JSON-RPC
```bash
python codexq.py serve --port 8765
python codexq.py rpc
```

### 12. Manage Third-Party Model Providers
```bash
# Add an OpenAI-compatible provider (optionally switch to it immediately with --switch)
python codexq.py provider add DeepSeek \
  --base-url https://api.deepseek.com/v1 \
  --key sk-xxxx \
  --model deepseek-chat \
  --models deepseek-chat,deepseek-reasoner \
  --context-window 256000

# List / activate / probe / remove
python codexq.py provider list
python codexq.py provider use deepseek -m deepseek-reasoner
python codexq.py provider test https://api.deepseek.com/v1 --key sk-xxxx
python codexq.py provider remove deepseek
```

---

## Python SDK Example

Import `codexq` as a standard library async Python module:

```python
import asyncio
from codexq import CodexQ

async def main():
    q = CodexQ()

    # 1. List accounts with automatic discovery
    accounts = q.list_accounts()
    for acc in accounts:
        print(acc["display_name"], acc["plan"], "Remaining:", acc["primary"]["remaining_percent"])

    # 2. Concurrently refresh quotas
    results = await q.refresh_all(concurrency=5)
    print("Refresh results:", results)

    # 3. Switch active account
    ok, msg = q.switch_account("main")
    print(msg)

    # 4. Trigger lightweight warmup ping
    warmup_res = await q.warmup("main", force=True)
    print("Warmup:", warmup_res)

if __name__ == "__main__":
    asyncio.run(main())
```

---

## Security & Sandboxing Standards

- **Zero Plaintext Tokens**: SQLite only stores account metadata, profile ID, and auth SHA256 checksums. Plaintext tokens are never stored in the database.
- **Strict File Permissions**: Profile directories enforce `0700` directory and `0600` file modes on Unix-like environments.
- **Isolated Profile Execution**: Each account profile enforces `cli_auth_credentials_store = "file"`, ensuring queries execute in sandbox isolation without polluting system-wide credential stores.
- **Repository Safety**: Never commit `~/.codexq/profiles/*/auth.json` or `~/.codex/auth.json` to public version control.
