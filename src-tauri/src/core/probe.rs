//! Rate limit probing via `codex app-server --stdio` child processes.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use rusqlite::params;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Semaphore;

use super::auth::{
    atomic_write, ensure_profile_config, is_access_token_expired, is_auth_newer_or_equal,
    read_auth_file, refresh_oauth_token_for_profile,
};
use super::db::{
    get_connection, get_current_identity_key, list_accounts_with_quota, resolve_account,
    sync_profile_auth_metadata,
};
use super::paths::active_auth_path;

/// Resolves the path to the `codex` executable.
#[must_use]
pub fn find_codex_bin() -> String {
    if let Ok(path) = std::env::var("CODEX_BIN") {
        if !path.trim().is_empty() {
            return path.trim().to_string();
        }
    }
    #[cfg(windows)]
    {
        if let Some(local_app_data) = dirs::data_local_dir() {
            let p1 = local_app_data.join("Programs").join("OpenAI").join("Codex").join("bin").join("codex.exe");
            if p1.is_file() {
                return p1.to_string_lossy().to_string();
            }
            let p2 = local_app_data.join("Microsoft").join("WinGet").join("Links").join("codex.exe");
            if p2.is_file() {
                return p2.to_string_lossy().to_string();
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = dirs::home_dir() {
            let candidates = [
                home.join(".local").join("bin").join("codex"),
                std::path::PathBuf::from("/usr/local/bin/codex"),
                home.join(".cargo").join("bin").join("codex"),
                std::path::PathBuf::from("/usr/bin/codex"),
            ];
            for c in candidates {
                if c.is_file() {
                    return c.to_string_lossy().to_string();
                }
            }
        }
    }
    "codex".to_string()
}

/// Communicates with `codex app-server --stdio` to read rate limit windows.
///
/// # Arguments
///
/// * `profile_dir` - Sandbox directory where `auth.json` and `config.toml` reside.
/// * `timeout_duration` - Maximum duration to await RPC responses.
///
/// # Errors
///
/// Returns `Err` if process spawning, IO, or RPC fails.
pub async fn query_rate_limits(
    profile_dir: &Path,
    timeout_duration: Duration,
) -> Result<serde_json::Value, String> {
    ensure_profile_config(profile_dir)?;

    let codex_bin = find_codex_bin();
    let mut cmd = Command::new(codex_bin);
    cmd.arg("app-server")
        .arg("--stdio")
        .env("CODEX_HOME", profile_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW = 0x08000000
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn codex app-server: {e}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to capture stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture stdout".to_string())?;

    let mut reader = BufReader::new(stdout).lines();

    let handshake = async {
        // 1. Send initialize
        let init_req = serde_json::json!({
            "method": "initialize",
            "id": 0,
            "params": {
                "clientInfo": {
                    "name": "codexq",
                    "title": "Codex Quota Manager",
                    "version": "0.2.0"
                }
            }
        });
        let mut msg = serde_json::to_vec(&init_req).map_err(|e| e.to_string())?;
        msg.push(b'\n');
        stdin.write_all(&msg).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        // Wait for id: 0
        while let Some(line) = reader.next_line().await.map_err(|e| e.to_string())? {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if v.get("id").and_then(|id| id.as_i64()) == Some(0) {
                    if let Some(err) = v.get("error") {
                        return Err(format!("app-server initialize error: {err}"));
                    }
                    break;
                }
            }
        }

        // 2. Send initialized notification
        let initialized_notif = serde_json::json!({
            "method": "initialized",
            "params": {}
        });
        let mut notif_msg = serde_json::to_vec(&initialized_notif).map_err(|e| e.to_string())?;
        notif_msg.push(b'\n');
        stdin.write_all(&notif_msg).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        // 3. Send rateLimits read request
        let read_req = serde_json::json!({
            "method": "account/rateLimits/read",
            "id": 1
        });
        let mut read_msg = serde_json::to_vec(&read_req).map_err(|e| e.to_string())?;
        read_msg.push(b'\n');
        stdin.write_all(&read_msg).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        // Wait for id: 1
        while let Some(line) = reader.next_line().await.map_err(|e| e.to_string())? {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                if v.get("id").and_then(|id| id.as_i64()) == Some(1) {
                    if let Some(err) = v.get("error") {
                        return Err(format!("app-server rateLimits error: {err}"));
                    }
                    if let Some(res) = v.get("result") {
                        return Ok(res.clone());
                    }
                    return Ok(v);
                }
            }
        }

        Err("app-server exited without returning rateLimits".to_string())
    };

    let handshake_res = tokio::time::timeout(timeout_duration, handshake).await;
    let _ = child.kill().await;

    match handshake_res {
        Ok(res) => res,
        Err(_) => Err(format!(
            "codex app-server timed out after {}s",
            timeout_duration.as_secs()
        )),
    }
}

fn extract_f64(v: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(val) = v.get(k) {
            if let Some(f) = val.as_f64() {
                return Some(f);
            }
            if let Some(i) = val.as_i64() {
                return Some(i as f64);
            }
        }
    }
    None
}

