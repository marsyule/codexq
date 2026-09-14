//! Authentication credentials parsing, JWT decoding, and profile sandboxing.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use base64::Engine;
use sha2::{Digest, Sha256};

use super::paths::{codex_home, profiles_dir};

/// Strongly typed account identity representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub user_id: String,
    pub account_id: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub org_title: Option<String>,
}

impl Identity {
    /// Returns the unique identity key: `"{user_id}\x1f{account_id}"`.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}\x1f{}", self.user_id, self.account_id)
    }

    /// Computes the 20-character profile ID hash.
    #[must_use]
    pub fn profile_id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.key().as_bytes());
        let hash = hasher.finalize();
        let hex = format!("{hash:x}");
        hex.chars().take(20).collect()
    }

    /// Returns the sandbox directory for this identity (`~/.codexq/profiles/<profile_id>`).
    #[must_use]
    pub fn profile_dir(&self) -> PathBuf {
        profiles_dir().join(self.profile_id())
    }
}

/// Computes the SHA256 hex string of a byte slice.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Decodes the payload portion of a JWT token without requiring external signature verification.
///
/// # Arguments
///
/// * `jwt` - Complete JWT string.
///
/// # Errors
///
/// Returns `Err` if the JWT format is invalid or base64 decoding fails.
pub fn decode_jwt_payload(jwt: &str) -> Result<serde_json::Value, String> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 || parts[1].is_empty() {
        return Err("id_token is not a valid 3-part JWT".to_string());
    }

    let raw_payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw_payload)
        .or_else(|_| {
            let mut padded = raw_payload.to_string();
            while padded.len() % 4 != 0 {
                padded.push('=');
            }
            base64::engine::general_purpose::URL_SAFE
                .decode(&padded)
                .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&padded))
        })
        .map_err(|e| format!("Cannot decode JWT payload base64: {e}"))?;

    serde_json::from_slice(&decoded).map_err(|e| format!("Cannot parse JWT payload JSON: {e}"))
}

/// Extracts identity metadata from parsed `auth.json` JSON.
///
/// # Arguments
///
/// * `auth` - Parsed JSON object from `auth.json`.
///
/// # Errors
///
/// Returns `Err` if user_id or account_id cannot be resolved.
pub fn extract_identity(auth: &serde_json::Value) -> Result<Identity, String> {
    let mut user_id: Option<String> = None;
    let mut account_id: Option<String> = None;
    let mut email: Option<String> = None;
    let mut plan: Option<String> = None;
    let mut org_title: Option<String> = None;

    if let Some(tokens) = auth.get("tokens").and_then(|t| t.as_object()) {
        if let Some(id_token) = tokens.get("id_token").and_then(|t| t.as_str()) {
            if let Ok(claims) = decode_jwt_payload(id_token) {
                let auth_claims = claims.get("https://api.openai.com/auth");
                let profile_claims = claims.get("https://api.openai.com/profile");

                if let Some(ac) = auth_claims {
                    user_id = ac
                        .get("chatgpt_user_id")
                        .or_else(|| ac.get("user_id"))
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned);

                    account_id = tokens
                        .get("account_id")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned)
                        .or_else(|| {
                            ac.get("chatgpt_account_id")
                                .and_then(|v| v.as_str())
                                .map(ToOwned::to_owned)
                        });

                    if let Some(plan_val) = ac.get("chatgpt_plan_type") {
                        plan = if plan_val.is_string() {
                            plan_val.as_str().map(ToOwned::to_owned)
                        } else {
                            Some(plan_val.to_string())
                        };
                    }

                    if let Some(orgs) = ac.get("organizations").and_then(|o| o.as_array()) {
                        for org in orgs {
                            if org.get("is_default").and_then(|b| b.as_bool()).unwrap_or(false) {
                                org_title = org.get("title").and_then(|t| t.as_str()).map(ToOwned::to_owned);
                                break;
                            }
                        }
                        if org_title.is_none() && !orgs.is_empty() {
                            org_title = orgs[0].get("title").and_then(|t| t.as_str()).map(ToOwned::to_owned);
                        }
                    }
                }

                email = claims
                    .get("email")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        profile_claims
                            .and_then(|p| p.get("email"))
                            .and_then(|v| v.as_str())
                            .map(ToOwned::to_owned)
                    });
            }
        }
    }

    if let Some(agent_identity) = auth.get("agent_identity").and_then(|a| a.as_object()) {
        if user_id.is_none() {
            user_id = agent_identity.get("chatgpt_user_id").and_then(|v| v.as_str()).map(ToOwned::to_owned);
        }
        if account_id.is_none() {
            account_id = agent_identity.get("account_id").and_then(|v| v.as_str()).map(ToOwned::to_owned);
        }
        if email.is_none() {
            email = agent_identity.get("email").and_then(|v| v.as_str()).map(ToOwned::to_owned);
        }
        if plan.is_none() {
            plan = agent_identity.get("plan_type").map(|v| {
                if v.is_string() {
                    v.as_str().unwrap().to_string()
                } else {
                    v.to_string()
                }
            });
        }
        if org_title.is_none() {
            org_title = agent_identity.get("org_title").and_then(|v| v.as_str()).map(ToOwned::to_owned);
        }
    }

    let uid = user_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Cannot find chatgpt_user_id in auth.json".to_string())?;

    let aid = account_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Cannot find chatgpt_account_id in auth.json".to_string())?;

    Ok(Identity {
        user_id: uid,
        account_id: aid,
        email: email.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        plan: plan.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        org_title: org_title.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    })
}

