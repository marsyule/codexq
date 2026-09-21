//! Third-party AI model provider management, key sandboxing, and connectivity testing.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::auth::atomic_write;
use super::paths::providers_dir;

/// Minimum guaranteed context window size in tokens (256k).
pub const DEFAULT_MIN_CONTEXT_WINDOW: u64 = 256_000;

/// Data representation of a third-party AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub wire_api: String,
    pub active_model: String,
    pub models: Vec<String>,
    pub context_window: Option<u64>,
    pub model_context_windows: Option<HashMap<String, u64>>,
    pub notes: Option<String>,
    pub custom_config_toml: Option<String>,
    pub custom_auth_json: Option<String>,
    pub key_masked: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Result of connectivity probe and models fetching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectivityResult {
    pub success: bool,
    pub status_code: Option<u16>,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub available_models: Vec<String>,
}

/// Reasoning effort tier definition in Codex model catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogReasoningLevel {
    pub effort: String,
    pub description: String,
}

/// Truncation policy for model context window in Codex model catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogTruncationPolicy {
    pub mode: String,
    pub limit: u64,
}

/// Full model catalog entry strictly matching official Codex desktop & CLI schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexModelCatalogEntry {
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub default_reasoning_level: String,
    pub supported_reasoning_levels: Vec<ModelCatalogReasoningLevel>,
    pub shell_type: String,
    pub visibility: String,
    pub supported_in_api: bool,
    pub priority: u64,
    pub additional_speed_tiers: Vec<String>,
    pub availability_nux: Option<String>,
    pub upgrade: Option<String>,
    pub base_instructions: Option<String>,
    pub model_messages: Option<serde_json::Value>,
    pub supports_reasoning_summaries: bool,
    pub default_reasoning_summary: String,
    pub support_verbosity: bool,
    pub default_verbosity: String,
    pub apply_patch_tool_type: String,
    pub web_search_tool_type: String,
    pub truncation_policy: ModelCatalogTruncationPolicy,
    pub supports_parallel_tool_calls: bool,
    pub supports_image_detail_original: bool,
    pub context_window: u64,
    pub max_context_window: u64,
    pub effective_context_window_percent: u64,
    pub experimental_supported_tools: Vec<String>,
    pub input_modalities: Vec<String>,
    pub supports_search_tool: bool,
    pub auto_compact_token_limit: Option<u64>,
    pub use_responses_lite: bool,
    pub service_tiers: Vec<serde_json::Value>,
}

/// Top-level model catalog structure for Codex desktop & CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexModelCatalog {
    pub models: Vec<CodexModelCatalogEntry>,
}

/// Generates a masked representation of an API key for safe display (e.g. "sk-****abcd").
#[must_use]
pub fn mask_api_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() <= 8 {
        return "****".to_string();
    }
    let prefix = if trimmed.starts_with("sk-") && trimmed.len() > 10 {
        &trimmed[..5]
    } else {
        &trimmed[..3]
    };
    let suffix = &trimmed[trimmed.len() - 4..];
    format!("{prefix}****{suffix}")
}

/// Computes the SHA256 hex digest of an API key string.
#[must_use]
pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Returns the sandbox directory for a provider (`~/.codexq/providers/<id>`).
#[must_use]
pub fn provider_sandbox_dir(id: &str) -> PathBuf {
    providers_dir().join(id)
}

/// Returns the path to the provider's secret key file (`~/.codexq/providers/<id>/key`).
#[must_use]
pub fn provider_key_path(id: &str) -> PathBuf {
    provider_sandbox_dir(id).join("key")
}

/// Returns the path to the provider's model catalog (`~/.codexq/providers/<id>/models.json`).
#[must_use]
pub fn provider_catalog_path(id: &str) -> PathBuf {
    provider_sandbox_dir(id).join("models.json")
}