fn extract_i64(v: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    for k in keys {
        if let Some(val) = v.get(k) {
            if let Some(i) = val.as_i64() {
                return Some(i);
            }
        }
    }
    None
}

/// Helper to parse a JSON value into a rate limit bucket tuple if it describes quota windows.
///
/// Returns `Some((limit_id, p_used, p_win, p_resets, s_used, s_win, s_resets, raw_json_str))` if valid.
pub(crate) fn parse_bucket(
    val: &serde_json::Value,
    fallback_id: &str,
) -> Option<(String, Option<f64>, Option<i64>, Option<i64>, Option<f64>, Option<i64>, Option<i64>, String)> {
    if !val.is_object() {
        return None;
    }

    let primary = val.get("primary");
    let secondary = val.get("secondary");

    let has_primary = primary.map_or(false, |p| p.is_object());
    let has_secondary = secondary.map_or(false, |s| s.is_object());
    let has_flat = val.get("usedPercent").or_else(|| val.get("used_percent")).is_some();

    if !has_primary && !has_secondary && !has_flat {
        return None;
    }

    let limit_id = val
        .get("limitId")
        .or_else(|| val.get("limit_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_id)
        .to_string();

    let (p_used, p_win, p_resets, s_used, s_win, s_resets) = if has_primary || has_secondary {
        let pu = primary.and_then(|p| extract_f64(p, &["usedPercent", "used_percent"]));
        let pw = primary.and_then(|p| extract_i64(p, &["windowDurationMins", "window_duration_mins", "windowMinutes", "window_minutes"]));
        let pr = primary.and_then(|p| extract_i64(p, &["resetsAt", "resets_at"]));

        let su = secondary.and_then(|s| extract_f64(s, &["usedPercent", "used_percent"]));
        let sw = secondary.and_then(|s| extract_i64(s, &["windowDurationMins", "window_duration_mins", "windowMinutes", "window_minutes"]));
        let sr = secondary.and_then(|s| extract_i64(s, &["resetsAt", "resets_at"]));
        (pu, pw, pr, su, sw, sr)
    } else {
        let pu = extract_f64(val, &["usedPercent", "used_percent"]);
        let pw = extract_i64(val, &["windowDurationMins", "window_duration_mins", "windowMinutes", "window_minutes"]);
        let pr = extract_i64(val, &["resetsAt", "resets_at"]);
        (pu, pw, pr, None, None, None)
    };

    Some((limit_id, p_used, p_win, p_resets, s_used, s_win, s_resets, val.to_string()))
}

/// Parses the rate limits response and persists snapshots and latest quota in SQLite.
///
/// # Arguments
///
/// * `identity_key` - Unique account identity key.
/// * `res` - Parsed JSON object returned from app-server.
///
/// # Errors
///
/// Returns `Err` if SQLite insertion fails.
pub fn save_quota(identity_key: &str, res: &serde_json::Value) -> Result<(), String> {
    let conn = get_connection()?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let mut buckets: Vec<(String, Option<f64>, Option<i64>, Option<i64>, Option<f64>, Option<i64>, Option<i64>, String)> = Vec::new();

    // 1. If `res` itself contains quota windows (e.g. {"limitId": "codex", "primary": {...}, ...})
    if let Some(b) = parse_bucket(res, "codex") {
        buckets.push(b);
    }

    // 2. Look for `rateLimits` or `rate_limits` container
    if buckets.is_empty() {
        if let Some(rate_limits) = res.get("rateLimits").or_else(|| res.get("rate_limits")) {
            if let Some(b) = parse_bucket(rate_limits, "codex") {
                buckets.push(b);
            } else if let Some(arr) = rate_limits.as_array() {
                for item in arr {
                    if let Some(b) = parse_bucket(item, "codex") {
                        buckets.push(b);
                    }
                }
            } else if let Some(map) = rate_limits.as_object() {
                for (k, v) in map {
                    if k == "rateLimitResetCredits" || k == "rate_limit_reset_credits" {
                        continue;
                    }
                    if let Some(b) = parse_bucket(v, k) {
                        buckets.push(b);
                    }
                }
            }
        }
    }

    // 3. Look for `rateLimitsByLimitId` or `rate_limits_by_limit_id` container
    if buckets.is_empty() {
        if let Some(by_id) = res
            .get("rateLimitsByLimitId")
            .or_else(|| res.get("rate_limits_by_limit_id"))
            .and_then(|v| v.as_object())
        {
            for (k, v) in by_id {
                if let Some(b) = parse_bucket(v, k) {
                    buckets.push(b);
                }
            }
        }
    }

    // 4. Fallback if res is an array of limits
    if buckets.is_empty() {
        if let Some(arr) = res.as_array() {
            for item in arr {
                if let Some(b) = parse_bucket(item, "codex") {
                    buckets.push(b);
                }
            }
        }
    }

    for (limit_id, p_used, p_win, p_resets, s_used, s_win, s_resets, raw_str) in buckets {
        conn.execute(
            "INSERT INTO quota_latest (
                 identity_key, limit_id, fetched_at,
                 primary_used_percent, primary_window_minutes, primary_resets_at,
                 secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                 raw_json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(identity_key, limit_id) DO UPDATE SET
                 fetched_at = excluded.fetched_at,
                 primary_used_percent = excluded.primary_used_percent,
                 primary_window_minutes = excluded.primary_window_minutes,
                 primary_resets_at = excluded.primary_resets_at,
                 secondary_used_percent = excluded.secondary_used_percent,
                 secondary_window_minutes = excluded.secondary_window_minutes,
                 secondary_resets_at = excluded.secondary_resets_at,
                 raw_json = excluded.raw_json",
            params![
                identity_key,
                limit_id,
                now,
                p_used,
                p_win,
                p_resets,
                s_used,
                s_win,
                s_resets,
                raw_str,
            ],
        )
        .map_err(|e| format!("Failed to update quota_latest: {e}"))?;

        conn.execute(
            "INSERT INTO quota_snapshots (
                 identity_key, limit_id, observed_at,
                 primary_used_percent, primary_window_minutes, primary_resets_at,
                 secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                 raw_json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                identity_key,
                limit_id,
                now,
                p_used,
                p_win,
                p_resets,
                s_used,
                s_win,
                s_resets,
                raw_str,
            ],
        )
        .map_err(|e| format!("Failed to record quota_snapshot: {e}"))?;
    }

    let reset_count = res
        .get("rateLimitResetCredits")
        .or_else(|| res.get("rate_limit_reset_credits"))
        .and_then(|c| c.get("availableCount").or_else(|| c.get("available_count")))
        .and_then(|c| c.as_i64());

    let plan = res
        .get("planType")
        .or_else(|| res.get("plan_type"))
        .and_then(|p| p.as_str());

    conn.execute(
        "UPDATE accounts SET credential_status = 'active', last_error = NULL, reset_credits = COALESCE(?1, reset_credits), plan = COALESCE(?2, plan) WHERE identity_key = ?3",
        params![reset_count, plan, identity_key],
    )
    .map_err(|e| format!("Failed to update account status: {e}"))?;

    Ok(())
}

/// Refreshes quota for a single account.
pub async fn refresh_one(identity_key: &str, timeout_secs: u64) -> Result<bool, String> {
    let ident = match resolve_account(identity_key)? {
        Some(i) => i,
        None => return Err(format!("Account '{identity_key}' not found")),
    };

    let p_dir = ident.profile_dir();
    let auth_file = p_dir.join("auth.json");
    if !auth_file.exists() {
        let conn = get_connection()?;
        let _ = conn.execute(
            "UPDATE accounts SET credential_status = 'reauth_required', last_error = 'saved auth.json missing' WHERE identity_key = ?1",
            params![ident.key()],
        );
        return Ok(false);
    }

    // Proactive renewal: if access_token is expired or will expire within 5 minutes, renew via OAuth
    if let Ok((auth_val, _, _)) = read_auth_file(&auth_file) {
        if is_access_token_expired(&auth_val, 300) {
            log::info!(
                "Access token for {} is expired or expiring soon, attempting proactive OAuth renewal",
                ident.key()
            );
            if let Err(e) = refresh_oauth_token_for_profile(&p_dir).await {
                log::warn!("Proactive OAuth renewal for {} failed: {}", ident.key(), e);
            }
        }
    }

    let mut probe_result = query_rate_limits(&p_dir, Duration::from_secs(timeout_secs)).await;

    // Reactive self-healing: if app-server returns 401/unauthorized/expired, attempt OAuth refresh and retry once
    if let Err(ref err) = probe_result {
        let lower = err.to_lowercase();
        let is_auth_error = lower.contains("401")
            || lower.contains("token_expired")
            || lower.contains("token is expired")
            || lower.contains("unauthorized")
            || lower.contains("login required")
            || lower.contains("not logged in");

        if is_auth_error {
            log::info!(
                "Encountered auth error for {}, attempting OAuth token renewal fallback",
                ident.key()
            );
            match refresh_oauth_token_for_profile(&p_dir).await {
                Ok(true) => {
                    log::info!(
                        "OAuth token renewal succeeded for {}, retrying rate limits probe",
                        ident.key()
                    );
                    probe_result = query_rate_limits(&p_dir, Duration::from_secs(timeout_secs)).await;
                }
                Ok(false) => {}
                Err(refresh_err) => {
                    log::warn!(
                        "OAuth token renewal fallback for {} failed: {}",
                        ident.key(),
                        refresh_err
                    );
                }
            }
        }
    }

    match probe_result {
        Ok(res) => {
            save_quota(&ident.key(), &res)?;

            // 1. Sync updated profile credentials to DB if refreshed by codex app-server
            let _ = sync_profile_auth_metadata(&ident.key(), &p_dir);

            // 2. If this account is currently the active login in ~/.codex/auth.json,
            //    keep active_auth_path() synchronized with the newly refreshed credentials.
            if let Some(cur_key) = get_current_identity_key() {
                if cur_key == ident.key() {
                    let p_auth = p_dir.join("auth.json");
                    let host_auth = active_auth_path();
                    if let (Ok((p_val, _, p_raw)), Ok((h_val, _, _))) =
                        (read_auth_file(&p_auth), read_auth_file(&host_auth))
                    {
                        if is_auth_newer_or_equal(&p_val, &h_val) && p_val != h_val {
                            let _ = atomic_write(&host_auth, &p_raw);
                        }
                    }
                }
            }

            Ok(true)
        }
        Err(err) => {
            let conn = get_connection()?;
            let lower = err.to_lowercase();
            let is_auth_error = lower.contains("401")
                || lower.contains("token_expired")
                || lower.contains("token is expired")
                || lower.contains("unauthorized")
                || lower.contains("login required")
                || lower.contains("not logged in");

            let status = if is_auth_error {
                "reauth_required"
            } else {
                "error"
            };

            let clean_msg = if is_auth_error {
                "Authentication token expired (401 Unauthorized). Automatic renewal failed; please switch to this account and sign in again.".to_string()
            } else if err.len() > 180 {
                format!("{}...", &err[..180])
            } else {
                err.clone()
            };

            let _ = conn.execute(
                "UPDATE accounts SET credential_status = ?1, last_error = ?2 WHERE identity_key = ?3",
                params![status, clean_msg, ident.key()],
            );
            Err(err)
        }
    }
}

/// Concurrently refreshes all active accounts and returns the updated accounts list.
pub async fn refresh_all(concurrency: u32) -> Result<serde_json::Value, String> {
    let keys: Vec<String> = {
        let conn = get_connection()?;
        let mut stmt = conn
            .prepare("SELECT identity_key FROM accounts")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.flatten().collect()
    };

    let sem = Arc::new(Semaphore::new(concurrency.max(1) as usize));
    let mut tasks = Vec::new();

    for key in keys {
        let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        tasks.push(tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let _ = refresh_one(&key, 30).await;
        }));
    }

    for task in tasks {
        let _ = task.await;
    }

    let accounts = list_accounts_with_quota()?;
    serde_json::to_value(accounts).map_err(|e| e.to_string())
}
