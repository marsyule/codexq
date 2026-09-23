//! Unified runtime slot switching engine managing both `auth.json` and `config.toml`.

use std::fs;
use std::path::PathBuf;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use super::auth::{
    atomic_write, ensure_host_codex_config, is_access_token_expired, read_auth_file,
    refresh_oauth_token_for_profile,
};
use super::db::{
    auto_sync_current, get_connection, get_provider, list_providers, resolve_account,
    update_provider_active_model,
};
use super::paths::{active_auth_path, backups_dir, codex_home};
use super::process::restart_codex;
use super::protocol_proxy::{self, CODEX_WIRE_API};
use super::provider::{generate_model_catalog, read_provider_key};

/// Maximum number of `~/.codexq/backups/<ts>_<reason>` snapshots retained on disk.
///
/// Older snapshots are pruned after every switch to bound disk usage of plaintext
/// credential backups.
const MAX_RUNTIME_SNAPSHOTS: usize = 20;

/// Returns true when the given `model_provider` id refers to the built-in official provider.
fn is_official_provider_id(id: &str) -> bool {
    matches!(id.trim().to_ascii_lowercase().as_str(), "openai" | "")
}

/// Rewrites every `[model_providers.*]` table into a shape Codex will actually load.
///
/// Codex validates **all** provider tables at startup, including tables no longer
/// referenced by any profile or `--model`. A single legacy `wire_api = "chat"` — or an
/// empty `name` — therefore fails deserialization for the **whole** config, which
/// disables every command while `codex doctor` still reports `config.load: fail`.
///
/// Normalization is consequently unconditional and full-sweep: there is no prior
/// behaviour to preserve, because such a config never loaded in the first place.
/// See AGENTS.md §3 invariant 5.
fn normalize_provider_tables(doc: &mut DocumentMut) {
    let Some(item) = doc.get_mut("model_providers") else {
        return;
    };

    match item {
        Item::Table(providers) => {
            for (id, entry) in providers.iter_mut() {
                if let Some(table) = entry.as_table_like_mut() {
                    normalize_provider_table(id.get(), table);
                }
            }
        }
        // `model_providers = { a = { ... } }` — not produced by CodexQ, but a user may
        // have hand-written it, and Codex validates it just the same.
        Item::Value(Value::InlineTable(providers)) => {
            for (id, entry) in providers.iter_mut() {
                if let Value::InlineTable(table) = entry {
                    normalize_provider_table(id.get(), table);
                }
            }
        }
        _ => {}
    }
}

/// Normalizes one provider table: `wire_api` must be `"responses"`, `name` must be non-empty.
fn normalize_provider_table(id: &str, table: &mut dyn TableLike) {
    if table.get("wire_api").and_then(|item| item.as_str()) != Some(CODEX_WIRE_API) {
        log::warn!("config.toml: normalizing unsupported wire_api on provider '{id}'");
        table.insert("wire_api", toml_edit::value(CODEX_WIRE_API));
    }

    let has_name = table
        .get("name")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .is_some_and(|name| !name.is_empty());
    if !has_name {
        log::warn!("config.toml: backfilling missing provider name on '{id}'");
        table.insert("name", toml_edit::value("Custom"));
    }
}

/// Points an existing `<id>` provider table at `base_url` with a Codex-legal `wire_api`.
///
/// Used when an already-active provider is edited. Adding or removing a per-model protocol
/// override changes whether that provider must go through the gateway, and leaving the old
/// address in place would silently keep the previous protocol in effect until the user
/// switched away and back.
///
/// A no-op when the provider table is absent, so a user-authored `custom_config_toml` that
/// replaced the standard table cannot be resurrected here.
///
/// # Arguments
///
/// * `doc` - Parsed `config.toml`.
/// * `id` - Provider table key to update.
/// * `base_url` - Address Codex should use, already resolved by
///   [`protocol_proxy::routed_base_url`].
pub fn apply_provider_routing(doc: &mut DocumentMut, id: &str, base_url: &str) {
    let Some(item) = doc.get_mut("model_providers") else {
        return;
    };
    match item {
        Item::Table(providers) => {
            if let Some(table) = providers
                .get_mut(id)
                .and_then(|entry| entry.as_table_like_mut())
            {
                set_provider_routing(table, base_url);
            }
        }
        Item::Value(Value::InlineTable(providers)) => {
            if let Some(Value::InlineTable(table)) = providers.get_mut(id) {
                set_provider_routing(table, base_url);
            }
        }
        _ => {}
    }
}

