# CodexQ - Agent Guidelines

Welcome to the CodexQ project. This document serves as the **primary entry point and action rule map** for AI agents and human contributors working within this repository. Adhere strictly to these guidelines to ensure system reliability, architectural integrity, and environment safety.

---

## 1. Project Overview & Tech Stack

CodexQ is a local, lightweight, high-performance quota manager, atomic account switcher and protocol adapter tailored for OpenAI Codex CLI.

- **Desktop Host & Core Engine (`src-tauri/`)**: Tauri 2 (Rust) native desktop application with in-process core engine (`rusqlite` WAL persistence, Tokio asynchronous app-server rate limit probe, atomic account switcher, 5-hour staggered warmup alarm scheduler, and a loopback-only protocol gateway that adapts Chat Completions upstreams to the Responses API). Zero external runtime dependencies.
- **Desktop UI (`src/`)**: React 19 + TypeScript + Vite + Tailwind CSS modern dashboard.
- **Python CLI / SDK Companion (`codexq.py`)**: Standalone single-file Python script providing CLI and Python Async SDK for headless / server environments without GUI.
  - **Core Constraint**: **Strictly standard library only (Python 3.10+). NEVER introduce external Python dependencies.**
  - **FROZEN**: see §1.1 — this surface is in freeze maintenance and accepts no new features.

### 1.1 Project Stage & Maintenance Focus

**Read this before proposing any change — it decides where work is even allowed to land.**

- **Stage: post-early-version.** CodexQ has shipped v1.0.x with cross-platform CI/CD, a stable SQLite schema, a stable `~/.codexq/config.json` contract and live users. There is no more "rewrite it if it's ugly" latitude. Changes MUST be **backward compatible** with existing `~/.codexq/` state and MUST NOT silently migrate, drop or reinterpret a user's existing `~/.codex/config.toml`, profiles, snapshots or provider records. A schema or config-shape change requires an explicit, idempotent migration path plus a matching note in `docs/work/history.md`.
- **Focus: the desktop surface only.** `src-tauri/` (Rust core) and `src/` (React 19 UI) are the **only active development vehicles**. Product capability, protocol work, new UI, new storage, new scheduling — everything lands here.
- **Python (`codexq.py`) is frozen.** It accepts **blocking-severity bug fixes and security fixes only**. Explicitly forbidden:
  - adding new subcommands, new JSON-RPC methods, or new provider/routing capability;
  - porting a desktop feature "for parity" — parity is no longer a goal;
  - refactoring for style, or expanding the 46-case unittest suite beyond what a blocking fix requires.
  - Rationale: the pure-stdlib, single-file constraint cannot carry protocol adaptation, streaming transforms or an embedded HTTP gateway. Any feature that would need those MUST be implemented on the desktop side instead. When a desktop change makes the Python surface semantically stale, record the divergence in `docs/architecture.md` rather than chasing it.
- **Consequence for review:** if a task can be satisfied on the desktop side, it MUST NOT be implemented in Python; if a task is Python-only, ask whether it is truly blocking before writing code.

---

## 2. Progressive Disclosure: Documentation Map

Do not ingest all documentation or code at once. Explore downward on demand according to your immediate task:

```text
AGENTS.md (Root map & hard invariants)
  │
  ├── Architecture, data flows & RPC ──> docs/architecture.md
  ├── Product scope & non-goals ─────────> docs/PROJECT.md
  ├── Active tasks & current state ──────> docs/work/current.md
  ├── Architectural decisions & lessons ─> docs/work/history.md
  └── Documentation index & authority ───> docs/index.md
```

---

## 3. Hard Invariants (Non-Negotiable)

When implementing changes, the following invariants **MUST** be maintained at all times:

1. **Zero External Dependencies in Python Core** (frozen surface — see §1.1):
   - MUST use standard library modules exclusively (`asyncio`, `sqlite3`, `json`, `subprocess`, `urllib.request`, `argparse`, etc.).
   - NEVER introduce third-party libraries such as `requests`, `fastapi`, `pydantic`, `sqlalchemy`, or `pyyaml`.
   - This invariant is a property of the frozen surface: it forbids adding dependencies, and it does NOT authorize adding features.
2. **Credential Security & Profile Sandboxing**:
   - **SQLite NEVER stores plaintext tokens.** Store only account metadata, profile ID, and auth SHA256 checksum.
   - Every profile directory MUST enforce `config.toml` declaring `cli_auth_credentials_store = "file"`.
   - Account switching (`switch`) MUST be **atomic and lossless**: archive the active token back to its source profile before writing destination credentials to prevent token loss.
   - Provider API keys live only in `~/.codexq/providers/<id>/key` (`0600`); SQLite stores a mask plus SHA256 only.
