//! Path resolution utilities for CodexQ and Codex CLI.

use std::path::PathBuf;

/// Helper to determine if a given executable path represents portable mode.
#[must_use]
pub fn check_is_portable_for_path(exe_path: &std::path::Path) -> bool {
    let is_portable_filename = exe_path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase().contains("portable"))
        .unwrap_or(false);

    if is_portable_filename {
        return true;
    }

    if let Some(exe_dir) = exe_path.parent() {
        let portable_marker = exe_dir.join("portable");
        let data_dir = exe_dir.join("data");
        if portable_marker.exists() || data_dir.is_dir() {
            return true;
        }
    }

    false
}

/// Returns whether CodexQ is running in portable mode (local `portable` file, `data/` directory, or exe filename containing "portable").
#[must_use]
pub fn is_portable() -> bool {
    if let Ok(exe_path) = std::env::current_exe() {
        return check_is_portable_for_path(&exe_path);
    }
    false
}

/// Returns the base directory for CodexQ data (`~/.codexq`, `$CODEXQ_HOME`, or `./data` in portable mode).
#[must_use]
pub fn codexq_home() -> PathBuf {
    if let Ok(val) = std::env::var("CODEXQ_HOME") {
        if !val.trim().is_empty() {
            return PathBuf::from(val.trim());
        }
    }
    if is_portable() {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                return exe_dir.join("data");
            }
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".codexq"))
        .unwrap_or_else(|| PathBuf::from(".codexq"))
}

/// Returns the base directory for official Codex credentials (`~/.codex` or `$CODEX_HOME`).
#[must_use]
pub fn codex_home() -> PathBuf {
    if let Ok(val) = std::env::var("CODEX_HOME") {
        if !val.trim().is_empty() {
            return PathBuf::from(val.trim());
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

/// Returns the path to the active `auth.json` (`~/.codex/auth.json`).
#[must_use]
pub fn active_auth_path() -> PathBuf {
    codex_home().join("auth.json")
}

/// Returns the path to the global `config.json` (`~/.codexq/config.json`).
#[must_use]
pub fn config_path() -> PathBuf {
    codexq_home().join("config.json")
}

/// Returns the path to the SQLite database (`~/.codexq/codexq.db`).
#[must_use]
pub fn db_path() -> PathBuf {
    codexq_home().join("codexq.db")
}

/// Returns the path to the profiles directory (`~/.codexq/profiles`).
#[must_use]
pub fn profiles_dir() -> PathBuf {
    codexq_home().join("profiles")
}

/// Returns the path to the trash directory (`~/.codexq/trash`).
#[must_use]
pub fn trash_dir() -> PathBuf {
    codexq_home().join("trash")
}
