//! Path resolution utilities for CodexQ and Codex CLI.

use std::path::PathBuf;

/// Returns the base directory for CodexQ data (`~/.codexq` or `$CODEXQ_HOME`).
#[must_use]
pub fn codexq_home() -> PathBuf {
    if let Ok(val) = std::env::var("CODEXQ_HOME") {
        if !val.trim().is_empty() {
            return PathBuf::from(val.trim());
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
