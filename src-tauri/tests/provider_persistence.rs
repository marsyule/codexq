//! Persistence round-trip for provider records and their per-model protocol overrides.
//!
//! Why this is an integration test rather than a unit test: `CODEXQ_HOME` is a
//! process-global environment variable, and `cargo test` runs unit tests as parallel
//! threads inside one process. Redirecting the data directory there would race every
//! other test that resolves a path. An integration test binary gets its own process, so
//! this file deliberately contains a single test that owns the environment.
//!
//! Every run points `CODEXQ_HOME` at a throwaway directory, so it can never read or write
//! the developer's real `~/.codexq` — and it never touches `~/.codex` at all, because
//! catalog generation (the only path that writes there) is not exercised. See AGENTS.md §4.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use app_lib::core::db::{get_provider, list_providers, upsert_provider};
use app_lib::core::provider::Provider;

/// Creates an isolated `CODEXQ_HOME` and returns its path.
fn sandbox_home() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("codexq-it-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create sandbox home");
    std::env::set_var("CODEXQ_HOME", &dir);
    dir
}

/// Writes a `providers` table in its pre-`model_wire_apis` shape, emulating an install
/// created by v1.0.2. This is what makes the migration ALTER path observable: a fresh
/// database would get the column from `CREATE TABLE` instead and never exercise it.
fn seed_legacy_database(home: &Path) {
    let conn = rusqlite::Connection::open(home.join("codexq.db")).expect("open legacy db");
    conn.execute_batch(
        "CREATE TABLE providers (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             base_url TEXT NOT NULL,
             wire_api TEXT NOT NULL DEFAULT 'responses',
             active_model TEXT NOT NULL,
             models_json TEXT NOT NULL DEFAULT '[]',
             context_window INTEGER DEFAULT 256000,
             model_context_windows TEXT DEFAULT '{}',
             reasoning_levels TEXT,
             model_reasoning_levels TEXT,
             notes TEXT,
             custom_config_toml TEXT,
             custom_auth_json TEXT,
             key_masked TEXT NOT NULL DEFAULT '',
             key_sha256 TEXT NOT NULL DEFAULT '',
             created_at TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );",
    )
    .expect("create legacy providers table");
}

/// Builds a provider record for the given overrides.
fn provider(name: &str, wire_api: &str, overrides: Option<HashMap<String, String>>) -> Provider {
    Provider {
        id: "opencode-go".to_string(),
        name: name.to_string(),
        base_url: "https://opencode.ai/zen/go/v1".to_string(),
        wire_api: wire_api.to_string(),
        gateway_enabled: true,
        active_model: "deepseek-v4.1-flash".to_string(),
        models: vec![
            "deepseek-v4.1-flash".to_string(),
            "gpt-5.6-luna".to_string(),
        ],
        context_window: Some(256_000),
        model_context_windows: None,
        reasoning_levels: None,
        model_reasoning_levels: None,
        model_wire_apis: overrides,
        notes: None,
        custom_config_toml: None,
        custom_auth_json: None,
        key_masked: String::new(),
        created_at: "2026-09-22T00:00:00Z".to_string(),
        updated_at: "2026-09-22T00:00:00Z".to_string(),
    }
}

#[test]
fn provider_per_model_protocol_overrides_survive_the_round_trip() {
    let home = sandbox_home();
    seed_legacy_database(&home);

    // 1. An existing-install upgrade must add the column and persist a mixed-protocol map.
    let overrides = HashMap::from([
        ("gpt-5.6-luna".to_string(), "responses".to_string()),
        ("deepseek-v4.1-flash".to_string(), "chat".to_string()),
    ]);
    let record = provider("opencode-go", "chat", Some(overrides));
    upsert_provider(&record, "sha-of-key").expect("upsert into a migrated legacy database");

    let loaded = get_provider("opencode-go")
        .expect("read provider")
        .expect("provider exists");
    let loaded_overrides = loaded
        .model_wire_apis
        .as_ref()
        .expect("model_wire_apis must survive persistence");
    assert_eq!(loaded_overrides.len(), 2);
    assert_eq!(
        loaded_overrides.get("gpt-5.6-luna").map(String::as_str),
        Some("responses")
    );
    assert_eq!(
        loaded_overrides.get("deepseek-v4.1-flash").map(String::as_str),
        Some("chat")
    );

    // The whole point: one provider, two protocols, resolved per model.
    assert_eq!(loaded.wire_api_for_model("gpt-5.6-luna"), "responses");
    assert_eq!(loaded.wire_api_for_model("deepseek-v4.1-flash"), "chat");
    // An unknown model inherits the provider default.
    assert_eq!(loaded.wire_api_for_model("unknown-model"), "chat");
    // A Responses-only sibling keeps the provider off the gateway.
    let mut responses_only = loaded.clone();
    responses_only.wire_api = "responses".to_string();
    responses_only.model_wire_apis =
        Some(HashMap::from([("gpt-5.6-luna".to_string(), "responses".to_string())]));
    assert!(!responses_only.needs_gateway());
    assert!(loaded.needs_gateway());

    // 2. Legacy values are normalized on write, never stored verbatim.
    let legacy = provider(
        "opencode-go",
        "chat_completions",
        Some(HashMap::from([(
            "gpt-5.6-luna".to_string(),
            "completions".to_string(),
        )])),
    );
    upsert_provider(&legacy, "").expect("upsert legacy values");
    let reloaded = get_provider("opencode-go")
        .expect("read provider")
        .expect("provider exists");
    assert_eq!(
        reloaded
            .model_wire_apis
            .as_ref()
            .and_then(|map| map.get("gpt-5.6-luna"))
            .map(String::as_str),
        Some("chat"),
        "a legacy `completions` override must be folded into the internal chat domain"
    );

    // 3. Clearing the overrides must actually clear the column, not leave it stale.
    let cleared = provider("opencode-go", "responses", None);
    upsert_provider(&cleared, "").expect("upsert without overrides");
    let after_clear = get_provider("opencode-go")
        .expect("read provider")
        .expect("provider exists");
    assert!(
        after_clear.model_wire_apis.is_none(),
        "removing every override must persist as NULL, not as a stale map"
    );
    assert!(!after_clear.needs_gateway());

    // 4. The column must survive the list path too, which reads it by a different index.
    let listed = list_providers().expect("list providers");
    let listed_record = listed
        .iter()
        .find(|p| p.id == "opencode-go")
        .expect("provider appears in the list");
    assert!(listed_record.model_wire_apis.is_none());

    let _ = std::fs::remove_dir_all(&home);
}
