//! Global configuration management (`~/.codexq/config.json`).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use serde::{Deserialize, Serialize};

use super::paths::{config_path, codexq_home};

fn default_schema_version() -> u32 {
    1
}

fn default_locale() -> String {
    "auto".to_string()
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_interval_minutes() -> u32 {
    15
}

fn default_model() -> String {
    "gpt-5.6-luna".to_string()
}

fn default_preset_models() -> Vec<String> {
    vec![
        "gpt-5.6-luna".to_string(),
        "o3-mini".to_string(),
        "gpt-4o".to_string(),
    ]
}

fn default_prompt() -> String {
    "ping".to_string()
}

fn default_min_interval_hours() -> u32 {
    5
}

/// Root structure of `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_schema_version", rename = "$schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub auto_refresh: AutoRefreshConfig,
    #[serde(default)]
    pub trigger: TriggerConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            general: GeneralConfig::default(),
            auto_refresh: AutoRefreshConfig::default(),
            trigger: TriggerConfig::default(),
        }
    }
}

/// General application preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_locale")]
    pub locale: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            locale: default_locale(),
        }
    }
}

/// Automated quota refresh preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRefreshConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u32,
    #[serde(default = "default_true")]
    pub refresh_on_startup: bool,
    #[serde(default = "default_true")]
    pub dynamic_reset_enabled: bool,
    #[serde(default = "default_true")]
    pub notify_on_quota_restored: bool,
    #[serde(default = "default_false")]
    pub notify_on_update: bool,
}

impl Default for AutoRefreshConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            interval_minutes: default_interval_minutes(),
            refresh_on_startup: default_true(),
            dynamic_reset_enabled: default_true(),
            notify_on_quota_restored: default_true(),
            notify_on_update: default_false(),
        }
    }
}

/// Automated warmup / ping trigger preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    #[serde(default = "default_model")]
    pub default_model: String,
    #[serde(default = "default_preset_models")]
    pub preset_models: Vec<String>,
    #[serde(default = "default_prompt")]
    pub prompt: String,
    #[serde(default = "default_true")]
    pub skip_if_active: bool,
    #[serde(default = "default_min_interval_hours")]
    pub min_interval_hours: u32,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            default_model: default_model(),
            preset_models: default_preset_models(),
            prompt: default_prompt(),
            skip_if_active: default_true(),
            min_interval_hours: default_min_interval_hours(),
        }
    }
}

/// Loads the application configuration from disk or returns defaults.
#[must_use]
pub fn load_config() -> AppConfig {
    let path = config_path();
    if !path.is_file() {
        let default_cfg = AppConfig::default();
        let _ = save_config(&default_cfg);
        return default_cfg;
    }

    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(cfg) => cfg,
            Err(err) => {
                log::warn!("Failed to parse config.json, using defaults: {err}");
                AppConfig::default()
            }
        },
        Err(err) => {
            log::warn!("Failed to read config.json, using defaults: {err}");
            AppConfig::default()
        }
    }
}

/// Atomically saves the application configuration to disk.
///
/// # Arguments
///
/// * `config` - Reference to configuration struct to persist.
///
/// # Errors
///
/// Returns `Err` if serialization, directory creation, or file writing fails.
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let target = config_path();
    let dir = codexq_home();
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;

    let json_text = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {e}"))?;

    let tmp_path = dir.join(format!(
        "config.json.tmp.{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));

    {
        let mut file = File::create(&tmp_path)
            .map_err(|e| format!("Failed to create temp config file: {e}"))?;
        file.write_all(json_text.as_bytes())
            .map_err(|e| format!("Failed to write temp config file: {e}"))?;
        file.write_all(b"\n")
            .map_err(|e| format!("Failed to write newline: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync temp config file: {e}"))?;
    }

    fs::rename(&tmp_path, &target).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to atomically replace config.json: {e}")
    })?;

    Ok(())
}

