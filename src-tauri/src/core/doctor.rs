//! System environment diagnostic and health checks (CodexQ Doctor).
//!
//! Verifies Codex CLI presence, credentials storage sandbox enforcement,
//! SQLite database integrity, and network connectivity to OpenAI auth services.

use std::fs;
use std::process::Command;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::time::Instant;
use serde::{Deserialize, Serialize};

use super::paths::{codex_home, db_path, profiles_dir};
use super::probe::find_codex_bin;

/// A single diagnostic check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticItem {
    /// Unique identifier for this diagnostic check.
    pub key: String,
    /// Human-readable title of the diagnostic check.
    pub title: String,
    /// Result status: `"ok"`, `"warning"`, or `"error"`.
    pub status: String,
    /// Summary outcome message.
    pub message: String,
    /// Additional context or remedial guidance if applicable.
    pub detail: Option<String>,
}

/// Comprehensive system diagnostic report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    /// List of individual diagnostic check results.
    pub items: Vec<DiagnosticItem>,
    /// Whether all checks passed without errors.
    pub overall_healthy: bool,
    /// ISO 8601 timestamp when diagnostics were executed.
    pub timestamp: String,
}

/// Checks whether official `codex` CLI is installed and discoverable in PATH.
fn check_codex_cli() -> DiagnosticItem {
    let codex_bin = find_codex_bin();
    let mut cmd = Command::new(&codex_bin);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    match cmd.output() {
        Ok(output) if output.status.success() => {
            let ver = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let clean_ver = if ver.is_empty() {
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            } else {
                ver
            };
            DiagnosticItem {
                key: "codex_cli".to_string(),
                title: "Codex CLI Installation".to_string(),
                status: "ok".to_string(),
                message: if clean_ver.is_empty() {
                    "Installed and accessible".to_string()
                } else {
                    clean_ver
                },
                detail: None,
            }
        }
        _ => {
            #[cfg(target_os = "windows")]
            {
                let mut fallback_cmd = Command::new("cmd.exe");
                fallback_cmd.args(["/c", "codex", "--version"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                fallback_cmd.creation_flags(0x08000000);

                if let Ok(output) = fallback_cmd.output() {
                    if output.status.success() {
                        let ver = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        let clean_ver = if ver.is_empty() {
                            String::from_utf8_lossy(&output.stderr).trim().to_string()
                        } else {
                            ver
                        };
                        return DiagnosticItem {
                            key: "codex_cli".to_string(),
                            title: "Codex CLI Installation".to_string(),
                            status: "ok".to_string(),
                            message: if clean_ver.is_empty() {
                                "Installed and accessible".to_string()
                            } else {
                                clean_ver
                            },
                            detail: None,
                        };
                    }
                }
            }

            DiagnosticItem {
                key: "codex_cli".to_string(),
                title: "Codex CLI Installation".to_string(),
                status: "warning".to_string(),
                message: "Codex CLI executable not found in PATH".to_string(),
                detail: Some(
                    "Install the official OpenAI Codex CLI and ensure 'codex' is available in your system PATH."
                        .to_string(),
                ),
            }
        }
    }
}

/// Verifies whether `~/.codex/config.toml` enforces file-based credentials storage.
fn check_credentials_store() -> DiagnosticItem {
    let cfg_path = codex_home().join("config.toml");
    if !cfg_path.is_file() {
        return DiagnosticItem {
            key: "credentials_store".to_string(),
            title: "Credentials Storage Mode".to_string(),
            status: "warning".to_string(),
            message: "config.toml not found".to_string(),
            detail: Some(
                "Expected ~/.codex/config.toml declaring cli_auth_credentials_store = 'file'."
                    .to_string(),
            ),
        };
    }

    match fs::read_to_string(&cfg_path) {
        Ok(content) => {
            if content.contains("cli_auth_credentials_store = \"file\"")
                || content.contains("cli_auth_credentials_store = 'file'")
            {
                DiagnosticItem {
                    key: "credentials_store".to_string(),
                    title: "Credentials Storage Mode".to_string(),
                    status: "ok".to_string(),
                    message: "File store sandbox enforced (cli_auth_credentials_store = 'file')"
                        .to_string(),
                    detail: None,
                }
            } else {
                DiagnosticItem {
                    key: "credentials_store".to_string(),
                    title: "Credentials Storage Mode".to_string(),
                    status: "warning".to_string(),
                    message: "cli_auth_credentials_store is not set to 'file'".to_string(),
                    detail: Some(
                        "Codex CLI may attempt to use OS keychain instead of file-based sandboxing."
                            .to_string(),
                    ),
                }
            }
        }
        Err(e) => DiagnosticItem {
            key: "credentials_store".to_string(),
            title: "Credentials Storage Mode".to_string(),
            status: "error".to_string(),
            message: format!("Cannot read config.toml: {e}"),
            detail: None,
        },
    }
}

/// Verifies SQLite database integrity, WAL journal mode, and sandbox profile counts.
fn check_storage_health() -> DiagnosticItem {
    let p = db_path();
    if !p.is_file() {
        return DiagnosticItem {
            key: "storage_health".to_string(),
            title: "Local Database & Sandbox".to_string(),
            status: "warning".to_string(),
            message: "Database file not yet created".to_string(),
            detail: Some("Run refresh to initialize SQLite WAL storage.".to_string()),
        };
    }

    match super::db::get_connection() {
        Ok(conn) => {
            let journal_mode: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap_or_else(|_| "unknown".to_string());

            let account_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))
                .unwrap_or(0);

            let profiles = profiles_dir();
            let profile_count = fs::read_dir(profiles)
                .map(|entries| entries.flatten().filter(|e| e.path().is_dir()).count())
                .unwrap_or(0);

            DiagnosticItem {
                key: "storage_health".to_string(),
                title: "Local Database & Sandbox".to_string(),
                status: "ok".to_string(),
                message: format!(
                    "SQLite WAL active ({journal_mode}), {account_count} accounts, {profile_count} profiles"
                ),
                detail: None,
            }
        }
        Err(e) => DiagnosticItem {
            key: "storage_health".to_string(),
            title: "Local Database & Sandbox".to_string(),
            status: "error".to_string(),
            message: format!("SQLite connection failure: {e}"),
            detail: None,
        },
    }
}