/// Writes `base_url` and the Codex-legal `wire_api` into one provider table.
fn set_provider_routing(table: &mut dyn TableLike, base_url: &str) {
    table.insert("base_url", toml_edit::value(base_url));
    table.insert("wire_api", toml_edit::value(CODEX_WIRE_API));
}

/// Leaves an inactive provider id available to historical desktop sessions without its secrets.
pub(super) fn deactivate_provider_entry(
    doc: &mut DocumentMut,
    id: &str,
    name: Option<&str>,
) -> bool {
    if name.is_some() && doc.get("model_providers").is_none() {
        doc["model_providers"] = Item::Table(Table::new());
    }
    let Some(providers) = doc
        .get_mut("model_providers")
        .and_then(|item| item.as_table_like_mut())
    else {
        return false;
    };

    if let Some(name) = name {
        let name = name
            .trim()
            .is_empty()
            .then_some("Inactive provider")
            .unwrap_or(name.trim());
        if let Some(table) = providers
            .get_mut(id)
            .and_then(|item| item.as_table_like_mut())
        {
            table.clear();
            table.insert("name", toml_edit::value(name));
            table.insert("base_url", toml_edit::value("http://127.0.0.1:1/v1"));
            table.insert("wire_api", toml_edit::value(CODEX_WIRE_API));
        } else {
            let mut table = Table::new();
            table["name"] = toml_edit::value(name);
            table["base_url"] = toml_edit::value("http://127.0.0.1:1/v1");
            table["wire_api"] = toml_edit::value(CODEX_WIRE_API);
            providers.insert(id, Item::Table(table));
        }
        true
    } else {
        providers.remove(id).is_some()
    }
}

/// Restores metadata-only provider tables for saved desktop sessions after an upgrade.
///
/// The active third-party provider remains untouched. Inactive providers are represented by
/// unreachable local endpoints, with no credentials or user-supplied custom fields.
///
/// # Errors
///
/// Returns Err if provider metadata cannot be read, the config cannot be parsed, or the
/// updated config cannot be written.
pub fn ensure_inactive_provider_stubs() -> Result<(), String> {
    let config_path = codex_home().join("config.toml");
    if !config_path.is_file() {
        return Ok(());
    }

    let providers = list_providers()?;
    if providers.is_empty() {
        return Ok(());
    }

    let content = fs::read_to_string(&config_path).map_err(|error| error.to_string())?;
    let mut doc = content.parse::<DocumentMut>().map_err(|error| error.to_string())?;
    let active_id = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let mut changed = false;

    for provider in providers {
        if active_id.as_deref() == Some(provider.id.as_str())
            || matches!(provider.id.as_str(), "openai" | "ollama" | "lmstudio")
        {
            continue;
        }
        changed |= deactivate_provider_entry(&mut doc, &provider.id, Some(&provider.name));
    }

    if changed {
        normalize_provider_tables(&mut doc);
        atomic_write(&config_path, doc.to_string().as_bytes())?;
    }
    Ok(())
}

/// Prunes the oldest runtime snapshots so at most `keep` directories remain.
fn prune_runtime_snapshots(keep: usize) {
    let base = backups_dir();
    let Ok(entries) = fs::read_dir(&base) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    if dirs.len() <= keep {
        return;
    }
    // Directory names start with a `%Y%m%d_%H%M%S` timestamp, so lexical sort
    // is chronological order.
    dirs.sort();
    let remove_count = dirs.len() - keep;
    for path in dirs.into_iter().take(remove_count) {
        let _ = fs::remove_dir_all(&path);
    }
}

/// Current active runtime mode of the Codex sub-system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ActiveRuntimeMode {
    Official {
        identity_key: Option<String>,
        email: Option<String>,
        plan: Option<String>,
        display_name: Option<String>,
    },
    Provider {
        provider_id: String,
        name: String,
        active_model: String,
        base_url: String,
        models: Vec<String>,
    },
}

