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

/// Returns the auto-rollover configuration for a specific account.
///
/// # Arguments
///
/// * `identity_key` - Unique account identifier.
///
/// # Errors
///
/// Returns `Err` if serialization fails.
#[tauri::command]
pub async fn get_account_rollover(
    identity_key: String,
) -> Result<crate::core::config::AccountRolloverConfig, String> {
    Ok(crate::core::config::get_account_rollover(&identity_key))
}

/// Saves the auto-rollover configuration for a specific account.
///
/// # Arguments
///
/// * `identity_key` - Unique account identifier.
/// * `enabled` - Whether auto-rollover is enabled for this account.
/// * `min_weekly_remaining` - Minimum weekly remaining quota percent (0.0 - 100.0).
///
/// # Errors
///
/// Returns `Err` if saving config fails.
#[tauri::command]
pub async fn save_account_rollover(
    identity_key: String,
    enabled: bool,
    min_weekly_remaining: f64,
) -> Result<String, String> {
    crate::core::config::save_account_rollover(&identity_key, enabled, min_weekly_remaining)?;
    Ok("Account rollover configuration saved".to_string())
}

/// Payload for creating or updating a third-party provider.
#[derive(serde::Deserialize)]
pub struct SaveProviderPayload {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub wire_api: Option<String>,
    pub active_model: String,
    pub models: Vec<String>,
    pub context_window: Option<u64>,
    pub model_context_windows: Option<std::collections::HashMap<String, u64>>,
    pub notes: Option<String>,
    pub custom_config_toml: Option<String>,
    pub custom_auth_json: Option<String>,
    pub api_key: Option<String>,
}

/// Lists all configured third-party providers.
#[tauri::command]
pub async fn list_providers() -> Result<Value, String> {
    let providers = crate::core::db::list_providers()?;
    serde_json::to_value(providers).map_err(|e| e.to_string())
}

/// Saves or updates a third-party provider and its secret API key.
#[tauri::command]
pub async fn save_provider(payload: SaveProviderPayload) -> Result<Value, String> {
    let id = match payload.id {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            let slug = payload
                .name
                .trim()
                .to_lowercase()
                .replace(|c: char| !c.is_alphanumeric(), "-")
                .trim_matches('-')
                .to_string();
            if slug.is_empty() {
                format!("provider-{}", chrono::Utc::now().timestamp_millis())
            } else {
                // Never silently clobber an existing provider that slugifies to the same id.
                let mut candidate = slug.clone();
                let mut suffix = 2u32;
                while crate::core::db::get_provider(&candidate)?.is_some() {
                    candidate = format!("{slug}-{suffix}");
                    suffix += 1;
                }
                candidate
            }
        }
    };

    let existing = crate::core::db::get_provider(&id)?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let (key_masked, key_sha256) = match payload.api_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw_key) => {
            crate::core::provider::save_provider_key(&id, raw_key)?;
            (
                crate::core::provider::mask_api_key(raw_key),
                crate::core::provider::hash_api_key(raw_key),
            )
        }
        None => match existing.as_ref() {
            Some(ex) => (
                ex.key_masked.clone(),
                crate::core::db::get_provider_key_hash(&id)?.unwrap_or_default(),
            ),
            None => return Err("API key is required for new provider".to_string()),
        },
    };

    let mut models = payload.models;
    let active_model = payload.active_model.trim().to_string();
    if !active_model.is_empty() && !models.contains(&active_model) {
        models.insert(0, active_model.clone());
    }

    let provider = crate::core::provider::Provider {
        id: id.clone(),
        name: payload.name.trim().to_string(),
        base_url: payload.base_url.trim().to_string(),
        wire_api: payload.wire_api.unwrap_or_else(|| "responses".to_string()),
        active_model,
        models,
        context_window: payload.context_window,
        model_context_windows: payload.model_context_windows,
        notes: payload.notes.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        custom_config_toml: payload.custom_config_toml.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        custom_auth_json: payload.custom_auth_json.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        key_masked,
        created_at: existing.as_ref().map(|e| e.created_at.clone()).unwrap_or_else(|| now.clone()),
        updated_at: now,
    };

    crate::core::db::upsert_provider(&provider, &key_sha256)?;

    // Generate/refresh the provider model catalog artifact. It is deliberately NOT wired
    // into config.toml via a root-level `model_catalog_json` key, which Codex CLI >= 0.149.1
    // rejects as an undeclared config key.
    let _ = crate::core::provider::generate_model_catalog(
        &id,
        &provider.active_model,
        &provider.models,
        provider.context_window,
        provider.model_context_windows.as_ref(),
    )?;

    // If this provider is currently the active provider in ~/.codex/config.toml, sync config.toml
    if let Ok(crate::core::switch::ActiveRuntimeMode::Provider { provider_id, .. }) =
        crate::core::switch::get_active_runtime_mode()
    {
        if provider_id == id {
            let host_config_path = crate::core::paths::codex_home().join("config.toml");
            if host_config_path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&host_config_path) {
                    if let Ok(mut doc) = content.parse::<toml_edit::DocumentMut>() {
                        doc.remove("model_catalog_json");
                        doc["model"] =
                            toml_edit::Item::Value(toml_edit::Value::from(provider.active_model.as_str()));
                        let _ = crate::core::auth::atomic_write(&host_config_path, doc.to_string().as_bytes());
                    }
                }
            }
        }
    }

    serde_json::to_value(&provider).map_err(|e| e.to_string())
}

/// Deletes a third-party provider and purges its sandbox files.
#[tauri::command]
pub async fn delete_provider(id: String) -> Result<String, String> {
    let ok = crate::core::db::delete_provider(&id)?;
    if ok {
        Ok(format!("Provider '{id}' deleted successfully"))
    } else {
        Err(format!("Provider '{id}' not found"))
    }
}

/// Tests connectivity to an API endpoint and attempts to fetch its models list.
#[tauri::command]
pub async fn test_provider_connectivity(
    base_url: String,
    api_key: String,
    provider_id: Option<String>,
) -> Result<Value, String> {
    let resolved_key = if api_key.trim().is_empty() {
        if let Some(pid) = provider_id {
            crate::core::provider::read_provider_key(&pid).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        api_key
    };

    let res = crate::core::provider::test_provider_connectivity(&base_url, &resolved_key).await;
    serde_json::to_value(res).map_err(|e| e.to_string())
}

/// Switches the active runtime slot to a third-party model provider.
#[tauri::command]
pub async fn switch_to_provider(
    provider_id: String,
    model_override: Option<String>,
    restart: Option<bool>,
) -> Result<String, String> {
    crate::core::switch::switch_to_provider(
        &provider_id,
        model_override.as_deref(),
        restart.unwrap_or(false),
    )
    .await
}

/// Returns the current active runtime mode (Official account or Third-party provider).
#[tauri::command]
pub async fn get_active_runtime_mode() -> Result<Value, String> {
    let mode = crate::core::switch::get_active_runtime_mode()?;
    serde_json::to_value(mode).map_err(|e| e.to_string())
}
