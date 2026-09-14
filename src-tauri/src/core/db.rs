//! SQLite database operations, accounts schema, and persistence layer.

use std::fs;
use std::path::{Path, PathBuf};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::auth::{
    atomic_write, ensure_profile_config, extract_identity, is_auth_newer_or_equal, parse_auth_bytes,
    read_auth_file, Identity,
};
use super::paths::{active_auth_path, codex_home, db_path, profiles_dir, trash_dir};

/// Rate limit window representation matching frontend `RateLimitWindow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitWindow {
    pub label: String,
    pub used_percent: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub resets_at: Option<i64>,
}

/// Full account data representation matching frontend `AccountData`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountData {
    pub identity_key: String,
    pub profile_id: String,
    pub user_id: String,
    pub account_id: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub org_title: Option<String>,
    pub alias: Option<String>,
    pub display_name: String,
    pub is_current: bool,
    pub credential_status: String,
    pub last_seen_at: String,
    pub primary: RateLimitWindow,
    pub secondary: RateLimitWindow,
    pub reset_credits: Option<i64>,
    pub last_error: Option<String>,
}

/// Historical snapshot record matching frontend `SnapshotRecord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub id: i64,
    pub identity_key: String,
    pub limit_id: String,
    pub observed_at: String,
    pub primary_used_percent: Option<f64>,
    pub primary_window_minutes: Option<i64>,
    pub primary_resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub raw_json: Option<String>,
}

/// Trash account data matching frontend `TrashAccountData`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashAccountData {
    pub identity_key: String,
    pub profile_id: String,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub plan: Option<String>,
    pub display_name: String,
    pub removed_at: String,
    pub has_credentials: bool,
}

/// Account alarm record matching frontend `AccountAlarm`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountAlarm {
    pub id: String,
    pub identity_key: String,
    pub time_of_day: String,
    pub days_of_week: String,
    pub enabled: bool,
    pub model_override: Option<String>,
    pub prompt_override: Option<String>,
    pub last_triggered_at: Option<String>,
    pub last_status: Option<String>,
    pub created_at: String,
}

/// Returns an initialized SQLite connection with WAL mode and tables created.
///
/// # Errors
///
/// Returns `Err` if opening the database or executing migrations fails.
pub fn get_connection() -> Result<Connection, String> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let conn = Connection::open(&path).map_err(|e| format!("Failed to open codexq.db: {e}"))?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         
         CREATE TABLE IF NOT EXISTS accounts (
             identity_key TEXT PRIMARY KEY,
             profile_id TEXT NOT NULL UNIQUE,
             user_id TEXT NOT NULL,
             account_id TEXT NOT NULL,
             email TEXT,
             plan TEXT,
             alias TEXT,
             org_title TEXT,
             first_seen_at TEXT NOT NULL,
             last_seen_at TEXT NOT NULL,
             last_credential_update TEXT NOT NULL,
             credential_sha256 TEXT NOT NULL,
             credential_path TEXT NOT NULL,
             credential_status TEXT NOT NULL DEFAULT 'active',
             reset_credits INTEGER DEFAULT 0,
             last_error TEXT,
             UNIQUE(user_id, account_id)
         );

         CREATE TABLE IF NOT EXISTS quota_latest (
             identity_key TEXT NOT NULL,
             limit_id TEXT NOT NULL,
             fetched_at TEXT NOT NULL,
             primary_used_percent REAL,
             primary_window_minutes INTEGER,
             primary_resets_at INTEGER,
             secondary_used_percent REAL,
             secondary_window_minutes INTEGER,
             secondary_resets_at INTEGER,
             raw_json TEXT NOT NULL,
             PRIMARY KEY(identity_key, limit_id),
             FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS quota_snapshots (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             identity_key TEXT NOT NULL,
             limit_id TEXT NOT NULL,
             observed_at TEXT NOT NULL,
             primary_used_percent REAL,
             primary_window_minutes INTEGER,
             primary_resets_at INTEGER,
             secondary_used_percent REAL,
             secondary_window_minutes INTEGER,
             secondary_resets_at INTEGER,
             raw_json TEXT NOT NULL,
             FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
         );

         CREATE INDEX IF NOT EXISTS idx_quota_snapshots_identity_time
             ON quota_snapshots(identity_key, observed_at);

         CREATE TABLE IF NOT EXISTS removed_accounts (
             identity_key TEXT PRIMARY KEY,
             profile_id TEXT NOT NULL,
             email TEXT,
             user_id TEXT,
             plan TEXT,
             display_name TEXT,
             removed_at TEXT NOT NULL
         );

         CREATE TABLE IF NOT EXISTS account_alarms (
             id TEXT PRIMARY KEY,
             identity_key TEXT NOT NULL,
             time_of_day TEXT NOT NULL,
             days_of_week TEXT NOT NULL DEFAULT '1,2,3,4,5',
             enabled INTEGER NOT NULL DEFAULT 1,
             model_override TEXT,
             prompt_override TEXT,
             last_triggered_at TEXT,
             last_status TEXT,
             created_at TEXT NOT NULL,
             FOREIGN KEY(identity_key) REFERENCES accounts(identity_key) ON DELETE CASCADE
         );

         CREATE INDEX IF NOT EXISTS idx_account_alarms_identity
             ON account_alarms(identity_key);
        ",
    )
    .map_err(|e| format!("Failed to migrate database schema: {e}"))?;

    Ok(conn)
}