/// Creates a timestamped pre-switch backup of both `auth.json` and `config.toml`.
pub fn create_runtime_snapshot(reason: &str) -> Result<(), String> {
    let now = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let snap_dir = backups_dir().join(format!("{now}_{reason}"));
    let _ = fs::create_dir_all(&snap_dir);

    let host_auth = active_auth_path();
    if host_auth.is_file() {
        if let Ok(bytes) = fs::read(&host_auth) {
            let _ = atomic_write(&snap_dir.join("auth.json"), &bytes);
        }
    }

    let host_config = codex_home().join("config.toml");
    if host_config.is_file() {
        if let Ok(bytes) = fs::read(&host_config) {
            let _ = atomic_write(&snap_dir.join("config.toml"), &bytes);
        }
    }

    prune_runtime_snapshots(MAX_RUNTIME_SNAPSHOTS);

    Ok(())
}

/// Detects the current active runtime mode by reading `~/.codex/config.toml` and `~/.codex/auth.json`.
///
/// # Errors
///
/// Returns `Err` if inspection encounters fatal I/O failure.
pub fn get_active_runtime_mode() -> Result<ActiveRuntimeMode, String> {
    let host_config = codex_home().join("config.toml");
    let mut active_provider_id: Option<String> = None;
    let mut active_model: Option<String> = None;

    if host_config.is_file() {
        if let Ok(content) = fs::read_to_string(&host_config) {
            if let Ok(doc) = content.parse::<DocumentMut>() {
                if let Some(p) = doc.get("model_provider").and_then(|v| v.as_str()) {
                    let p_clean = p.trim().to_string();
                    if !p_clean.is_empty() {
                        active_provider_id = Some(p_clean);
                    }
                }
                if let Some(m) = doc.get("model").and_then(|v| v.as_str()) {
                    let m_clean = m.trim().to_string();
                    if !m_clean.is_empty() {
                        active_model = Some(m_clean);
                    }
                }
            }
        }
    }

    // If model_provider is set in config.toml to a third-party id, we are in Provider mode.
    // The built-in `openai` provider id must be treated as official routing.
    if let Some(pid) = active_provider_id {
        if !is_official_provider_id(&pid) {
            if let Ok(Some(prov)) = get_provider(&pid) {
                let model = active_model.clone().unwrap_or(prov.active_model.clone());
                return Ok(ActiveRuntimeMode::Provider {
                    provider_id: prov.id,
                    name: prov.name,
                    active_model: model,
                    base_url: prov.base_url,
                    models: prov.models,
                });
            }

            // Fallback if provider was configured manually
            return Ok(ActiveRuntimeMode::Provider {
                provider_id: pid.clone(),
                name: pid,
                active_model: active_model.unwrap_or_default(),
                base_url: String::new(),
                models: Vec::new(),
            });
        }
    }

    // Otherwise, we are in Official mode; resolve active official account
    let active_auth = active_auth_path();
    if active_auth.is_file() {
        if let Ok((auth_val, _, _)) = read_auth_file(&active_auth) {
            if let Ok(ident) = super::auth::extract_identity(&auth_val) {
                let name = ident.email.clone().unwrap_or_else(|| ident.user_id.clone());
                return Ok(ActiveRuntimeMode::Official {
                    identity_key: Some(ident.key()),
                    email: ident.email,
                    plan: ident.plan,
                    display_name: Some(name),
                });
            }
        }
    }

    Ok(ActiveRuntimeMode::Official {
        identity_key: None,
        email: None,
        plan: None,
        display_name: None,
    })
}