/// Checks whether `candidate` credentials are newer than or equal to `existing` credentials.
///
/// Compares `last_refresh` ISO timestamps first; falls back to JWT expiration timestamp.
/// If timestamps cannot be determined, returns `true` (permissive).
#[must_use]
pub fn is_auth_newer_or_equal(candidate: &serde_json::Value, existing: &serde_json::Value) -> bool {
    let cand_lr = candidate.get("last_refresh").and_then(|v| v.as_str());
    let exist_lr = existing.get("last_refresh").and_then(|v| v.as_str());

    if let (Some(c_str), Some(e_str)) = (cand_lr, exist_lr) {
        if let (Ok(c_dt), Ok(e_dt)) = (
            chrono::DateTime::parse_from_rfc3339(c_str),
            chrono::DateTime::parse_from_rfc3339(e_str),
        ) {
            return c_dt >= e_dt;
        }
    }

    let get_exp = |val: &serde_json::Value| -> Option<i64> {
        let tok = val.get("tokens")?.get("access_token")?.as_str()?;
        let claims = decode_jwt_payload(tok).ok()?;
        claims.get("exp")?.as_i64()
    };

    if let (Some(c_exp), Some(e_exp)) = (get_exp(candidate), get_exp(existing)) {
        return c_exp >= e_exp;
    }

    true
}

/// Ensures a `config.toml` file contains `cli_auth_credentials_store = "file"`.
/// If the file does not exist, it is created.
/// If it exists and lacks the key, it is appended.
///
/// # Errors
///
/// Returns `Err` if file reading or writing fails.
pub fn ensure_config_file_has_file_store(config_toml: &Path) -> Result<(), String> {
    let desired = "cli_auth_credentials_store = \"file\"\n";
    if !config_toml.exists() {
        if let Some(parent) = config_toml.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {e}"))?;
        }
        atomic_write(config_toml, desired.as_bytes())?;
        return Ok(());
    }

    let text = fs::read_to_string(config_toml).unwrap_or_default();
    if !text.contains("cli_auth_credentials_store") {
        let mut new_text = text;
        if !new_text.is_empty() && !new_text.ends_with('\n') {
            new_text.push('\n');
        }
        new_text.push_str(desired);
        atomic_write(config_toml, new_text.as_bytes())?;
    }
    Ok(())
}

