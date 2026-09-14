//! Tauri command handlers directly bridging GUI actions to the Pure Rust Core.

use serde_json::Value;

/// Returns full list of accounts with their latest rate limit quota.
///
/// # Errors
///
/// Returns `Err` if SQLite querying or JSON serialization fails.
#[tauri::command]
pub async fn list_accounts() -> Result<Value, String> {
    let accounts = crate::core::db::list_accounts_with_quota()?;
    serde_json::to_value(accounts).map_err(|e| e.to_string())
}

/// Refreshes quotas for all accounts concurrently.
///
/// # Arguments
///
/// * `concurrency` - Number of concurrent probe workers (defaults to 5).
///
/// # Errors
///
/// Returns `Err` if rate limit probing fails.
#[tauri::command]
pub async fn refresh_all(concurrency: Option<u32>) -> Result<Value, String> {
    crate::core::probe::refresh_all(concurrency.unwrap_or(5)).await
}

/// Switches active credentials to the target account.
///
/// # Arguments
///
/// * `target` - Target identity key, email, or alias.
/// * `restart` - Whether to restart the Codex application.
///
/// # Errors
///
/// Returns `Err` if the target account does not exist or switching fails.
#[tauri::command]
pub async fn switch_account(target: String, restart: Option<bool>) -> Result<String, String> {
    crate::core::switch::switch_account(&target, restart.unwrap_or(false)).await
}

/// Restarts or launches the Codex application.
///
/// # Arguments
///
/// * `relaunch` - Whether to relaunch after terminating.
/// * `start_if_not_running` - Whether to launch if no process was running.
///
/// # Errors
///
/// Returns `Err` if process management fails.
#[tauri::command]
pub async fn restart_codex(
    relaunch: Option<bool>,
    start_if_not_running: Option<bool>,
) -> Result<String, String> {
    crate::core::process::restart_codex(
        relaunch.unwrap_or(true),
        start_if_not_running.unwrap_or(true),
    )
    .await
}

