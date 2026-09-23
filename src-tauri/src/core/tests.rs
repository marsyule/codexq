//! Unit tests for CodexQ Pure Rust Core Engine.

#[cfg(test)]
mod tests {
    use super::super::auth::*;
    use super::super::config::*;
    use super::super::db::*;
    use super::super::scheduler::*;

    #[test]
    fn test_identity_key_and_profile_id() {
        let ident = Identity {
            user_id: "user_abc123".to_string(),
            account_id: "acc_xyz789".to_string(),
            email: Some("developer@example.com".to_string()),
            plan: Some("plus".to_string()),
            org_title: None,
        };

        assert_eq!(ident.key(), "user_abc123\x1facc_xyz789");
        assert_eq!(ident.profile_id().len(), 20);
    }

    #[test]
    fn test_jwt_payload_decode() {
        // header: {"alg":"none"} -> eyJhbGciOiJub25lIn0
        // payload: {"email":"test@example.com","sub":"user_123","https://api.openai.com/auth":{"chatgpt_user_id":"user_123","chatgpt_account_id":"acc_456","chatgpt_plan_type":"pro"}}
        // -> base64
        let payload_json = serde_json::json!({
            "email": "test@example.com",
            "sub": "user_123",
            "https://api.openai.com/auth": {
                "chatgpt_user_id": "user_123",
                "chatgpt_account_id": "acc_456",
                "chatgpt_plan_type": "pro"
            }
        });
        let payload_bytes = serde_json::to_vec(&payload_json).unwrap();
        let payload_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            &payload_bytes,
        );
        let fake_jwt = format!("eyJhbGciOiJub25lIn0.{}.signature", payload_b64);

        let decoded = decode_jwt_payload(&fake_jwt).expect("Should decode JWT payload");
        assert_eq!(decoded["email"], "test@example.com");

        let auth_obj = serde_json::json!({
            "tokens": {
                "id_token": fake_jwt,
                "account_id": "acc_456"
            }
        });

