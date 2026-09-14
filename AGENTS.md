# CodexQ - Agent Guidelines

Welcome to the CodexQ project. This document serves as the **primary entry point and action rule map** for AI agents and human contributors working within this repository. Adhere strictly to these guidelines to ensure system reliability, architectural integrity, and environment safety.

---

## 1. Project Overview & Tech Stack

CodexQ is a local, lightweight, high-performance quota manager and atomic account switcher tailored for OpenAI Codex CLI.

- **Desktop Host & Core Engine (`src-tauri/`)**: Tauri 2 (Rust) native desktop application with in-process core engine (`rusqlite` WAL persistence, Tokio asynchronous app-server rate limit probe, atomic account switcher, and 5-hour staggered warmup alarm scheduler). Zero external runtime dependencies.
- **Desktop UI (`src/`)**: React 19 + TypeScript + Vite + Tailwind CSS modern dashboard.
- **Python CLI / SDK Companion (`codexq.py`)**: Standalone single-file Python script providing CLI and Python Async SDK for headless / server environments without GUI.
  - **Core Constraint**: **Strictly standard library only (Python 3.10+). NEVER introduce external Python dependencies.**

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

## 3. Four Hard Invariants (Non-Negotiable)

When implementing changes, the following invariants **MUST** be maintained at all times:

1. **Zero External Dependencies in Python Core**:
   - MUST use standard library modules exclusively (`asyncio`, `sqlite3`, `json`, `subprocess`, `urllib.request`, `argparse`, etc.).
   - NEVER introduce third-party libraries such as `requests`, `fastapi`, `pydantic`, `sqlalchemy`, or `pyyaml`.
2. **Credential Security & Profile Sandboxing**:
   - **SQLite NEVER stores plaintext tokens.** Store only account metadata, profile ID, and auth SHA256 checksum.
   - Every profile directory MUST enforce `config.toml` declaring `cli_auth_credentials_store = "file"`.
   - Account switching (`switch`) MUST be **atomic and lossless**: archive the active token back to its source profile before writing destination credentials to prevent token loss.
3. **Rust Core Reliability & In-Process IPC**:
   - Tauri commands in `src-tauri/src/commands.rs` interface directly with the Rust core (`src-tauri/src/core/`), maintaining strict TypeScript type compatibility with `src/types.ts`.
   - The underlying rate limit probing communicates directly with official `codex app-server` via stdio JSON-RPC.
4. **Single-Binary Standalone Desktop Distribution**:
   - Desktop application builds into a self-contained binary (`CodexQ.exe`) with ZERO external Python or runtime dependencies.

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

### Run Python Companion Unit Tests (if modifying `codexq.py`)
```bash
python -m unittest discover tests -v
```

---

## 5. Change Management & Source of Truth

- **Single Source of Truth**: When updating architecture or core logic, update [docs/architecture.md](file:///d:/Code/codexq/docs/architecture.md) or [docs/PROJECT.md](file:///d:/Code/codexq/docs/PROJECT.md) in place.
- **Task Tracking**: Record active cross-session tasks in [docs/work/current.md](file:///d:/Code/codexq/docs/work/current.md); record completed decisions and post-mortems in [docs/work/history.md](file:///d:/Code/codexq/docs/work/history.md).
- **Git Version Control**: NEVER create duplicate version files (e.g., `_v2`, `_new`, `_backup`). All change history belongs in Git.

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