/// Switches the active runtime slot to a third-party model provider.
///
/// Seamlessly updates `~/.codex/config.toml` while archiving current official tokens.
///
/// # Arguments
///
/// * `provider_id` - ID of the third-party provider.
/// * `model_override` - Optional specific model to activate within this provider.
/// * `restart` - Whether to restart Codex.
///
/// # Errors
///
/// Returns `Err` if provider is missing, key is unreadable, or file operations fail.
pub async fn switch_to_provider(
    provider_id: &str,
    model_override: Option<&str>,
    restart: bool,
) -> Result<String, String> {
    // 0. Ensure host ~/.codex/config.toml enforces cli_auth_credentials_store = "file"
    let _ = ensure_host_codex_config();

    // 1. Snapshot runtime before any mutation
    let _ = create_runtime_snapshot(&format!("to_provider_{provider_id}"));

    // 2. If currently in official mode, sync active token back to its profile
    auto_sync_current();

    // 3. Load provider & secret key
    let prov = get_provider(provider_id)?
        .ok_or_else(|| format!("Provider '{provider_id}' not found"))?;

    // `openai` / `ollama` / `lmstudio` are Codex built-ins: overriding one makes Codex
    // reject the entire config at load. Refuse before touching the filesystem.
    if protocol_proxy::is_reserved_provider_id(&prov.id) {
        return Err(format!(
            "Provider id '{}' collides with a Codex built-in provider id. Codex rejects a \
             config.toml that overrides it, so the switch was aborted. Rename the provider first.",
            prov.id
        ));
    }

    let api_key = read_provider_key(provider_id)?;

    let chosen_model = model_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| prov.active_model.clone());

    if chosen_model != prov.active_model {
        let _ = update_provider_active_model(provider_id, &chosen_model);
    }

    // 4. Generate the provider model catalog artifact.
    let catalog_path = generate_model_catalog(
        provider_id,
        &chosen_model,
        &prov.models,
        prov.context_window,
        prov.model_context_windows.as_ref(),
        prov.reasoning_levels.as_deref(),
        prov.model_reasoning_levels.as_ref(),
    )?;

    // 4.5. Resolve the effective base_url and protocol.
    //
    // Codex removed the chat wire protocol entirely (openai/codex discussion #7782), so
    // `wire_api` written into config.toml is ALWAYS "responses". Providers that only
    // speak Chat Completions are routed through the loopback protocol gateway instead;
    // see `core/protocol_proxy` and AGENTS.md §3 invariant 5.
    let uses_custom_config = prov
        .custom_config_toml
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());

    let mut gateway_note = String::new();
    let effective_base_url = if uses_custom_config {
        // A user-authored provider table owns its own base_url; rewriting it would
        // silently discard their configuration.
        if prov.needs_gateway() {
            gateway_note =
                " [custom config.toml kept as-is: the protocol gateway was NOT applied]".to_string();
        }
        prov.base_url.trim().to_string()
    } else {
        let (base_url, warning) = protocol_proxy::routed_base_url(&prov).await;
        match warning {
            // Never write a base_url that cannot be reached. Stay on the direct address and
            // report why, so the failure is diagnosable instead of surfacing as an
            // unexplained connection refusal.
            Some(err) => {
                gateway_note = format!(
                    " [WARNING: protocol gateway unavailable, provider left on a direct \
                     connection which its Chat Completions models cannot serve: {err}]"
                );
                log::warn!("switch_to_provider: {err}");
            }
            None if base_url != prov.base_url.trim() => {
                gateway_note = format!(" [upstream routed via the local protocol gateway at {base_url}]");
            }
            None => {}
        }
        base_url
    };

    // 5. Update ~/.codex/config.toml with toml_edit preserving existing user configs
    let host_config_path = codex_home().join("config.toml");
    let config_content = if host_config_path.is_file() {
        fs::read_to_string(&host_config_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut doc: DocumentMut = config_content.parse().unwrap_or_default();

    // Set model
    doc["model"] = Item::Value(Value::from(chosen_model.as_str()));
    // Set model_provider
    doc["model_provider"] = Item::Value(Value::from(provider_id));
    // Point Codex at the active provider's model list. Codex resolves this at startup.
    doc["model_catalog_json"] = Item::Value(Value::from(catalog_path));

    // Merge custom_config_toml if provided; otherwise standard provider injection
    if let Some(custom_toml) = prov.custom_config_toml.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(custom_doc) = custom_toml.parse::<DocumentMut>() {
            for (k, v) in custom_doc.iter() {
                doc[k] = v.clone();
            }
        }
    } else {
        // Ensure model_providers table exists
        if !doc.contains_key("model_providers") {
            doc["model_providers"] = Item::Table(Table::new());
        }

        let mut provider_table = Table::new();
        provider_table["name"] = Item::Value(Value::from(prov.name.as_str()));
        provider_table["base_url"] = Item::Value(Value::from(effective_base_url.as_str()));
        provider_table["wire_api"] = Item::Value(Value::from(CODEX_WIRE_API));
        provider_table["experimental_bearer_token"] = Item::Value(Value::from(api_key.as_str()));

        if let Some(mp) = doc.get_mut("model_providers").and_then(|i| i.as_table_like_mut()) {
            mp.insert(provider_id, Item::Table(provider_table));
        }
    }

    normalize_provider_tables(&mut doc);

    atomic_write(&host_config_path, doc.to_string().as_bytes())?;

    // If provider has custom_auth_json, write to active auth.json
    if let Some(custom_auth) = prov.custom_auth_json.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if serde_json::from_str::<serde_json::Value>(custom_auth).is_ok() {
            let _ = atomic_write(&active_auth_path(), custom_auth.as_bytes());
        }
    }

    let mut msg = format!("Switched active provider to '{}' (model: {})", prov.name, chosen_model);
    msg.push_str(&gateway_note);

    if restart {
        match restart_codex(true, true).await {
            Ok(r_msg) => msg.push_str(&format!(" ({r_msg})")),
            Err(e) => msg.push_str(&format!(" (Restart failed: {e})")),
        }
    }

    Ok(msg)
}

