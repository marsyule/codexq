//! Atomic and lossless account switching implementation.

use rusqlite::params;

use super::auth::{
    atomic_write, ensure_host_codex_config, is_access_token_expired, read_auth_file,
    refresh_oauth_token_for_profile,
};
use super::db::{auto_sync_current, get_connection, resolve_account};
use super::paths::active_auth_path;
use super::process::restart_codex;

/// Atomically switches the active Codex CLI credentials to the target account.
///
/// Ensures the currently active token is first saved back to its profile before replacing.
///
/// # Arguments
///
/// * `target` - Target account identity key, email, or alias.
/// * `restart` - Whether to restart the running Codex app.
///
/// # Errors
///
/// Returns `Err` if target account does not exist or atomic write fails.
pub async fn switch_account(target: &str, restart: bool) -> Result<String, String> {
    // 0. Ensure host ~/.codex/config.toml enforces cli_auth_credentials_store = "file"
    let _ = ensure_host_codex_config();

    // 1. Sync currently active token back to its profile first to prevent loss of refreshed tokens
    auto_sync_current();

    // 2. Resolve destination account
    let ident = resolve_account(target)?
        .ok_or_else(|| format!("Account '{target}' not found."))?;

    let p_auth = ident.profile_dir().join("auth.json");
    if !p_auth.is_file() {
        return Err(format!("Saved credentials for '{target}' do not exist."));
    }

    // Proactively renew destination account's OAuth token if expired or expiring soon
    if let Ok((auth_val, _, _)) = read_auth_file(&p_auth) {
        if is_access_token_expired(&auth_val, 300) {
            log::info!(
                "Destination account token for {} is expired or expiring soon, renewing before switch",
                ident.key()
            );
            if let Err(e) = refresh_oauth_token_for_profile(&ident.profile_dir()).await {
                log::warn!(
                    "OAuth renewal before switch failed for {}: {}",
                    ident.key(),
                    e
                );
            }
        }
    }

    let (_, _, raw_bytes) = read_auth_file(&p_auth)?;
    let dest = active_auth_path();

    // 3. Atomically overwrite active auth.json
    atomic_write(&dest, &raw_bytes)?;

    // 4. Update last_seen_at in DB
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    if let Ok(conn) = get_connection() {
        let _ = conn.execute(
            "UPDATE accounts SET last_seen_at = ?1 WHERE identity_key = ?2",
            params![now, ident.key()],
        );
    }

    let display_name = ident.email.clone().unwrap_or_else(|| ident.user_id.clone());
    let mut msg = format!("Switched active Codex account to: {display_name}");

    if restart {
        match restart_codex(true, true).await {
            Ok(r_msg) => msg.push_str(&format!(" ({r_msg})")),
            Err(e) => msg.push_str(&format!(" (Restart failed: {e})")),
        }
    }

    Ok(msg)
}