/// Ingests an `auth.json` file into SQLite and the profile sandbox.
///
/// # Arguments
///
/// * `source_path` - Path to credentials file.
///
/// Ingests raw `auth.json` bytes into SQLite and the profile sandbox.
///
/// # Returns
///
/// A tuple `(Identity, is_new, credential_changed)`.
///
/// # Errors
///
/// Returns `Err` if credentials cannot be parsed or written.
pub fn ingest_auth_raw(raw: Vec<u8>) -> Result<(Identity, bool, bool), String> {
    let (auth, new_sha, raw) = parse_auth_bytes(raw)?;
    let identity = extract_identity(&auth)?;
    let profile_dir = identity.profile_dir();

    ensure_profile_config(&profile_dir)?;
    let auth_dst = profile_dir.join("auth.json");

    let conn = get_connection()?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let existing_sha: Option<String> = conn
        .query_row(
            "SELECT credential_sha256 FROM accounts WHERE identity_key = ?1",
            params![identity.key()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let is_new = existing_sha.is_none();
    let mut credential_changed = is_new || existing_sha.as_deref() != Some(&new_sha);

    // Anti-downgrade guard: If profile already exists, do not overwrite if existing profile is newer
    if credential_changed && !is_new && auth_dst.is_file() {
        if let Ok((existing_auth, _, _)) = read_auth_file(&auth_dst) {
            if !is_auth_newer_or_equal(&auth, &existing_auth) {
                credential_changed = false;
            }
        }
    }

    let effective_sha = if !credential_changed && !is_new {
        existing_sha.clone().unwrap_or(new_sha.clone())
    } else {
        new_sha.clone()
    };

    if credential_changed {
        atomic_write(&auth_dst, &raw)?;
    }

    let _ = conn.execute(
        "DELETE FROM removed_accounts WHERE identity_key = ?1",
        params![identity.key()],
    );

    conn.execute(
        "INSERT INTO accounts (
             identity_key, profile_id, user_id, account_id, email, plan, org_title,
             first_seen_at, last_seen_at, last_credential_update,
             credential_sha256, credential_path, credential_status, last_error
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'active', NULL)
         ON CONFLICT(identity_key) DO UPDATE SET
             email = COALESCE(excluded.email, accounts.email),
             plan = COALESCE(excluded.plan, accounts.plan),
             org_title = COALESCE(excluded.org_title, accounts.org_title),
             last_seen_at = excluded.last_seen_at,
             last_credential_update = CASE
                 WHEN excluded.credential_sha256 <> accounts.credential_sha256
                 THEN excluded.last_credential_update
                 ELSE accounts.last_credential_update
             END,
             credential_sha256 = excluded.credential_sha256,
             credential_path = excluded.credential_path,
             credential_status = CASE
                 WHEN excluded.credential_sha256 <> accounts.credential_sha256
                 THEN 'active'
                 ELSE accounts.credential_status
             END,
             last_error = CASE
                 WHEN excluded.credential_sha256 <> accounts.credential_sha256
                 THEN NULL
                 ELSE accounts.last_error
             END",
        params![
            identity.key(),
            identity.profile_id(),
            identity.user_id,
            identity.account_id,
            identity.email,
            identity.plan,
            identity.org_title,
            now,
            now,
            now,
            effective_sha,
            auth_dst.to_string_lossy().to_string(),
        ],
    )
    .map_err(|e| format!("Failed to upsert account: {e}"))?;

    Ok((identity, is_new, credential_changed))
}

/// Ingests an `auth.json` file into SQLite and the profile sandbox.
///
/// # Returns
///
/// A tuple `(Identity, is_new, credential_changed)`.
///
/// # Errors
///
/// Returns `Err` if credentials cannot be parsed or written.
pub fn ingest_auth(source_path: &Path) -> Result<(Identity, bool, bool), String> {
    let raw = fs::read(source_path).map_err(|e| format!("Cannot read {}: {e}", source_path.display()))?;
    ingest_auth_raw(raw)
}

/// Checks the currently active `~/.codex/auth.json` and returns its identity key if valid.
#[must_use]
pub fn get_current_identity_key() -> Option<String> {
    let path = active_auth_path();
    if !path.is_file() {
        return None;
    }
    let (auth, _, _) = read_auth_file(&path).ok()?;
    extract_identity(&auth).ok().map(|i| i.key())
}

/// Automatically synchronizes the active `~/.codex/auth.json` into SQLite and profiles.
pub fn auto_sync_current() -> Option<(Identity, bool, bool)> {
    let path = active_auth_path();
    if !path.is_file() {
        return None;
    }
    ingest_auth(&path).ok()
}

/// Scans `~/.codex/backups/*/auth.json` and ingests newest backup files.
///
/// Discovers new accounts and updates existing accounts if backup credentials are newer.
pub fn auto_sync_backups() -> usize {
    let backup_dir = codex_home().join("backups");
    if !backup_dir.is_dir() {
        return 0;
    }

    let conn = match get_connection() {
        Ok(c) => c,
        Err(_) => return 0,
    };

    let mut removed = std::collections::HashSet::new();
    if let Ok(mut stmt) = conn.prepare("SELECT identity_key FROM removed_accounts") {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for r in rows.flatten() {
                removed.insert(r);
            }
        }
    }

    // Group candidates by identity_key, retaining the newest backup file
    let mut latest_candidates: std::collections::HashMap<
        String,
        (PathBuf, serde_json::Value, std::time::SystemTime),
    > = std::collections::HashMap::new();

    if let Ok(entries) = fs::read_dir(backup_dir) {
        for entry in entries.flatten() {
            let auth_file = entry.path().join("auth.json");
            if !auth_file.is_file() {
                continue;
            }
            let (auth, _, _) = match read_auth_file(&auth_file) {
                Ok(a) => a,
                Err(_) => continue,
            };
            let ident = match extract_identity(&auth) {
                Ok(i) => i,
                Err(_) => continue,
            };
            let key = ident.key();

            // Strictly ignore accounts that were explicitly removed by user in CodexQ
            if removed.contains(&key) {
                continue;
            }

            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            match latest_candidates.get(&key) {
                Some((_, prev_auth, prev_mtime)) => {
                    if is_auth_newer_or_equal(&auth, prev_auth)
                        && (mtime > *prev_mtime || &auth != prev_auth)
                    {
                        latest_candidates.insert(key, (auth_file, auth, mtime));
                    }
                }
                None => {
                    latest_candidates.insert(key, (auth_file, auth, mtime));
                }
            }
        }
    }

    let mut count = 0;
    for (_key, (auth_file, _, _)) in latest_candidates {
        if let Ok((_, is_new, changed)) = ingest_auth(&auth_file) {
            if is_new || changed {
                count += 1;
            }
        }
    }

    count
}

