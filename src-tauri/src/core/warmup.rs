//! Automated rate limit window warmup pinging via ephemeral `codex exec`.

use std::time::Duration;
use rusqlite::params;
use tokio::process::Command;

use super::auth::{
    ensure_profile_config, is_access_token_expired, read_auth_file, refresh_oauth_token_for_profile,
};
use super::config::load_config;
use super::db::{get_connection, get_current_identity_key, resolve_account};
use super::probe::find_codex_bin;

/// Triggers an ephemeral warmup ping using the account's sandbox to advance the 5-hour rate limit window.
///
/// # Arguments
///
/// * `target` - Target account identity key or alias (optional, defaults to active account).
/// * `model` - Custom model override (optional, defaults to `trigger.default_model`).
/// * `prompt` - Custom prompt override (optional, defaults to `trigger.prompt`).
/// * `force` - Whether to bypass active rate-limit window checks.
/// * `timeout_secs` - Timeout in seconds.
///
/// # Errors
///
/// Returns `Err` if the target account cannot be resolved.
pub async fn trigger_warmup(
    target: Option<String>,
    model: Option<String>,
    prompt: Option<String>,
    force: bool,
    timeout_secs: f64,
) -> Result<serde_json::Value, String> {
    let cfg = load_config();
    let resolved_model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or(cfg.trigger.default_model);
    let resolved_prompt = prompt
        .filter(|p| !p.trim().is_empty())
        .unwrap_or(cfg.trigger.prompt);

    // 1. Resolve identity
    let ident = if let Some(t) = target.filter(|s| !s.trim().is_empty()) {
        resolve_account(&t)?
            .ok_or_else(|| format!("Account '{t}' not found"))?
    } else if let Some(curr_key) = get_current_identity_key() {
        resolve_account(&curr_key)?
            .ok_or_else(|| "Current account not found".to_string())?
    } else {
        return Err("No active account found to trigger warmup".to_string());
    };

    let display_name = ident.email.clone().unwrap_or_else(|| ident.user_id.clone());

    // 2. Check if rate limit window is already active and skip if not forced
    if !force && cfg.trigger.skip_if_active {
        if let Ok(conn) = get_connection() {
            let row = conn
                .query_row(
                    "SELECT primary_used_percent, primary_resets_at FROM quota_latest WHERE identity_key = ?1",
                    params![ident.key()],
                    |r| Ok((r.get::<_, Option<f64>>(0)?, r.get::<_, Option<i64>>(1)?)),
                )
                .ok();

            if let Some((Some(used), Some(resets_at))) = row {
                let now_epoch = chrono::Utc::now().timestamp();
                if used > 0.0 && resets_at > now_epoch {
                    let reset_time_str = chrono::DateTime::from_timestamp(resets_at, 0)
                        .map(|dt| dt.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| resets_at.to_string());

                    let msg = format!(
                        "当前额度窗口已处于活跃状态（将于 {reset_time_str} 重置），已自动跳过以避免消耗额度。(Quota window already active)"
                    );

                    return Ok(serde_json::json!({
                        "status": "skipped",
                        "label": display_name,
                        "message": msg,
                        "resets_at": resets_at,
                        "model": resolved_model,
                    }));
                }
            }
        }
    }

    let p_dir = ident.profile_dir();
    ensure_profile_config(&p_dir)?;

    // Proactive renewal: if access_token is expired or will expire within 5 minutes, renew via OAuth
    let auth_file = p_dir.join("auth.json");
    if let Ok((auth_val, _, _)) = read_auth_file(&auth_file) {
        if is_access_token_expired(&auth_val, 300) {
            log::info!(
                "Access token for {} is expired or expiring soon, renewing before warmup",
                ident.key()
            );
            if let Err(e) = refresh_oauth_token_for_profile(&p_dir).await {
                log::warn!("OAuth renewal before warmup failed for {}: {}", ident.key(), e);
            }
        }
    }

    let codex_bin = find_codex_bin();
    let mut cmd = Command::new(codex_bin);
    cmd.arg("exec")
        .arg("--ephemeral")
        .arg("--skip-git-repo-check")
        .arg("-m")
        .arg(&resolved_model)
        .arg(&resolved_prompt)
        .env("CODEX_HOME", &p_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }

    let t_duration = Duration::from_secs_f64(timeout_secs.max(10.0));

    let run_exec = async {
        cmd.output()
            .await
            .map_err(|e| format!("Failed to run codex exec: {e}"))
    };

    match tokio::time::timeout(t_duration, run_exec).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

            if output.status.success() {
                Ok(serde_json::json!({
                    "status": "success",
                    "label": display_name,
                    "message": "Warmup triggered successfully",
                    "output": stdout,
                    "model": resolved_model,
                }))
            } else {
                let err_msg = if !stderr.is_empty() { stderr } else { stdout };
                Ok(serde_json::json!({
                    "status": "failed",
                    "label": display_name,
                    "error": err_msg,
                    "model": resolved_model,
                }))
            }
        }
        Ok(Err(e)) => Ok(serde_json::json!({
            "status": "failed",
            "label": display_name,
            "error": e,
            "model": resolved_model,
        })),
        Err(_) => Ok(serde_json::json!({
            "status": "failed",
            "label": display_name,
            "error": format!("Warmup execution timed out after {timeout_secs}s"),
            "model": resolved_model,
        })),
    }
}