/// Returns a flat mapping of setting key-values expected by the GUI frontend.
#[must_use]
pub fn get_all_settings() -> HashMap<String, String> {
    let cfg = load_config();
    let mut map = HashMap::new();

    map.insert("general.locale".to_string(), cfg.general.locale);
    map.insert("warmup.default_model".to_string(), cfg.trigger.default_model);
    map.insert(
        "warmup.preset_models".to_string(),
        serde_json::to_string(&cfg.trigger.preset_models).unwrap_or_else(|_| "[]".to_string()),
    );
    map.insert("warmup.prompt".to_string(), cfg.trigger.prompt);
    map.insert(
        "warmup.skip_if_active".to_string(),
        if cfg.trigger.skip_if_active { "true" } else { "false" }.to_string(),
    );
    map.insert(
        "warmup.min_interval_hours".to_string(),
        cfg.trigger.min_interval_hours.to_string(),
    );
    map.insert(
        "auto_refresh.enabled".to_string(),
        if cfg.auto_refresh.enabled { "true" } else { "false" }.to_string(),
    );
    map.insert(
        "auto_refresh.interval_minutes".to_string(),
        cfg.auto_refresh.interval_minutes.to_string(),
    );
    map.insert(
        "auto_refresh.refresh_on_startup".to_string(),
        if cfg.auto_refresh.refresh_on_startup { "true" } else { "false" }.to_string(),
    );
    map.insert(
        "auto_refresh.dynamic_reset_enabled".to_string(),
        if cfg.auto_refresh.dynamic_reset_enabled { "true" } else { "false" }.to_string(),
    );
    map.insert(
        "auto_refresh.notify_on_quota_restored".to_string(),
        if cfg.auto_refresh.notify_on_quota_restored { "true" } else { "false" }.to_string(),
    );
    map.insert(
        "auto_refresh.notify_on_update".to_string(),
        if cfg.auto_refresh.notify_on_update { "true" } else { "false" }.to_string(),
    );

    map
}

/// Updates a single setting key with a stringified or JSON value.
///
/// # Arguments
///
/// * `key` - The dot-notation key.
/// * `value` - The value string to set.
///
/// # Errors
///
/// Returns `Err` if saving the updated config fails.
pub fn set_setting(key: &str, value: &str) -> Result<(), String> {
    let mut cfg = load_config();
    let val_trimmed = value.trim();

    match key {
        "general.locale" => cfg.general.locale = val_trimmed.to_string(),
        "warmup.default_model" => cfg.trigger.default_model = val_trimmed.to_string(),
        "warmup.preset_models" => {
            if let Ok(models) = serde_json::from_str::<Vec<String>>(val_trimmed) {
                cfg.trigger.preset_models = models;
            }
        }
        "warmup.prompt" => cfg.trigger.prompt = val_trimmed.to_string(),
        "warmup.skip_if_active" => {
            cfg.trigger.skip_if_active = val_trimmed.eq_ignore_ascii_case("true");
        }
        "warmup.min_interval_hours" => {
            if let Ok(h) = val_trimmed.parse::<u32>() {
                cfg.trigger.min_interval_hours = h;
            }
        }
        "auto_refresh.enabled" => {
            cfg.auto_refresh.enabled = val_trimmed.eq_ignore_ascii_case("true");
        }
        "auto_refresh.interval_minutes" => {
            if let Ok(m) = val_trimmed.parse::<u32>() {
                cfg.auto_refresh.interval_minutes = m;
            }
        }
        "auto_refresh.refresh_on_startup" => {
            cfg.auto_refresh.refresh_on_startup = val_trimmed.eq_ignore_ascii_case("true");
        }
        "auto_refresh.dynamic_reset_enabled" => {
            cfg.auto_refresh.dynamic_reset_enabled = val_trimmed.eq_ignore_ascii_case("true");
        }
        "auto_refresh.notify_on_quota_restored" => {
            cfg.auto_refresh.notify_on_quota_restored = val_trimmed.eq_ignore_ascii_case("true");
        }
        "auto_refresh.notify_on_update" => {
            cfg.auto_refresh.notify_on_update = val_trimmed.eq_ignore_ascii_case("true");
        }
        _ => {
            log::warn!("Unrecognized config setting key: {key}");
        }
    }

    save_config(&cfg)
}