/// Atomically switches the active Codex CLI credentials to the target official account.
///
/// Also lifts third-party provider interception in `~/.codex/config.toml` to restore official routing.
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

    // 1. Snapshot runtime before any mutation
    let _ = create_runtime_snapshot("to_official");

    // 2. Sync currently active token back to its profile first
    auto_sync_current();

    // 3. Resolve destination official account
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

    // 4. Atomically overwrite active auth.json
    atomic_write(&dest, &raw_bytes)?;

    // 5. Restore config.toml to official routing.
    //
    // Only drop `model_provider` / `model` when they point at a third-party provider; a
    // user-authored `model_provider = "openai"` or a standalone `model` must be preserved.
    let host_config_path = codex_home().join("config.toml");
    if host_config_path.is_file() {
        if let Ok(content) = fs::read_to_string(&host_config_path) {
            if let Ok(mut doc) = content.parse::<DocumentMut>() {
                let mut changed = false;

                let active_pid = doc
                    .get("model_provider")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .filter(|s| !s.trim().is_empty());

                if let Some(pid) = active_pid.filter(|p| !is_official_provider_id(p)) {
                    for key in ["model_provider", "model"] {
                        if doc.remove(key).is_some() {
                            changed = true;
                        }
                    }
                    // Keep a harmless declaration so existing Codex Desktop threads can still
                    // resolve their provider id, but remove every active-provider setting and
                    // credential. The unroutable loopback URL prevents stale threads from
                    // accidentally sending requests to a third party after switching to OpenAI.
                    let inactive_name = get_provider(&pid)
                        .ok()
                        .flatten()
                        .map(|provider| provider.name);
                    changed |= deactivate_provider_entry(
                        &mut doc,
                        pid.as_str(),
                        inactive_name.as_deref(),
                    );
                }

                let remove_catalog = doc
                    .get("model_catalog_json")
                    .and_then(|value| value.as_str())
                    .is_some_and(is_codexq_model_catalog_path);
                if remove_catalog && doc.remove("model_catalog_json").is_some() {
                    changed = true;
                }

                if changed {
                    let _ = atomic_write(&host_config_path, doc.to_string().as_bytes());
                }
            }
        }
    }

    // Official mode never needs the protocol gateway.
    super::protocol_proxy::stop();

    // 6. Update last_seen_at in DB
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

/// Returns whether a model catalog path is managed by CodexQ.
pub(crate) fn is_codexq_model_catalog_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/model-catalogs/codexq-")
        || normalized.starts_with("model-catalogs/codexq-")
        || normalized.contains("/.codexq/providers/")
}