/// Sets or removes an account alias.
///
/// # Arguments
///
/// * `target` - Account identifier.
/// * `alias` - Custom alias string.
///
/// # Errors
///
/// Returns `Err` if database update fails or account not found.
#[tauri::command]
pub async fn set_alias(target: String, alias: Option<String>) -> Result<String, String> {
    let clean_alias = alias.and_then(|a| {
        let t = a.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    let ok = crate::core::db::set_alias(&target, clean_alias.as_deref())?;
    if ok {
        Ok("Alias updated successfully".to_string())
    } else {
        Err(format!("Account '{target}' not found"))
    }
}

/// Resets all account aliases.
///
/// # Errors
///
/// Returns `Err` if database update fails.
#[tauri::command]
pub async fn reset_all_aliases() -> Result<String, String> {
    let count = crate::core::db::reset_all_aliases()?;
    Ok(format!("Reset {count} aliases"))
}

/// Returns quota snapshots history for an account.
///
/// # Arguments
///
/// * `target` - Account identifier.
/// * `limit` - Maximum number of records to return.
///
/// # Errors
///
/// Returns `Err` if querying history fails.
#[tauri::command]
pub async fn get_history(target: String, limit: Option<u32>) -> Result<Value, String> {
    let snapshots = crate::core::db::get_history(&target, limit.unwrap_or(30))?;
    serde_json::to_value(snapshots).map_err(|e| e.to_string())
}

/// Moves an account to the recycle bin.
///
/// # Arguments
///
/// * `target` - Account identifier.
///
/// # Errors
///
/// Returns `Err` if removal fails or account not found.
#[tauri::command]
pub async fn remove_account(target: String) -> Result<String, String> {
    let ok = crate::core::db::remove_account(&target)?;
    if ok {
        Ok(format!("Account '{target}' moved to recycle bin"))
    } else {
        Err(format!("Account '{target}' not found"))
    }
}

/// Lists all accounts in the recycle bin.
///
/// # Errors
///
/// Returns `Err` if querying trash fails.
#[tauri::command]
pub async fn list_trash() -> Result<Value, String> {
    let trash = crate::core::db::list_trash()?;
    serde_json::to_value(trash).map_err(|e| e.to_string())
}

/// Restores an account from the recycle bin.
///
/// # Arguments
///
/// * `target` - Account identifier.
///
/// # Errors
///
/// Returns `Err` if restoration fails.
#[tauri::command]
pub async fn restore_account(target: String) -> Result<String, String> {
    crate::core::db::restore_account(&target)
}

/// Permanently purges an account or all accounts from the recycle bin.
///
/// # Arguments
///
/// * `target` - Optional specific account identifier.
///
/// # Errors
///
/// Returns `Err` if purging fails.
#[tauri::command]
pub async fn purge_trash(target: Option<String>) -> Result<String, String> {
    let clean_target = target.and_then(|t| {
        let s = t.trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });
    crate::core::db::purge_trash(clean_target.as_deref())
}

/// Returns all application configuration key-value pairs.
///
/// # Errors
///
/// Returns `Err` if serialization fails.
#[tauri::command]
pub async fn get_app_settings() -> Result<Value, String> {
    let settings = crate::core::config::get_all_settings();
    serde_json::to_value(settings).map_err(|e| e.to_string())
}

/// Updates a single application configuration setting.
///
/// # Arguments
///
/// * `key` - Configuration dot-notation key.
/// * `value` - Setting value as string.
///
/// # Errors
///
/// Returns `Err` if writing configuration fails.
#[tauri::command]
pub async fn set_app_setting(key: String, value: String) -> Result<String, String> {
    crate::core::config::set_setting(&key, &value)?;
    Ok("Setting updated".to_string())
}

/// Lists scheduled alarms for an account or all accounts.
///
/// # Arguments
///
/// * `target` - Optional specific account identifier.
///
/// # Errors
///
/// Returns `Err` if database querying fails.
#[tauri::command]
pub async fn list_account_alarms(target: Option<String>) -> Result<Value, String> {
    let alarms = crate::core::scheduler::list_account_alarms(target.as_deref())?;
    serde_json::to_value(alarms).map_err(|e| e.to_string())
}

/// Creates or updates a scheduled account alarm.
///
/// # Arguments
///
/// * `alarm` - Alarm data payload.
///
/// # Errors
///
/// Returns `Err` if interval constraints fail or database writing fails.
#[tauri::command]
pub async fn save_account_alarm(alarm: Value) -> Result<Value, String> {
    let saved = crate::core::scheduler::save_account_alarm(alarm)?;
    serde_json::to_value(saved).map_err(|e| e.to_string())
}

/// Deletes an account alarm by ID.
///
/// # Arguments
///
/// * `id` - Alarm ID.
///
/// # Errors
///
/// Returns `Err` if deletion fails.
#[tauri::command]
pub async fn delete_account_alarm(
    id: Option<String>,
    alarm_id: Option<String>,
) -> Result<String, String> {
    let target_id = id
        .or(alarm_id)
        .ok_or_else(|| "Missing required alarm id".to_string())?;
    let ok = crate::core::scheduler::delete_account_alarm(&target_id)?;
    if ok {
        Ok("Alarm deleted".to_string())
    } else {
        Err(format!("Alarm '{target_id}' not found"))
    }
}

/// Triggers an immediate rate limit window warmup ping.
///
/// # Arguments
///
/// * `target` - Optional account identifier.
/// * `model` - Optional model override.
/// * `prompt` - Optional prompt override.
/// * `force` - Whether to bypass active rate-limit window checks.
/// * `timeout` - Timeout in seconds.
///
/// # Errors
///
/// Returns `Err` if execution fails.
#[tauri::command]
pub async fn trigger_warmup(
    target: Option<String>,
    model: Option<String>,
    prompt: Option<String>,
    force: Option<bool>,
    timeout: Option<f64>,
) -> Result<Value, String> {
    let t_secs = timeout.unwrap_or(90.0);
    crate::core::warmup::trigger_warmup(
        target,
        model,
        prompt,
        force.unwrap_or(false),
        t_secs,
    )
    .await
}

/// Updates the desktop shell locale and rebuilds the system tray menu.
///
/// # Arguments
///
/// * `app` - Tauri application handle.
/// * `locale` - Language code ("zh-CN", "en-US", etc.).
///
/// # Errors
///
/// Returns `Err` if tray menu reconstruction fails.
#[tauri::command]
pub async fn set_locale(app: tauri::AppHandle, locale: String) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(new_menu) = crate::create_tray_menu(&app, &locale) {
            let _ = tray.set_menu(Some(new_menu));
        }
    }
    Ok(())
}