/// Safely persists a provider's plaintext API key into its filesystem sandbox with restricted permissions.
///
/// # Arguments
///
/// * `id` - Provider identifier.
/// * `key` - Secret API key string.
///
/// # Errors
///
/// Returns `Err` if directory creation or file writing fails.
pub fn save_provider_key(id: &str, key: &str) -> Result<(), String> {
    let dir = provider_sandbox_dir(id);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create provider dir: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }

    let key_path = provider_key_path(id);
    atomic_write(&key_path, key.trim().as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// Reads a provider's plaintext API key from its filesystem sandbox.
///
/// # Arguments
///
/// * `id` - Provider identifier.
///
/// # Errors
///
/// Returns `Err` if the key file is missing or unreadable.
pub fn read_provider_key(id: &str) -> Result<String, String> {
    let key_path = provider_key_path(id);
    if !key_path.is_file() {
        return Err(format!("Key file for provider '{id}' not found"));
    }
    fs::read_to_string(&key_path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("Failed to read key for provider '{id}': {e}"))
}

/// Deletes a provider's sandbox directory and files.
///
/// # Arguments
///
/// * `id` - Provider identifier.
///
/// # Errors
///
/// Returns `Err` if removal fails.
pub fn delete_provider_files(id: &str) -> Result<(), String> {
    let dir = provider_sandbox_dir(id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Failed to remove provider dir: {e}"))?;
    }
    Ok(())
}

/// Builds a Codex model catalog structure for the given model pool.
///
/// Pure function with no filesystem side effects, so it can be unit-tested directly.
/// Context windows default to at least 256,000 tokens (256k) and smaller configured
/// values are clamped upward. Auto-compaction is scheduled at 85% of the working window.
///
/// # Arguments
///
/// * `active_model` - The currently active model slug (placed first with priority 1000).
/// * `models` - Additional model slugs to include in the catalog pool.
/// * `default_context_window` - Provider-level context window setting (minimum 256,000).
/// * `model_context_windows` - Optional map of per-model context window overrides.
#[must_use]
pub fn build_model_catalog(
    active_model: &str,
    models: &[String],
    default_context_window: Option<u64>,
    model_context_windows: Option<&HashMap<String, u64>>,
) -> CodexModelCatalog {
    let mut model_list: Vec<String> = Vec::new();
    let trimmed_active = active_model.trim();
    if !trimmed_active.is_empty() {
        model_list.push(trimmed_active.to_string());
    }
    for m in models {
        let clean = m.trim().to_string();
        if !clean.is_empty() && !model_list.contains(&clean) {
            model_list.push(clean);
        }
    }
    if model_list.is_empty() {
        model_list.push("default".to_string());
    }

    let default_reasoning_levels = vec![
        ModelCatalogReasoningLevel {
            effort: "low".to_string(),
            description: "Fast responses with lighter reasoning".to_string(),
        },
        ModelCatalogReasoningLevel {
            effort: "medium".to_string(),
            description: "Balances speed and reasoning depth for everyday tasks".to_string(),
        },
        ModelCatalogReasoningLevel {
            effort: "high".to_string(),
            description: "Greater reasoning depth for complex problems".to_string(),
        },
        ModelCatalogReasoningLevel {
            effort: "xhigh".to_string(),
            description: "Extra high reasoning depth for complex problems".to_string(),
        },
    ];

    let entries: Vec<CodexModelCatalogEntry> = model_list
        .iter()
        .enumerate()
        .map(|(idx, slug)| {
            let configured_ctx = model_context_windows
                .and_then(|map| map.get(slug).copied())
                .or(default_context_window)
                .unwrap_or(DEFAULT_MIN_CONTEXT_WINDOW);
            let context_window = configured_ctx.max(DEFAULT_MIN_CONTEXT_WINDOW);

            CodexModelCatalogEntry {
                slug: slug.clone(),
                display_name: slug.clone(),
                description: slug.clone(),
                default_reasoning_level: "medium".to_string(),
                supported_reasoning_levels: default_reasoning_levels.clone(),
                shell_type: "shell_command".to_string(),
                visibility: "list".to_string(),
                supported_in_api: true,
                priority: 1000 + idx as u64,
                additional_speed_tiers: Vec::new(),
                availability_nux: None,
                upgrade: None,
                base_instructions: Some("You are Codex, an AI coding assistant.".to_string()),
                model_messages: None,
                supports_reasoning_summaries: true,
                default_reasoning_summary: "none".to_string(),
                support_verbosity: true,
                default_verbosity: "low".to_string(),
                apply_patch_tool_type: "freeform".to_string(),
                web_search_tool_type: "text_and_image".to_string(),
                truncation_policy: ModelCatalogTruncationPolicy {
                    mode: "tokens".to_string(),
                    limit: 10000,
                },
                supports_parallel_tool_calls: true,
                supports_image_detail_original: true,
                context_window,
                max_context_window: context_window,
                effective_context_window_percent: 95,
                experimental_supported_tools: Vec::new(),
                input_modalities: vec!["text".to_string(), "image".to_string()],
                supports_search_tool: true,
                // Auto-compaction triggers at 85% of the configured working window,
                // which is tighter than Codex's 90% default and keeps the active
                // context free of stale history.
                auto_compact_token_limit: Some(context_window.saturating_mul(85) / 100),
                use_responses_lite: false,
                service_tiers: Vec::new(),
            }
        })
        .collect();

    CodexModelCatalog { models: entries }
}

/// Generates a standardized Codex model catalog artifact for a provider.
///
/// Writes `~/.codex/model-catalogs/codexq-<id>.json` and keeps a mirror copy in the
/// provider's sandbox directory.
///
/// Note that this artifact is intentionally NOT referenced from `config.toml` via a
/// root-level `model_catalog_json` key, because Codex CLI >= 0.149.1 rejects that
/// undeclared key and fails to load the configuration.
///
/// # Arguments
///
/// * `id` - Provider identifier.
/// * `active_model` - The currently active model slug (placed at the top with priority 1000).
/// * `models` - List of model slugs to include in the catalog pool.
/// * `default_context_window` - Provider-level context window setting (minimum 256,000).
/// * `model_context_windows` - Optional map of per-model context window overrides.
///
/// # Returns
///
/// The forward-slash relative path `model-catalogs/codexq-<id>.json`.
///
/// # Errors
///
/// Returns `Err` if file serialization or writing fails.
pub fn generate_model_catalog(
    id: &str,
    active_model: &str,
    models: &[String],
    default_context_window: Option<u64>,
    model_context_windows: Option<&HashMap<String, u64>>,
) -> Result<String, String> {
    let catalog =
        build_model_catalog(active_model, models, default_context_window, model_context_windows);
    let json_bytes = serde_json::to_vec_pretty(&catalog)
        .map_err(|e| format!("Failed to serialize model catalog: {e}"))?;

    // 1. Write to ~/.codex/model-catalogs/codexq-<id>.json
    let target_dir = super::paths::codex_home().join("model-catalogs");
    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Failed to create model-catalogs directory: {e}"))?;
    let target_file = target_dir.join(format!("codexq-{id}.json"));
    atomic_write(&target_file, &json_bytes)?;

    // 2. Also keep a copy in provider sandbox
    let sandbox_dir = provider_sandbox_dir(id);
    let _ = fs::create_dir_all(&sandbox_dir);
    let sandbox_file = provider_catalog_path(id);
    let _ = atomic_write(&sandbox_file, &json_bytes);

    // Return forward-slash relative path for config.toml
    Ok(format!("model-catalogs/codexq-{id}.json"))
}

/// Tests connectivity to an OpenAI-compatible endpoint and queries available models.
///
/// # Arguments
///
/// * `base_url` - The base URL of the API endpoint.
/// * `api_key` - The secret API key.
///
/// # Returns
///
/// A `ConnectivityResult` struct containing latency, status, message, and extracted model list.
pub async fn test_provider_connectivity(base_url: &str, api_key: &str) -> ConnectivityResult {
    let trimmed_url = base_url.trim().trim_end_matches('/');
    if trimmed_url.is_empty() {
        return ConnectivityResult {
            success: false,
            status_code: None,
            latency_ms: None,
            message: "Base URL cannot be empty".to_string(),
            available_models: Vec::new(),
        };
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ConnectivityResult {
                success: false,
                status_code: None,
                latency_ms: None,
                message: format!("Failed to build HTTP client: {e}"),
                available_models: Vec::new(),
            };
        }
    };

    // 1. Target /models endpoint for checking auth and fetching model list
    let models_url = if trimmed_url.ends_with("/v1") {
        format!("{trimmed_url}/models")
    } else {
        format!("{trimmed_url}/v1/models")
    };

    let start = Instant::now();
    let resp_res = client
        .get(&models_url)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .send()
        .await;

    let latency = start.elapsed().as_millis() as u64;

    match resp_res {
        Ok(resp) => {
            let status = resp.status();
            let status_code = status.as_u16();

            if status.is_success() {
                let mut models = Vec::new();
                if let Ok(json_val) = resp.json::<serde_json::Value>().await {
                    if let Some(data) = json_val.get("data").and_then(|d| d.as_array()) {
                        for item in data {
                            if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                                let id_clean = id.trim().to_string();
                                if !id_clean.is_empty() && !models.contains(&id_clean) {
                                    models.push(id_clean);
                                }
                            }
                        }
                    }
                }
                models.sort();

                ConnectivityResult {
                    success: true,
                    status_code: Some(status_code),
                    latency_ms: Some(latency),
                    message: format!("Connected successfully (HTTP {status_code})"),
                    available_models: models,
                }
            } else {
                let err_text = resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                let short_err = if err_text.len() > 180 {
                    format!("{}...", &err_text[..180])
                } else {
                    err_text
                };

                ConnectivityResult {
                    success: false,
                    status_code: Some(status_code),
                    latency_ms: Some(latency),
                    message: format!("HTTP {status_code}: {short_err}"),
                    available_models: Vec::new(),
                }
            }
        }
        Err(e) => {
            // Fallback: try raw URL/models if /v1/models failed to connect
            let fallback_url = format!("{trimmed_url}/models");
            if fallback_url != models_url {
                let fallback_start = Instant::now();
                if let Ok(fb_resp) = client
                    .get(&fallback_url)
                    .header("Authorization", format!("Bearer {}", api_key.trim()))
                    .send()
                    .await
                {
                    let fb_latency = fallback_start.elapsed().as_millis() as u64;
                    let fb_status = fb_resp.status();
                    let fb_code = fb_status.as_u16();
                    if fb_status.is_success() {
                        let mut models = Vec::new();
                        if let Ok(json_val) = fb_resp.json::<serde_json::Value>().await {
                            if let Some(data) = json_val.get("data").and_then(|d| d.as_array()) {
                                for item in data {
                                    if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                                        let id_clean = id.trim().to_string();
                                        if !id_clean.is_empty() && !models.contains(&id_clean) {
                                            models.push(id_clean);
                                        }
                                    }
                                }
                            }
                        }
                        models.sort();
                        return ConnectivityResult {
                            success: true,
                            status_code: Some(fb_code),
                            latency_ms: Some(fb_latency),
                            message: format!("Connected successfully (HTTP {fb_code})"),
                            available_models: models,
                        };
                    }
                }
            }

            ConnectivityResult {
                success: false,
                status_code: None,
                latency_ms: Some(latency),
                message: format!("Connection error: {e}"),
                available_models: Vec::new(),
            }
        }
    }
}