        let ident = extract_identity(&auth_obj).expect("Should extract identity");
        assert_eq!(ident.user_id, "user_123");
        assert_eq!(ident.account_id, "acc_456");
        assert_eq!(ident.email.as_deref(), Some("test@example.com"));
        assert_eq!(ident.plan.as_deref(), Some("pro"));
    }

    #[test]
    fn test_validate_alarm_intervals() {
        let existing = vec![
            AccountAlarm {
                id: "alm_1".to_string(),
                identity_key: "k1".to_string(),
                time_of_day: "08:00".to_string(),
                days_of_week: "1,2,3,4,5".to_string(),
                enabled: true,
                model_override: None,
                prompt_override: None,
                last_triggered_at: None,
                last_status: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            },
            AccountAlarm {
                id: "alm_2".to_string(),
                identity_key: "k1".to_string(),
                time_of_day: "14:00".to_string(),
                days_of_week: "1,2,3,4,5".to_string(),
                enabled: true,
                model_override: None,
                prompt_override: None,
                last_triggered_at: None,
                last_status: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            },
        ];

        // 08:00 to 14:00 is 6 hours -> OK
        // Candidate 10:00 is 2 hours from 08:00 -> should conflict (< 5 hours = 300 min)
        let res_conflict = validate_alarm_intervals(&existing, "10:00", None, 300);
        assert!(res_conflict.is_err());
        assert!(res_conflict.unwrap_err().contains("必须 >= 5.0 小时"));

        // Candidate 20:00 is 6 hours from 14:00 and 12 hours from 08:00 -> OK
        let res_ok = validate_alarm_intervals(&existing, "20:00", None, 300);
        assert!(res_ok.is_ok());

        // Updating alm_1 with candidate 08:00 (excluded itself) -> OK
        let res_self = validate_alarm_intervals(&existing, "08:00", Some("alm_1"), 300);
        assert!(res_self.is_ok());
    }

    #[test]
    fn test_config_settings_roundtrip() {
        let all = get_all_settings();
        assert!(all.contains_key("general.locale"));
        assert!(all.contains_key("warmup.default_model"));
        assert!(all.contains_key("auto_refresh.interval_minutes"));
    }

    #[test]
    fn test_db_schema_in_memory() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE accounts (
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
             CREATE TABLE quota_latest (
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
             );"
        ).unwrap();

        conn.execute(
            "INSERT INTO accounts (
                 identity_key, profile_id, user_id, account_id, email, plan,
                 first_seen_at, last_seen_at, last_credential_update,
                 credential_sha256, credential_path
             ) VALUES ('u1\x1fa1', 'p1', 'u1', 'a1', 'u1@example.com', 'pro', 'now', 'now', 'now', 'sha', '/path')",
            [],
        ).unwrap();

        let count: i64 = conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_parse_bucket_formats() {
        use super::super::probe::parse_bucket;

        // 1. Top-level response from codex app-server
        let top_level_json = serde_json::json!({
            "limitId": "codex",
            "limitName": null,
            "primary": {
                "usedPercent": 5.0,
                "windowDurationMins": 300,
                "resetsAt": 1789311159
            },
            "secondary": {
                "usedPercent": 62.0,
                "windowDurationMins": 10080,
                "resetsAt": 1789805888
            },
            "credits": {
                "hasCredits": false,
                "balance": null
            },
            "planType": "team",
            "spendControlReached": false
        });

        let bucket = parse_bucket(&top_level_json, "fallback").expect("Should parse top-level bucket");
        assert_eq!(bucket.0, "codex");
        assert_eq!(bucket.1, Some(5.0));
        assert_eq!(bucket.2, Some(300));
        assert_eq!(bucket.3, Some(1789311159));
        assert_eq!(bucket.4, Some(62.0));
        assert_eq!(bucket.5, Some(10080));
        assert_eq!(bucket.6, Some(1789805888));

        // 2. Non-bucket fields should return None
        let not_a_bucket = serde_json::json!({
            "balance": null,
            "hasCredits": false,
            "unlimited": false
        });
        assert!(parse_bucket(&not_a_bucket, "fallback").is_none());
    }

    #[test]
    fn test_is_auth_newer_or_equal() {
        let older = serde_json::json!({
            "last_refresh": "2026-09-01T08:00:00Z"
        });
        let newer = serde_json::json!({
            "last_refresh": "2026-09-07T12:00:00Z"
        });

        assert!(is_auth_newer_or_equal(&newer, &older));
        assert!(!is_auth_newer_or_equal(&older, &newer));
        assert!(is_auth_newer_or_equal(&newer, &newer));
    }

    #[test]
    fn test_ensure_config_file_has_file_store() {
        let temp_dir = std::env::temp_dir().join(format!("codexq_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let config_file = temp_dir.join("test_config.toml");

        // 1. Initial write with other settings
        std::fs::write(&config_file, "model = \"gpt-5\"\n").unwrap();

        // 2. Ensure file store
        ensure_config_file_has_file_store(&config_file).unwrap();
        let content = std::fs::read_to_string(&config_file).unwrap();
        assert!(content.contains("model = \"gpt-5\""));
        assert!(content.contains("cli_auth_credentials_store = \"file\""));

        // 3. Idempotent check
        ensure_config_file_has_file_store(&config_file).unwrap();
        let content_after = std::fs::read_to_string(&config_file).unwrap();
        assert_eq!(content_after.matches("cli_auth_credentials_store").count(), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_is_access_token_expired() {
        let now = chrono::Utc::now().timestamp();

        // 1. Expired token (exp 1000s in the past)
        let expired_payload = serde_json::json!({
            "exp": now - 1000,
            "sub": "user_expired"
        });
        let expired_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            serde_json::to_vec(&expired_payload).unwrap(),
        );
        let expired_jwt = format!("eyJhbGciOiJub25lIn0.{expired_b64}.sig");
        let expired_auth = serde_json::json!({
            "tokens": {
                "access_token": expired_jwt
            }
        });
        assert!(is_access_token_expired(&expired_auth, 300));

        // 2. Valid token (exp 100,000s in the future)
        let valid_payload = serde_json::json!({
            "exp": now + 100_000,
            "sub": "user_valid"
        });
        let valid_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            serde_json::to_vec(&valid_payload).unwrap(),
        );
        let valid_jwt = format!("eyJhbGciOiJub25lIn0.{valid_b64}.sig");
        let valid_auth = serde_json::json!({
            "tokens": {
                "access_token": valid_jwt
            }
        });
        assert!(!is_access_token_expired(&valid_auth, 300));

        // 3. Expiring within buffer (exp 200s in the future, buffer is 300s)
        let buffer_payload = serde_json::json!({
            "exp": now + 200,
            "sub": "user_buffer"
        });
        let buffer_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            serde_json::to_vec(&buffer_payload).unwrap(),
        );
        let buffer_jwt = format!("eyJhbGciOiJub25lIn0.{buffer_b64}.sig");
        let buffer_auth = serde_json::json!({
            "tokens": {
                "access_token": buffer_jwt
            }
        });
        assert!(is_access_token_expired(&buffer_auth, 300));

        // 4. Non-OAuth config (no tokens key)
        let api_key_auth = serde_json::json!({
            "auth_mode": "api_key",
            "OPENAI_API_KEY": "sk-123456"
        });
        assert!(!is_access_token_expired(&api_key_auth, 300));
    }

    #[test]
    fn test_auto_rollover_settings_and_conditions() {
        use crate::core::config::{get_account_rollover, save_account_rollover};

        // Initially unconfigured account
        let initial = get_account_rollover("test_user\x1ftest_acc");
        assert!(!initial.enabled);
        assert_eq!(initial.min_weekly_remaining, 0.0);

        // Save account-level rollover settings
        assert!(save_account_rollover("test_user\x1ftest_acc", true, 20.0).is_ok());

        let updated = get_account_rollover("test_user\x1ftest_acc");
        assert!(updated.enabled);
        assert_eq!(updated.min_weekly_remaining, 20.0);

        // Clamping check (>100 or <0)
        assert!(save_account_rollover("test_user\x1ftest_acc", true, 150.0).is_ok());
        assert_eq!(get_account_rollover("test_user\x1ftest_acc").min_weekly_remaining, 100.0);

        // Reset
        let _ = save_account_rollover("test_user\x1ftest_acc", false, 0.0);
    }

    #[test]
    fn test_codexq_home_resolution() {
        use crate::core::paths::codexq_home;

        // With environment override
        unsafe {
            std::env::set_var("CODEXQ_HOME", "target/test_codexq_home");
        }
        assert_eq!(codexq_home(), std::path::PathBuf::from("target/test_codexq_home"));
        unsafe {
            std::env::remove_var("CODEXQ_HOME");
        }

        // Without override, it points to a valid path
        let default_home = codexq_home();
        assert!(!default_home.as_os_str().is_empty());
    }

    #[test]
    fn test_is_portable_detection() {
        use crate::core::paths::check_is_portable_for_path;
        use std::path::Path;

        // 1. Filename contains "portable" (case-insensitive)
        assert!(check_is_portable_for_path(Path::new("C:/Apps/CodexQ-v1.0.1-portable.exe")));
        assert!(check_is_portable_for_path(Path::new("C:/Apps/codexq_PORTABLE.exe")));
        assert!(!check_is_portable_for_path(Path::new("C:/Program Files/CodexQ/CodexQ.exe")));

        // 2. Directory contains "portable" marker file
        let temp_dir = std::env::temp_dir().join(format!("codexq_test_portable_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let marker = temp_dir.join("portable");
        let exe_in_temp = temp_dir.join("CodexQ.exe");

        assert!(!check_is_portable_for_path(&exe_in_temp));
        let _ = std::fs::write(&marker, "portable");
        assert!(check_is_portable_for_path(&exe_in_temp));

        // Cleanup
        let _ = std::fs::remove_file(&marker);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_provider_key_masking_and_catalog() {
        use crate::core::provider::*;

        assert_eq!(mask_api_key("sk-1234567890abcdef"), "sk-12****cdef");
        assert_eq!(mask_api_key("short"), "****");
        assert_eq!(mask_api_key(""), "");

        let hash1 = hash_api_key("sk-test-key-1");
        let hash2 = hash_api_key("sk-test-key-1");
        let hash3 = hash_api_key("sk-test-key-2");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);

        let mut model_contexts = std::collections::HashMap::new();
        model_contexts.insert("deepseek-v4-flash".to_string(), 1_000_000);

        // Build a full Codex model catalog conforming to Codex Desktop & CLI.
        // `build_model_catalog` is pure, so this test never touches the real ~/.codex.
        let cat_content = build_model_catalog(
            "deepseek-v4-flash",
            &["step-5-preview".to_string(), "deepseek-v4-flash".to_string()],
            Some(512_000),
            Some(&model_contexts),
            None,
            None,
        );
        assert_eq!(cat_content.models.len(), 2);
        assert_eq!(cat_content.models[0].slug, "deepseek-v4-flash");
        assert_eq!(cat_content.models[0].visibility, "list");
        assert_eq!(cat_content.models[0].priority, 1000);
        assert_eq!(cat_content.models[0].context_window, 1_000_000);
        assert_eq!(cat_content.models[0].effective_context_window_percent, 95);
        // Auto-compaction triggers at 85% of the working window.
        assert_eq!(cat_content.models[0].auto_compact_token_limit, Some(850_000));
        assert_eq!(cat_content.models[1].slug, "step-5-preview");
        assert_eq!(cat_content.models[1].visibility, "list");
        assert_eq!(cat_content.models[1].priority, 1001);
        assert_eq!(cat_content.models[1].context_window, 512_000);
        assert_eq!(cat_content.models[1].effective_context_window_percent, 95);
        assert_eq!(cat_content.models[1].auto_compact_token_limit, Some(435_200));
    }

    #[test]
    fn test_lossless_toml_editing_preserves_comments_and_mcp() {
        use toml_edit::{DocumentMut, Item, Table, Value};

        let initial_toml = r#"# User custom comment at top
cli_auth_credentials_store = "file"

[mcp_servers.my_tool]
command = "node"
args = ["server.js"]

# Another important user comment
[custom_section]
feature_flag = true
"#;

        let mut doc: DocumentMut = initial_toml.parse().expect("Parse initial TOML");

        // 1. Inject third-party provider
        doc["model"] = Item::Value(Value::from("step-5-preview"));
        doc["model_provider"] = Item::Value(Value::from("stepfun"));
        doc["model_catalog_json"] = Item::Value(Value::from("/path/to/models.json"));

        if !doc.contains_key("model_providers") {
            doc["model_providers"] = Item::Table(Table::new());
        }
        let mut p_table = Table::new();
        p_table["name"] = Item::Value(Value::from("StepFun"));
        p_table["base_url"] = Item::Value(Value::from("https://api.stepfun.com/v1"));
        p_table["wire_api"] = Item::Value(Value::from("responses"));
        p_table["experimental_bearer_token"] = Item::Value(Value::from("sk-secret"));

        if let Some(mp) = doc.get_mut("model_providers").and_then(|i| i.as_table_like_mut()) {
            mp.insert("stepfun", Item::Table(p_table));
        }

        let modified_toml = doc.to_string();

        // Verify that original comments, MCP settings, and flags are 100% preserved
        assert!(modified_toml.contains("# User custom comment at top"));
        assert!(modified_toml.contains("[mcp_servers.my_tool]"));
        assert!(modified_toml.contains("# Another important user comment"));
        assert!(modified_toml.contains("feature_flag = true"));
        assert!(modified_toml.contains("model_provider = \"stepfun\""));
        assert!(modified_toml.contains("[model_providers.stepfun]"));

        // 2. Now switch back to official mode: remove model_provider and model_catalog_json
        doc.remove("model_provider");
        doc.remove("model_catalog_json");
        doc.remove("model");

        let restored_toml = doc.to_string();
        assert!(!restored_toml.contains("model_provider ="));
        assert!(!restored_toml.contains("model_catalog_json ="));
        // Original comments and MCP server must STILL be intact!
        assert!(restored_toml.contains("# User custom comment at top"));
        assert!(restored_toml.contains("[mcp_servers.my_tool]"));
        assert!(restored_toml.contains("feature_flag = true"));
    }

    #[test]
    fn test_apply_provider_routing_rewrites_only_the_target_table() {
        use crate::core::switch::apply_provider_routing;
        use toml_edit::DocumentMut;

        // A config where one provider was previously left on a Chat Completions address,
        // as written by a pre-gateway CodexQ version.
        let mut doc: DocumentMut = r#"# keep this comment
model = "deepseek-v4.1-flash"
model_provider = "opencode-go"

[model_providers.opencode-go]
name = "opencode-go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "chat"

[model_providers.untouched]
name = "Untouched"
base_url = "https://api.stepfun.com/step_plan/v1"
wire_api = "responses"
"#
        .parse()
        .expect("parse config");

        apply_provider_routing(&mut doc, "opencode-go", "http://127.0.0.1:17871/v1");
        let rendered = doc.to_string();

        assert!(rendered.contains(r#"base_url = "http://127.0.0.1:17871/v1""#));
        // The provider was re-pointed, so its `wire_api` must become Codex-legal.
        assert!(!rendered.contains(r#"wire_api = "chat""#));
        // Only the target table may change; a sibling provider keeps its direct address.
        assert!(rendered.contains(r#"base_url = "https://api.stepfun.com/step_plan/v1""#));
        assert!(rendered.contains("# keep this comment"));
        assert!(rendered.contains("[model_providers.untouched]"));

        // An unknown provider id must be a no-op rather than adding a phantom table.
        apply_provider_routing(&mut doc, "does-not-exist", "http://127.0.0.1:9999/v1");
        let after_missing = doc.to_string();
        assert!(!after_missing.contains("does-not-exist"));
        assert!(!after_missing.contains("127.0.0.1:9999"));

        // The inline-table form Codex also accepts must be handled.
        let mut inline: DocumentMut = r#"
[model_providers]
opencode-go = { name = "opencode-go", base_url = "https://opencode.ai/zen/go/v1", wire_api = "chat" }
"#
        .parse()
        .expect("parse inline config");
        apply_provider_routing(&mut inline, "opencode-go", "http://127.0.0.1:17871/v1");
        let inline_rendered = inline.to_string();
        assert!(inline_rendered.contains("http://127.0.0.1:17871/v1"));
        assert!(!inline_rendered.contains(r#"wire_api = "chat""#));
    }

    #[test]
    fn test_inactive_provider_keeps_session_id_without_credentials() {
        use crate::core::switch::deactivate_provider_entry;
        use toml_edit::DocumentMut;

        let mut doc: DocumentMut = r#"
model_provider = "opencode-go"
model = "grok-4.1"

[model_providers.opencode-go]
name = "OpenCode Go"
base_url = "https://opencode.ai/zen/go/v1"
wire_api = "responses"
experimental_bearer_token = "secret-key"
custom_secret = "also-secret"

[model_providers.untouched]
name = "Untouched"
base_url = "https://example.com/v1"
wire_api = "responses"
"#
        .parse()
        .expect("parse config");

        assert!(deactivate_provider_entry(
            &mut doc,
            "opencode-go",
            Some("OpenCode Go"),
        ));

        let provider = &doc["model_providers"]["opencode-go"];
        assert_eq!(provider["name"].as_str(), Some("OpenCode Go"));
        assert_eq!(provider["base_url"].as_str(), Some("http://127.0.0.1:1/v1"));
        assert_eq!(provider["wire_api"].as_str(), Some("responses"));
        assert!(provider.get("experimental_bearer_token").is_none());
        assert!(provider.get("custom_secret").is_none());
        assert_eq!(
            doc["model_providers"]["untouched"]["base_url"].as_str(),
            Some("https://example.com/v1")
        );
        assert!(doc["model_providers"].get("missing").is_none());
        assert!(deactivate_provider_entry(&mut doc, "stepfun", Some("StepFun")));
        assert_eq!(
            doc["model_providers"]["stepfun"]["base_url"].as_str(),
            Some("http://127.0.0.1:1/v1")
        );

        assert!(deactivate_provider_entry(&mut doc, "opencode-go", None));
        assert!(doc["model_providers"].get("opencode-go").is_none());
    }

    #[test]
    fn test_model_catalog_default_and_minimum_clamp() {
        use crate::core::provider::*;

        // 1. Without context window provided, default to 256k minimum (256,000 tokens)
        let cat_content =
            build_model_catalog("default-model", &["default-model".to_string()], None, None, None, None);
        assert_eq!(cat_content.models[0].context_window, 256_000);
        assert_eq!(cat_content.models[0].effective_context_window_percent, 95);
        assert_eq!(cat_content.models[0].auto_compact_token_limit, Some(217_600));

        // 2. If configured below 256k (e.g. 128k), clamp to 256k minimum
        let cat_content2 =
            build_model_catalog("small-model", &["small-model".to_string()], Some(128_000), None, None, None);
        assert_eq!(cat_content2.models[0].context_window, 256_000);
        assert_eq!(cat_content2.models[0].effective_context_window_percent, 95);
        assert_eq!(cat_content2.models[0].auto_compact_token_limit, Some(217_600));
    }

    #[test]
    fn test_model_catalog_reasoning_levels() {
        use crate::core::provider::*;
        use std::collections::HashMap;

        // Case 1: Default baseline reasoning levels: low, medium, high
        let cat_default = build_model_catalog(
            "model-std",
            &["model-std".to_string()],
            None,
            None,
            None,
            None,
        );
        let std_efforts: Vec<String> = cat_default.models[0]
            .supported_reasoning_levels
            .iter()
            .map(|l| l.effort.clone())
            .collect();
        assert_eq!(std_efforts, vec!["low", "medium", "high"]);
        assert_eq!(cat_default.models[0].default_reasoning_level, "medium");

        // Case 2: Provider-level extended with max and xhigh (intentionally out-of-order input)
        let provider_levels = vec!["max".to_string(), "low".to_string(), "xhigh".to_string(), "medium".to_string(), "high".to_string()];
        let cat_ext = build_model_catalog(
            "model-ext",
            &["model-ext".to_string()],
            None,
            None,
            Some(&provider_levels),
            None,
        );
        let ext_efforts: Vec<String> = cat_ext.models[0]
            .supported_reasoning_levels
            .iter()
            .map(|l| l.effort.clone())
            .collect();
        // Must be sorted in canonical effort progression: low < medium < high < xhigh < max
        assert_eq!(ext_efforts, vec!["low", "medium", "high", "xhigh", "max"]);
        let max_tier = cat_ext.models[0]
            .supported_reasoning_levels
            .iter()
            .find(|l| l.effort == "max")
            .unwrap();
        assert_eq!(max_tier.description, "Maximum reasoning depth for the hardest problems");

        // Case 3: Per-model override with max (e.g. mimo-v2.6-pro has max, others have baseline)
        let mut model_reasoning = HashMap::new();
        model_reasoning.insert(
            "mimo-v2.6-pro".to_string(),
            vec!["low".to_string(), "medium".to_string(), "high".to_string(), "max".to_string()],
        );
        let cat_override = build_model_catalog(
            "mimo-v2.6-pro",
            &["mimo-v2.6-pro".to_string(), "mimo-v2.6-flash".to_string()],
            None,
            None,
            None,
            Some(&model_reasoning),
        );
        let pro_efforts: Vec<String> = cat_override.models[0]
            .supported_reasoning_levels
            .iter()
            .map(|l| l.effort.clone())
            .collect();
        assert_eq!(pro_efforts, vec!["low", "medium", "high", "max"]);

        let flash_efforts: Vec<String> = cat_override.models[1]
            .supported_reasoning_levels
            .iter()
            .map(|l| l.effort.clone())
            .collect();
        assert_eq!(flash_efforts, vec!["low", "medium", "high"]);
    }

    #[test]
    fn test_gateway_port_resolution_precedence() {
        use crate::core::config::AppConfig;
        use crate::core::protocol_proxy::{resolve_port_with_source, PortSource, DEFAULT_PROXY_PORT};

        let mut cfg = AppConfig::default();

        // Stored 0 means "unset" and must fall back to the compiled-in default.
        cfg.proxy.port = 0;
        assert_eq!(
            resolve_port_with_source(&cfg, None),
            (DEFAULT_PROXY_PORT, PortSource::Default)
        );

        // An explicit config value beats the default...
        cfg.proxy.port = 18888;
        assert_eq!(
            resolve_port_with_source(&cfg, None),
            (18888, PortSource::Config)
        );

        // ...the environment override beats the config (trimmed)...
        assert_eq!(
            resolve_port_with_source(&cfg, Some(" 19999 ")),
            (19999, PortSource::Env)
        );

        // ...and an unusable override falls through instead of pinning port 0.
        assert_eq!(
            resolve_port_with_source(&cfg, Some("0")),
            (18888, PortSource::Config)
        );
        assert_eq!(
            resolve_port_with_source(&cfg, Some("not-a-port")),
            (18888, PortSource::Config)
        );
    }

    #[test]
    fn test_probe_port_reports_occupied_then_free() {
        use crate::core::protocol_proxy::{probe_port, PortState};

        // Hold an OS-assigned port so the probe has to report it as occupied. Binding must
        // fail for a second bind on this platform, otherwise the gateway's own port conflict
        // detection (and this probe) would be meaningless.
        let holder = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("bind ephemeral port");
        let held = holder.local_addr().expect("local addr").port();
        assert_eq!(probe_port(held), PortState::Occupied);

        // Released again: the probe reports the port as bindable.
        drop(holder);
        assert_eq!(probe_port(held), PortState::Free);

        // Port 0 is never a real listening port.
        assert_eq!(probe_port(0), PortState::Free);
    }
    #[test]
    fn test_codexq_model_catalog_path_detection() {
        assert!(crate::core::switch::is_codexq_model_catalog_path(
            r"C:\Users\me\.codex\model-catalogs\codexq-stepfun.json"
        ));
        assert!(crate::core::switch::is_codexq_model_catalog_path(
            "model-catalogs/codexq-newapi.json"
        ));
        assert!(crate::core::switch::is_codexq_model_catalog_path(
            "/home/me/.codexq/providers/newapi/models.json"
        ));
        assert!(!crate::core::switch::is_codexq_model_catalog_path(
            "/home/me/models/custom.json"
        ));
    }
}