/// Ingests auth credentials provided as a JSON string.
///
/// # Arguments
///
/// * `content` - Raw JSON content of an `auth.json` file.
///
/// # Returns
///
/// The imported `AccountData` representation.
///
/// # Errors
///
/// Returns `Err` if parsing or SQLite persistence fails.
#[tauri::command]
pub async fn import_auth_content(content: String) -> Result<Value, String> {
    let raw = content.into_bytes();
    let (ident, is_new, changed) = crate::core::db::ingest_auth_raw(raw)?;
    let key = ident.key();
    let accounts = crate::core::db::list_accounts_with_quota()?;
    let found = accounts.into_iter().find(|a| a.identity_key == key);
    let mut res = serde_json::to_value(found).map_err(|e| e.to_string())?;
    if let Some(obj) = res.as_object_mut() {
        obj.insert("is_new".to_string(), Value::Bool(is_new));
        obj.insert("credential_changed".to_string(), Value::Bool(changed));
    }
    Ok(res)
}

/// Ingests auth credentials from a local file path.
///
/// # Arguments
///
/// * `path` - Path string to `auth.json`.
///
/// # Returns
///
/// The imported `AccountData` representation.
///
/// # Errors
///
/// Returns `Err` if reading, parsing, or SQLite persistence fails.
#[tauri::command]
pub async fn import_auth_file(path: String) -> Result<Value, String> {
    let p = std::path::PathBuf::from(path.trim());
    if !p.is_file() {
        return Err(format!("File not found: {}", p.display()));
    }
    let (ident, is_new, changed) = crate::core::db::ingest_auth(&p)?;
    let key = ident.key();
    let accounts = crate::core::db::list_accounts_with_quota()?;
    let found = accounts.into_iter().find(|a| a.identity_key == key);
    let mut res = serde_json::to_value(found).map_err(|e| e.to_string())?;
    if let Some(obj) = res.as_object_mut() {
        obj.insert("is_new".to_string(), Value::Bool(is_new));
        obj.insert("credential_changed".to_string(), Value::Bool(changed));
    }
    Ok(res)
}

/// Spawns an external terminal window executing `codex login`.
///
/// # Errors
///
/// Returns `Err` if spawning the terminal process fails.
#[tauri::command]
pub async fn launch_codex_login() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        #[cfg(target_os = "windows")]
        use std::os::windows::process::CommandExt;

        let mut cmd = std::process::Command::new("cmd.exe");
        cmd.args(["/c", "start", "cmd.exe", "/k", "codex login"]);
        cmd.creation_flags(0x08000000);
        cmd.spawn()
            .map_err(|e| format!("Failed to spawn terminal for codex login: {e}"))?;
        Ok("Terminal launched for codex login".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("osascript")
            .args(["-e", "tell application \"Terminal\" to do script \"codex login\""])
            .spawn()
            .map_err(|e| format!("Failed to spawn Terminal for codex login: {e}"))?;
        Ok("Terminal launched for codex login".to_string())
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let candidates: &[(&str, &[&str])] = &[
            ("x-terminal-emulator", &["-e", "codex", "login"]),
            ("gnome-terminal", &["--", "codex", "login"]),
            ("konsole", &["-e", "codex", "login"]),
            ("xfce4-terminal", &["-x", "codex", "login"]),
            ("alacritty", &["-e", "codex", "login"]),
            ("kitty", &["codex", "login"]),
            ("foot", &["codex", "login"]),
            ("tilix", &["-e", "codex", "login"]),
            ("terminator", &["-x", "codex", "login"]),
            ("xterm", &["-e", "codex", "login"]),
        ];

        for (term, args) in candidates {
            if let Ok(_) = std::process::Command::new(term).args(*args).spawn() {
                return Ok(format!("Terminal launched ({term}) for codex login"));
            }
        }

        if let Ok(_) = std::process::Command::new("sh")
            .args(["-c", "x-terminal-emulator -e 'codex login' || xterm -e 'codex login' || gnome-terminal -- codex login"])
            .spawn()
        {
            return Ok("Terminal launched via fallback for codex login".to_string());
        }

        Err("No supported Linux terminal emulator found. Please run 'codex login' manually in your terminal.".to_string())
    }
}

/// Runs system environment diagnostics and returns a structured health report.
///
/// # Errors
///
/// Returns `Err` if report serialization fails.
#[tauri::command]
pub async fn run_diagnostics() -> Result<Value, String> {
    let report = crate::core::doctor::run_diagnostics().await;
    serde_json::to_value(report).map_err(|e| e.to_string())
}