3. **Rust Core Reliability & In-Process IPC**:
   - Tauri commands in `src-tauri/src/commands.rs` interface directly with the Rust core (`src-tauri/src/core/`), maintaining strict TypeScript type compatibility with `src/types.ts`.
   - The underlying rate limit probing communicates directly with official `codex app-server` via stdio JSON-RPC.
4. **Single-Binary Standalone Desktop Distribution**:
   - Desktop application builds into a self-contained binary (`CodexQ.exe`) with ZERO external Python or runtime dependencies.
   - The protocol gateway is an **in-process** Tokio service (`axum` + `futures-util` on top of the existing `tokio`/`reqwest` stack). NEVER shell out to an external proxy, `npx` helper or sidecar binary to do protocol work, and never add a crate that needs a runtime system dependency.
5. **Codex `config.toml` Load Safety** (learned from openai/codex discussion #7782 and the 0.148/0.149 load validators):
   - `wire_api` written into `~/.codex/config.toml` **MUST be exactly `"responses"`, unconditionally.** Codex removed the chat wire entirely in Feb 2026; any other value fails deserialization of the **whole** config, which bricks every command (`codex exec`, `codex login status`, …) while `codex doctor` still runs and reports `config.load: fail`.
   - Upstream protocol differences MUST be absorbed by the local gateway (`src-tauri/src/core/protocol_proxy/`). NEVER leak an upstream wire protocol into Codex's config, and NEVER expose a chat/completions option that reaches `config.toml`.
   - Codex validates **every** `[model_providers.*]` block at startup — including blocks no longer referenced by any profile or `--model`. Normalization therefore MUST be a **full sweep** over all provider tables, never a targeted fix of the active one only.
   - Reserved provider ids **`openai` / `ollama` / `lmstudio` must never be used as third-party provider ids** (Codex ≥0.148 rejects the entire config when one is overridden; the match is case-sensitive and bedrock ids are exempt). A provider `name` MUST never be empty (Codex rejects `provider name must not be empty`).

---

## 4. Mechanical Verification & Feedback Loop

Always verify changes using isolated test sandboxes. **NEVER run tests against the real host `~/.codex` or `~/.codexq` directories.**

### Run Rust Core Unit Tests
```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

### Validate Frontend Types & Production Build
```bash
pnpm run build
```

### Check Rust Compilation & Lints
```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

### Run Python Companion Unit Tests (only when a blocking fix touches `codexq.py`)
```bash
python -m unittest discover tests -v
```

> Python is a frozen surface (§1.1). Its suite MUST stay green, but a green suite is not an invitation to extend the Python side.

### Protocol Gateway Checks (`src-tauri/src/core/protocol_proxy/`)
Protocol translation is the highest-risk code in the repo — a wrong field mapping silently breaks a user's coding session. Any change here MUST be covered by round-trip tests:
- Request translation: Responses `input`/`instructions`/`tools` → Chat Completions `messages`/`tools`, including `function_call` / `function_call_output` pairs.
- Response translation: Chat Completions → Responses `output` array, including non-stream and streamed SSE paths.
- Streaming lifecycle: every Responses terminal event (`response.completed` / `response.failed`) MUST be emitted exactly once, and no event may be emitted after termination.
- Route resolution failures MUST surface as an explicit error response, never as a silent fallback to a different upstream.
- Header envelope: translating the wire protocol MUST NOT strip the HTTP envelope. Inbound headers are forwarded to the upstream by default (deny-list, not allow-list) so provider session/affinity headers survive — omitting this hard-fails upstreams that require them (e.g. OpenCode Go rejects a missing `x-opencode-session` with `MissingSessionID`). Only hop-by-hop headers and gateway-controlled ones (`authorization`, `content-type`, `user-agent`, `host`, `content-length`, `accept-encoding`) may be dropped.
- Upstream identity: never let the HTTP client's own `User-Agent` reach the upstream; forward Codex's, or fall back to `codexq/<version>`.
- Status mapping: upstream `401`/`403` MUST be rewritten to `502` so Codex does not mistake a provider-side rejection for its own expired credentials and trigger an official re-login. `429` and other 4xx MUST pass through so backoff and diagnostics still work.
- Header behaviour is part of the tested surface; add a unit test alongside any change to header handling.
- Protocol granularity: the upstream protocol is a **per-model** property. `provider.wire_api` is only the provider-level default; `model_wire_apis` overrides it per slug. Resolution MUST happen per request from the body's `model` field (never frozen into `config.toml`), because Codex lets the user change models mid-session while CodexQ only rewrites the config on an explicit slot switch.
- No auto-detection: NEVER implement "try Responses, fall back to Chat Completions on failure". That is the same class of silent downgrade as falling back to another upstream, and it would misread a parameter error as a protocol mismatch and double-bill the user. Protocol ownership must be declared explicitly by the user.

---

## 5. Change Management & Source of Truth

- **Single Source of Truth**: When updating architecture or core logic, update [docs/architecture.md](file:///d:/Code/codexq/docs/architecture.md) or [docs/PROJECT.md](file:///d:/Code/codexq/docs/PROJECT.md) in place.
- **Task Tracking**: Record active cross-session tasks in [docs/work/current.md](file:///d:/Code/codexq/docs/work/current.md); record completed decisions and post-mortems in [docs/work/history.md](file:///d:/Code/codexq/docs/work/history.md).
- **Git Version Control & Staging Hygiene**:
  - **NEVER create duplicate version files** (e.g., `_v2`, `_new`, `_backup`). All change history belongs in Git.
  - **No Blind Staging**: Strictly prohibit `git add .` or `git add -A`. Explicitly stage only intended target files.
  - **Pre-Commit Audit**: ALWAYS inspect changes with `git status` and `git diff --staged` before committing.
  - **Sensitive Data & Junk Filter**: NEVER commit live tokens/auth files (`auth.json`, session keys), local sandbox databases (`*.db`, `*.db-wal`), test caches, `.env` files, or temporary/scratch files (`*.tmp`, debug dumps, OS metadata).

---

## 6. Code Documentation & Comment Standards

To ensure long-term readability and autonomous agent collaboration across this multi-language codebase, all new and refactored code MUST strictly follow these standards:

### 6.1 Python: Google Style Docstrings
- Modules, classes, and public functions MUST include docstrings.
- The first line MUST be a concise one-line summary ending with a period, followed by an optional blank line and detailed description.
- MUST contain structured sections: `Args:`, `Returns:`, and `Raises:` (if exceptions are raised).
- Example:
  ```python
  async def trigger_warmup(
      self,
      identity_key: str,
      force: bool = False,
  ) -> dict[str, Any]:
      """Trigger a lightweight greeting request to activate the quota window.

      Args:
          identity_key: Unique account identifier (user_id\x1faccount_id).
          force: Whether to force execution, bypassing active window check.

      Returns:
          A dictionary containing success status, message, and output.

      Raises:
          ValueError: If identity_key is not found or profile is corrupted.
      """
  ```

### 6.2 TypeScript / React: TSDoc Standards
- Public components, custom hooks, exported utility functions, and core interfaces MUST have `/** ... */` doc blocks.
- Leverage TypeScript's type system directly. **NEVER** duplicate type annotations like `{string}` or `{boolean}` inside comment tags.
- Common tags: `@param` (parameter description), `@returns` (return description), `@remarks` (advanced notes/gotchas), `@example` (usage example).
- Example:
  ```typescript
  /**
   * View component managing scheduled alarms and trigger settings.
   *
   * @param props - Component props object.
   * @param props.activeAccountKey - Key of the currently active/selected account.
   * @param props.onAccountSwitched - Callback triggered when the account is switched.
   */
  export const SchedulerView: React.FC<SchedulerViewProps> = ({ ... }) => { ... };
  ```

### 6.3 Rust: Rustdoc Standards (RFC 1574)
- Module-level documentation belongs at the top of the file using `//!`; structs, enums, methods, and Tauri commands use `///`.
- Document content MUST use standard Markdown.
- Functions returning `Result<T, E>` **MUST** include an `# Errors` section detailing all possible error conditions. Document `# Panics` if any unhandled panics exist.
- Example:
  ```rust
  /// Sends a JSON-RPC request to the persistent Python daemon and awaits response.
  ///
  /// Applies tiered timeouts based on method complexity.
  ///
  /// # Arguments
  ///
  /// * `method` - JSON-RPC 2.0 method name (e.g. `"trigger_warmup"`).
  /// * `params` - Request parameters payload.
  ///
  /// # Returns
  ///
  /// Deserialized `serde_json::Value` on success.
  ///
  /// # Errors
  ///
  /// Returns `Err` if:
  /// - The child daemon process is not running or crashed.
  /// - The request times out according to the method's timeout threshold.
  /// - The Python server returns a JSON-RPC error response.
  pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> { ... }
  ```