/// Ensures the profile sandbox directory has a `config.toml` forcing `cli_auth_credentials_store = "file"`.
///
/// # Arguments
///
/// * `profile_dir` - Path to the profile directory.
///
/// # Errors
///
/// Returns `Err` if directory creation or file writing fails.
pub fn ensure_profile_config(profile_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(profile_dir).map_err(|e| format!("Failed to create profile dir: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(profile_dir, fs::Permissions::from_mode(0o700));
    }
    ensure_config_file_has_file_store(&profile_dir.join("config.toml"))
}

/// Ensures the official host Codex directory (`~/.codex/config.toml`) has `cli_auth_credentials_store = "file"`.
///
/// # Errors
///
/// Returns `Err` if file writing fails.
pub fn ensure_host_codex_config() -> Result<(), String> {
    let host_config = codex_home().join("config.toml");
    ensure_config_file_has_file_store(&host_config)
}

/// Atomically writes content to a target file using a temporary file and atomic replace.
///
/// # Arguments
///
/// * `path` - Destination file path.
/// * `data` - Byte slice to write.
///
/// # Errors
///
/// Returns `Err` if writing or renaming fails.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create parent dir: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp");
    let tmp_path = path.with_file_name(format!(
        ".{file_name}.{}_{}.tmp",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));

    {
        let mut file = File::create(&tmp_path)
            .map_err(|e| format!("Failed to create temp file: {e}"))?;
        file.write_all(data)
            .map_err(|e| format!("Failed to write temp file: {e}"))?;
        file.flush()
            .map_err(|e| format!("Failed to flush temp file: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync temp file: {e}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let is_credential = file_name.starts_with("auth");
            let mode = if is_credential { 0o600 } else { 0o644 };
            let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(mode));
        }
    }

    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to atomically replace file {}: {e}", path.display())
    })?;

    Ok(())
}

/// Parses raw bytes of an `auth.json`, returning the parsed JSON, SHA256 checksum, and original raw bytes.
///
/// # Errors
///
/// Returns `Err` if JSON parsing fails.
pub fn parse_auth_bytes(raw: Vec<u8>) -> Result<(serde_json::Value, String, Vec<u8>), String> {
    let val: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| format!("Cannot parse auth JSON: {e}"))?;
    let sha = sha256_hex(&raw);
    Ok((val, sha, raw))
}

/// Reads an auth file, returning the parsed JSON, SHA256 checksum, and raw bytes.
///
/// # Arguments
///
/// * `path` - Path to `auth.json`.
///
/// # Errors
///
/// Returns `Err` if reading or JSON parsing fails.
pub fn read_auth_file(path: &Path) -> Result<(serde_json::Value, String, Vec<u8>), String> {
    let raw = fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    parse_auth_bytes(raw)
}

/// Checks whether an account's OAuth access_token is expired or will expire within `buffer_secs`.
///
/// Returns `false` if the credentials do not use an OAuth token (e.g. API key mode).
///
/// # Arguments
///
/// * `val` - Deserialized `auth.json` JSON structure.
/// * `buffer_secs` - Pre-expiration safety window in seconds (e.g. 300 for 5 minutes).
///
/// # Returns
///
/// `true` if the access token is missing, unparseable, or expired/expiring within buffer.
#[must_use]
pub fn is_access_token_expired(val: &serde_json::Value, buffer_secs: i64) -> bool {
    let tokens = match val.get("tokens") {
        Some(t) if t.is_object() => t,
        _ => return false,
    };
    let tok = match tokens.get("access_token").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return true,
    };
    let claims = match decode_jwt_payload(tok) {
        Ok(c) => c,
        Err(_) => return true,
    };
    let exp = match claims.get("exp").and_then(|v| v.as_i64()) {
        Some(e) => e,
        None => return false,
    };
    let now = chrono::Utc::now().timestamp();
    now + buffer_secs >= exp
}