/// Re-computes SHA256 of `profile_dir/auth.json` and updates SQLite metadata.
///
/// Called after external operations (like `codex app-server` probing) refresh credentials in the sandbox.
///
/// # Arguments
///
/// * `identity_key` - Unique account identifier.
/// * `profile_dir` - Sandbox directory path.
///
/// # Returns
///
/// `Ok(true)` if metadata was updated, `Ok(false)` if unchanged or file missing.
///
/// # Errors
///
/// Returns `Err` if reading the auth file or database update fails.
pub fn sync_profile_auth_metadata(identity_key: &str, profile_dir: &Path) -> Result<bool, String> {
    let auth_path = profile_dir.join("auth.json");
    if !auth_path.is_file() {
        return Ok(false);
    }

    let (auth, new_sha, _) = read_auth_file(&auth_path)?;
    let ident = extract_identity(&auth)?;
    if ident.key() != identity_key {
        return Err("Profile auth identity mismatch".to_string());
    }

    let conn = get_connection()?;
    let old_sha: Option<String> = conn
        .query_row(
            "SELECT credential_sha256 FROM accounts WHERE identity_key = ?1",
            params![identity_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if old_sha.as_deref() == Some(&new_sha) {
        return Ok(false);
    }

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    conn.execute(
        "UPDATE accounts
         SET credential_sha256 = ?1,
             last_credential_update = ?2,
             last_seen_at = ?2,
             email = COALESCE(?3, email),
             plan = COALESCE(?4, plan),
             org_title = COALESCE(?5, org_title),
             credential_status = 'active',
             last_error = NULL
         WHERE identity_key = ?6",
        params![new_sha, now, ident.email, ident.plan, ident.org_title, identity_key],
    )
    .map_err(|e| format!("Failed to update profile auth metadata: {e}"))?;

    Ok(true)
}

/// Resolves an account from identifier, email, alias, or profile ID prefix.
pub fn resolve_account(target: &str) -> Result<Option<Identity>, String> {
    let target = target.trim();
    if target.is_empty() {
        return Ok(None);
    }
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT user_id, account_id, email, plan, org_title FROM accounts
             WHERE identity_key = ?1
                OR alias = ?1
                OR email = ?1
                OR user_id = ?1
                OR profile_id = ?1
                OR profile_id LIKE ?1 || '%'
             ORDER BY CASE
                 WHEN identity_key = ?1 THEN 0
                 WHEN alias = ?1 THEN 1
                 WHEN email = ?1 THEN 2
                 WHEN profile_id = ?1 THEN 3
                 ELSE 4
             END
             LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let found = stmt
        .query_row(params![target], |row| {
            Ok(Identity {
                user_id: row.get(0)?,
                account_id: row.get(1)?,
                email: row.get(2)?,
                plan: row.get(3)?,
                org_title: row.get(4)?,
            })
        })
        .optional()
        .map_err(|e| e.to_string())?;

    if found.is_some() {
        return Ok(found);
    }

    // Fallback: match by email username prefix
    let target_lower = target.to_lowercase();
    let mut stmt_all = conn
        .prepare("SELECT user_id, account_id, email, plan, org_title FROM accounts")
        .map_err(|e| e.to_string())?;

    let all: Vec<Identity> = stmt_all
        .query_map([], |row| {
            Ok(Identity {
                user_id: row.get(0)?,
                account_id: row.get(1)?,
                email: row.get(2)?,
                plan: row.get(3)?,
                org_title: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|id| {
            id.email
                .as_ref()
                .and_then(|e| e.split('@').next())
                .map(|p| p.to_lowercase() == target_lower)
                .unwrap_or(false)
        })
        .collect();

    if all.len() == 1 {
        return Ok(Some(all.into_iter().next().unwrap()));
    }

    Ok(None)
}

fn to_remaining(used: Option<f64>) -> Option<f64> {
    used.map(|u| (100.0 - u).clamp(0.0, 100.0))
}

/// Returns a complete list of accounts joined with their latest quota and status.
pub fn list_accounts_with_quota() -> Result<Vec<AccountData>, String> {
    let _ = auto_sync_backups();
    let _ = auto_sync_current();

    let current_key = get_current_identity_key();
    let conn = get_connection()?;

    let mut stmt = conn
        .prepare(
            "SELECT a.identity_key, a.profile_id, a.user_id, a.account_id, a.email, a.plan,
                    a.org_title, a.alias, a.credential_status, a.last_seen_at, a.reset_credits,
                    a.last_error,
                    q.primary_used_percent, q.primary_window_minutes, q.primary_resets_at,
                    q.secondary_used_percent, q.secondary_window_minutes, q.secondary_resets_at
             FROM accounts a
             LEFT JOIN quota_latest q
                ON q.identity_key = a.identity_key
               AND q.limit_id = COALESCE(
                   (
                     SELECT q2.limit_id
                     FROM quota_latest q2
                     WHERE q2.identity_key = a.identity_key
                     ORDER BY CASE WHEN q2.limit_id='codex' THEN 0 ELSE 1 END, q2.limit_id
                     LIMIT 1
                   ),
                   'codex'
               )
             ORDER BY COALESCE(a.alias, a.email, a.user_id)",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let identity_key: String = row.get(0)?;
            let profile_id: String = row.get(1)?;
            let user_id: String = row.get(2)?;
            let account_id: String = row.get(3)?;
            let email: Option<String> = row.get(4)?;
            let plan: Option<String> = row.get(5)?;
            let org_title: Option<String> = row.get(6)?;
            let alias: Option<String> = row.get(7)?;
            let credential_status: String = row.get(8)?;
            let last_seen_at: String = row.get(9)?;
            let reset_credits: Option<i64> = row.get(10)?;
            let last_error: Option<String> = row.get(11)?;

            let p_used: Option<f64> = row.get(12)?;
            let _p_win: Option<i64> = row.get(13)?;
            let p_resets: Option<i64> = row.get(14)?;

            let s_used: Option<f64> = row.get(15)?;
            let _s_win: Option<i64> = row.get(16)?;
            let s_resets: Option<i64> = row.get(17)?;

            let display_name = alias
                .clone()
                .or_else(|| email.clone())
                .unwrap_or_else(|| {
                    if user_id.len() > 18 {
                        format!("{}...{}", &user_id[..8], &user_id[user_id.len() - 6..])
                    } else {
                        user_id.clone()
                    }
                });

            let is_current = current_key.as_deref() == Some(&identity_key);

            Ok(AccountData {
                identity_key,
                profile_id,
                user_id,
                account_id,
                email,
                plan,
                org_title,
                alias,
                display_name,
                is_current,
                credential_status,
                last_seen_at,
                primary: RateLimitWindow {
                    label: "5H".to_string(),
                    used_percent: p_used,
                    remaining_percent: to_remaining(p_used),
                    resets_at: p_resets,
                },
                secondary: RateLimitWindow {
                    label: "7D".to_string(),
                    used_percent: s_used,
                    remaining_percent: to_remaining(s_used),
                    resets_at: s_resets,
                },
                reset_credits,
                last_error,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for r in rows {
        result.push(r.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Updates or clears an account alias.
pub fn set_alias(target: &str, alias: Option<&str>) -> Result<bool, String> {
    let ident = match resolve_account(target)? {
        Some(i) => i,
        None => return Ok(false),
    };
    let clean_alias = alias
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let conn = get_connection()?;
    let affected = conn
        .execute(
            "UPDATE accounts SET alias = ?1 WHERE identity_key = ?2",
            params![clean_alias, ident.key()],
        )
        .map_err(|e| e.to_string())?;

    Ok(affected > 0)
}

/// Resets all account aliases.
pub fn reset_all_aliases() -> Result<usize, String> {
    let conn = get_connection()?;
    let count = conn
        .execute("UPDATE accounts SET alias = NULL WHERE alias IS NOT NULL", [])
        .map_err(|e| e.to_string())?;
    Ok(count)
}

/// Moves an account to the recycle bin (soft delete).
pub fn remove_account(target: &str) -> Result<bool, String> {
    let ident = match resolve_account(target)? {
        Some(i) => i,
        None => return Ok(false),
    };

    let conn = get_connection()?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let row = conn
        .query_row(
            "SELECT email, plan, alias, user_id FROM accounts WHERE identity_key = ?1",
            params![ident.key()],
            |r| {
                let email: Option<String> = r.get(0)?;
                let plan: Option<String> = r.get(1)?;
                let alias: Option<String> = r.get(2)?;
                let uid: String = r.get(3)?;
                let display = alias.or(email.clone()).unwrap_or(uid);
                Ok((email, plan, display))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let (email, plan, display_name) = match row {
        Some(triple) => triple,
        None => return Ok(false),
    };

    conn.execute(
        "INSERT OR REPLACE INTO removed_accounts (
             identity_key, profile_id, email, user_id, plan, display_name, removed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            ident.key(),
            ident.profile_id(),
            email,
            ident.user_id,
            plan,
            display_name,
            now
        ],
    )
    .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM accounts WHERE identity_key = ?1", params![ident.key()])
        .map_err(|e| e.to_string())?;

    let p_dir = ident.profile_dir();
    if p_dir.is_dir() {
        let t_dir = trash_dir();
        let _ = fs::create_dir_all(&t_dir);
        let trash_target = t_dir.join(ident.profile_id());
        if trash_target.exists() {
            let _ = fs::remove_dir_all(&trash_target);
        }
        let _ = fs::rename(&p_dir, &trash_target);
    }

    Ok(true)
}

/// Lists accounts in the recycle bin.
pub fn list_trash() -> Result<Vec<TrashAccountData>, String> {
    let conn = get_connection()?;
    let mut stmt = conn
        .prepare("SELECT identity_key, profile_id, email, user_id, plan, display_name, removed_at FROM removed_accounts ORDER BY removed_at DESC")
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let identity_key: String = row.get(0)?;
            let profile_id: String = row.get(1)?;
            let email: Option<String> = row.get(2)?;
            let user_id: Option<String> = row.get(3)?;
            let plan: Option<String> = row.get(4)?;
            let display_name: String = row.get(5)?;
            let removed_at: String = row.get(6)?;

            let trash_auth = trash_dir().join(&profile_id).join("auth.json");
            let has_credentials = trash_auth.is_file();

            Ok(TrashAccountData {
                identity_key,
                profile_id,
                email,
                user_id,
                plan,
                display_name,
                removed_at,
                has_credentials,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r.map_err(|e| e.to_string())?);
    }
    Ok(list)
}

/// Restores an account from the recycle bin.
pub fn restore_account(target: &str) -> Result<String, String> {
    let conn = get_connection()?;
    let target = target.trim();

    let row = conn
        .query_row(
            "SELECT identity_key, profile_id, display_name FROM removed_accounts
             WHERE identity_key = ?1 OR profile_id = ?1 OR email = ?1 OR display_name = ?1
             LIMIT 1",
            params![target],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let (identity_key, profile_id, display_name) = match row {
        Some(triple) => triple,
        None => return Err(format!("Account '{target}' not found in recycle bin")),
    };

    let trash_profile = trash_dir().join(&profile_id);
    let trash_auth = trash_profile.join("auth.json");
    let dest_profile = profiles_dir().join(&profile_id);

    if trash_auth.is_file() {
        let _ = fs::create_dir_all(profiles_dir());
        if dest_profile.exists() {
            let _ = fs::remove_dir_all(&dest_profile);
        }
        fs::rename(&trash_profile, &dest_profile)
            .map_err(|e| format!("Failed to restore profile folder: {e}"))?;
        let _ = ingest_auth(&dest_profile.join("auth.json"))?;
    }

    let _ = conn.execute("DELETE FROM removed_accounts WHERE identity_key = ?1", params![identity_key]);

    Ok(format!("Restored '{display_name}' from recycle bin"))
}

/// Permanently removes an account from the recycle bin.
pub fn purge_trash(target: Option<&str>) -> Result<String, String> {
    let conn = get_connection()?;
    if let Some(t) = target.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let row = conn
            .query_row(
                "SELECT identity_key, profile_id, display_name FROM removed_accounts
                 WHERE identity_key = ?1 OR profile_id = ?1 OR email = ?1 OR display_name = ?1
                 LIMIT 1",
                params![t],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        if let Some((key, pid, display)) = row {
            let _ = conn.execute("DELETE FROM removed_accounts WHERE identity_key = ?1", params![key]);
            let dir = trash_dir().join(pid);
            let _ = fs::remove_dir_all(dir);
            return Ok(format!("Permanently purged '{display}' from recycle bin"));
        }
        return Err(format!("Account '{t}' not found in recycle bin"));
    }

    // Purge all
    let mut stmt = conn.prepare("SELECT profile_id FROM removed_accounts").map_err(|e| e.to_string())?;
    let pids: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    for pid in pids {
        let _ = fs::remove_dir_all(trash_dir().join(pid));
    }
    conn.execute("DELETE FROM removed_accounts", []).map_err(|e| e.to_string())?;

    Ok("Recycle bin completely purged".to_string())
}

/// Returns the quota snapshots history for an account.
pub fn get_history(target: &str, limit: u32) -> Result<Vec<SnapshotRecord>, String> {
    let ident = match resolve_account(target)? {
        Some(i) => i,
        None => return Err(format!("Account '{target}' not found")),
    };

    let conn = get_connection()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, identity_key, limit_id, observed_at,
                    primary_used_percent, primary_window_minutes, primary_resets_at,
                    secondary_used_percent, secondary_window_minutes, secondary_resets_at,
                    raw_json
             FROM quota_snapshots
             WHERE identity_key = ?1
             ORDER BY observed_at DESC
             LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![ident.key(), limit], |row| {
            Ok(SnapshotRecord {
                id: row.get(0)?,
                identity_key: row.get(1)?,
                limit_id: row.get(2)?,
                observed_at: row.get(3)?,
                primary_used_percent: row.get(4)?,
                primary_window_minutes: row.get(5)?,
                primary_resets_at: row.get(6)?,
                secondary_used_percent: row.get(7)?,
                secondary_window_minutes: row.get(8)?,
                secondary_resets_at: row.get(9)?,
                raw_json: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for r in rows {
        results.push(r.map_err(|e| e.to_string())?);
    }
    Ok(results)
}