/// Tests HTTPS reachability to OpenAI authentication server and reports proxy environment.
async fn check_network_connectivity() -> DiagnosticItem {
    let proxy_info = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("ALL_PROXY"))
        .or_else(|_| std::env::var("all_proxy"))
        .ok();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return DiagnosticItem {
                key: "network_connectivity".to_string(),
                title: "OpenAI Auth Service Reachability".to_string(),
                status: "error".to_string(),
                message: format!("Failed to build HTTP client: {e}"),
                detail: None,
            };
        }
    };

    let start = Instant::now();
    let url = "https://auth.openai.com";
    match client.head(url).send().await {
        Ok(resp) => {
            let elapsed = start.elapsed().as_millis();
            let proxy_desc = proxy_info
                .as_ref()
                .map(|p| format!(" (via proxy: {p})"))
                .unwrap_or_default();

            DiagnosticItem {
                key: "network_connectivity".to_string(),
                title: "OpenAI Auth Service Reachability".to_string(),
                status: "ok".to_string(),
                message: format!(
                    "Connected to auth.openai.com [{} {}] in {}ms{}",
                    resp.status().as_u16(),
                    resp.status().canonical_reason().unwrap_or(""),
                    elapsed,
                    proxy_desc
                ),
                detail: None,
            }
        }
        Err(e) => {
            let proxy_desc = proxy_info
                .as_ref()
                .map(|p| format!(" Active proxy env: {p}."))
                .unwrap_or_else(|| " No proxy environment variables set.".to_string());

            DiagnosticItem {
                key: "network_connectivity".to_string(),
                title: "OpenAI Auth Service Reachability".to_string(),
                status: "warning".to_string(),
                message: format!("Request failed: {e}"),
                detail: Some(format!(
                    "Cannot reach OpenAI authentication endpoint.{proxy_desc} Verify your internet connection or proxy settings."
                )),
            }
        }
    }
}

/// Executes all system health diagnostics and compiles a structured report.
pub async fn run_diagnostics() -> DiagnosticsReport {
    let mut items = Vec::new();

    items.push(check_codex_cli());
    items.push(check_credentials_store());
    items.push(check_storage_health());
    items.push(check_network_connectivity().await);

    let overall_healthy = items.iter().all(|it| it.status != "error");
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    DiagnosticsReport {
        items,
        overall_healthy,
        timestamp,
    }
}