/// Refreshes OAuth access_token using refresh_token for a specific profile directory.
///
/// Implements the official OpenAI OAuth 2.0 Refresh Token Rotation (RTR) flow.
/// Upon success, atomically updates `auth.json` in `profile_dir`, updates database metadata,
/// and synchronizes `~/.codex/auth.json` if this profile is currently active.
///
/// # Arguments
///
/// * `profile_dir` - Path to the profile sandbox directory containing `auth.json`.
///
/// # Returns
///
/// `Ok(true)` if token renewal succeeded and updated credentials were saved.
///
/// # Errors
///
/// Returns `Err` if:
/// - `auth.json` does not exist or lacks a refresh_token.
/// - The OAuth endpoint rejects the refresh token (e.g. invalid or revoked refresh token).
/// - Network communication times out or fails on all endpoints.
/// - Atomic file writing fails.
pub async fn refresh_oauth_token_for_profile(profile_dir: &Path) -> Result<bool, String> {
    let auth_path = profile_dir.join("auth.json");
    if !auth_path.is_file() {
        return Err(format!("Auth file not found at {}", auth_path.display()));
    }

    let (mut auth_val, _, _) = read_auth_file(&auth_path)?;
    let (refresh_token, client_id) = {
        let tokens = auth_val
            .get("tokens")
            .and_then(|t| t.as_object())
            .ok_or_else(|| "No tokens object found in auth.json".to_string())?;

        let r_tok = tokens
            .get("refresh_token")
            .and_then(|t| t.as_str())
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| "No refresh_token found in auth.json".to_string())?
            .to_string();

        let c_id = tokens
            .get("access_token")
            .and_then(|t| t.as_str())
            .and_then(|tok| decode_jwt_payload(tok).ok())
            .and_then(|claims| claims.get("client_id").and_then(|c| c.as_str()).map(ToOwned::to_owned))
            .unwrap_or_else(|| "app_EMoamEEZ73f0CkXaXp7hrann".to_string());

        (r_tok, c_id)
    };

    let payload = serde_json::json!({
        "client_id": client_id,
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let endpoints = [
        "https://auth.openai.com/oauth/token",
        "https://auth0.openai.com/oauth/token",
    ];

    let mut last_error = String::new();
    let mut response_opt = None;

    for endpoint in endpoints {
        match client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .header("User-Agent", "Codex/0.149.1")
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                response_opt = Some(resp);
                break;
            }
            Err(e) => {
                last_error = format!("Request to {endpoint} failed: {e}");
            }
        }
    }

    let resp = match response_opt {
        Some(r) => r,
        None => return Err(last_error),
    };

    let status = resp.status();
    let resp_bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read OAuth response body: {e}"))?;

    let resp_json: serde_json::Value = serde_json::from_slice(&resp_bytes)
        .map_err(|e| format!("Failed to parse OAuth response JSON: {e}"))?;

    if !status.is_success() {
        let err_msg = resp_json
            .get("error")
            .and_then(|err| {
                if let Some(msg) = err.get("message").and_then(|m| m.as_str()) {
                    Some(msg.to_string())
                } else if let Some(code) = err.get("code").and_then(|c| c.as_str()) {
                    Some(format!("OAuth error ({code})"))
                } else {
                    err.as_str().map(ToOwned::to_owned)
                }
            })
            .or_else(|| {
                resp_json
                    .get("error_description")
                    .and_then(|d| d.as_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| format!("OAuth token refresh failed with HTTP {status}"));

        return Err(err_msg);
    }

    let new_access_token = resp_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "OAuth response missing access_token".to_string())?;

    let new_refresh_token = resp_json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or(refresh_token);

    let new_id_token = resp_json.get("id_token").and_then(|v| v.as_str());

    // Update tokens in auth_val
    if let Some(tokens_mut) = auth_val.get_mut("tokens").and_then(|t| t.as_object_mut()) {
        tokens_mut.insert(
            "access_token".to_string(),
            serde_json::Value::String(new_access_token.to_string()),
        );
        tokens_mut.insert(
            "refresh_token".to_string(),
            serde_json::Value::String(new_refresh_token.to_string()),
        );
        if let Some(id_tok) = new_id_token {
            tokens_mut.insert(
                "id_token".to_string(),
                serde_json::Value::String(id_tok.to_string()),
            );
        }
    }

    let now_iso = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    if let Some(map) = auth_val.as_object_mut() {
        map.insert("last_refresh".to_string(), serde_json::Value::String(now_iso));
    }

    let new_raw = serde_json::to_vec_pretty(&auth_val)
        .map_err(|e| format!("Failed to serialize updated auth.json: {e}"))?;

    // Atomic write back to profile sandbox
    atomic_write(&auth_path, &new_raw)?;

    // Extract identity to sync SQLite metadata
    let ident = extract_identity(&auth_val)?;
    let _ = super::db::sync_profile_auth_metadata(&ident.key(), profile_dir);

    // If this profile is active in ~/.codex/auth.json, synchronize host credentials
    if let Some(cur_key) = super::db::get_current_identity_key() {
        if cur_key == ident.key() {
            let host_auth = super::paths::active_auth_path();
            let _ = atomic_write(&host_auth, &new_raw);
        }
    }

    log::info!(
        "Successfully refreshed OAuth tokens for profile {}",
        profile_dir.display()
    );
    Ok(true)
}
