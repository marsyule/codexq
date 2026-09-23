//! Loopback-only protocol gateway that adapts Chat Completions upstreams to the Responses API.
//!
//! Codex CLI removed the `chat/completions` wire protocol in February 2026
//! (openai/codex discussion #7782). Any `wire_api` value other than `"responses"`
//! makes Codex reject the **whole** `config.toml` at load, which disables every
//! command — including `codex exec` and `codex login status` — while `codex doctor`
//! still runs and reports `config.load: fail`.
//!
//! CodexQ therefore never leaks an upstream protocol into Codex's config. When a
//! provider only speaks Chat Completions, the provider `base_url` is rewritten to
//! this gateway and Codex keeps using the standard Responses API; the gateway
//! translates in both directions, including streamed SSE, tool calls and reasoning
//! metadata.
//!
//! Design constraints (see `AGENTS.md` §3 invariant 5 and §4):
//! - In-process Tokio service. Never shells out to an external proxy or sidecar.
//! - Binds `127.0.0.1` only, and re-checks the peer address per request.
//! - Never caches or persists request/response bodies; logs carry no payload.
//! - A bind failure does NOT fall back to a half-configured state: callers must
//!   keep the upstream direct connection instead of writing an unreachable gateway URL.

pub mod route;
pub mod server;
pub mod stream;
pub mod translate;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use super::config::AppConfig;

/// Default loopback port for the protocol gateway.
pub const DEFAULT_PROXY_PORT: u16 = 17871;

/// Environment override for the gateway port; wins over `config.json`.
pub const PROXY_PORT_ENV: &str = "CODEXQ_PROXY_PORT";

/// The only `wire_api` value Codex still accepts.
pub const CODEX_WIRE_API: &str = "responses";

/// Internal marker for "upstream only speaks Chat Completions".
pub const CHAT_WIRE_API: &str = "chat";

/// Provider ids Codex reserves for its built-in providers.
///
/// Codex ≥0.148 rejects the **entire** config at load when one of these ids is
/// overridden by a `[model_providers.<id>]` table. The upstream match is exact and
/// case-sensitive, and the bedrock ids are exempt — mirror that behaviour here.
pub const CODEX_RESERVED_PROVIDER_IDS: &[&str] = &["openai", "ollama", "lmstudio"];

/// Running gateway handle.
struct GatewayRuntime {
    port: u16,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

static RUNTIME: Mutex<Option<GatewayRuntime>> = Mutex::new(None);
static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Snapshot of the gateway lifecycle state exposed to the UI and Tauri commands.
///
/// `port` is the **effective** port (env > `config.json` > default) and is what a loopback
/// address is built from; `listen_port` is the port actually bound by this process and is
/// `None` while the listener is stopped. Keeping both lets the UI stop claiming that a
/// loopback address is live just because it is configured.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GatewayStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub enabled: bool,
    /// Port bound by this process, or `None` when the listener is stopped.
    pub listen_port: Option<u16>,
    /// Port stored in `~/.codexq/config.json`, ignoring the environment override.
    pub config_port: u16,
    /// Where the effective port came from: `"env"` | `"config"` | `"default"`.
    pub port_source: &'static str,
    /// Whether `CODEXQ_PROXY_PORT` currently overrides `config.json`.
    pub env_override: bool,
    /// Port availability: `"running"` | `"free"` | `"occupied"`.
    pub port_state: &'static str,
}

/// Which source supplied the effective gateway port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortSource {
    /// `CODEXQ_PROXY_PORT` — wins over everything else.
    Env,
    /// `~/.codexq/config.json` → `proxy.port`.
    Config,
    /// [`DEFAULT_PROXY_PORT`].
    Default,
}

impl PortSource {
    /// Returns the stable identifier shared with the TypeScript layer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PortSource::Env => "env",
            PortSource::Config => "config",
            PortSource::Default => "default",
        }
    }
}

/// Availability of one loopback port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortState {
    /// This process owns the listener.
    Running,
    /// Nothing is listening; the port can be bound.
    Free,
    /// Binding failed — another process (possibly a stale CodexQ instance) holds it.
    Occupied,
}

impl PortState {
    /// Returns the stable identifier shared with the TypeScript layer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PortState::Running => "running",
            PortState::Free => "free",
            PortState::Occupied => "occupied",
        }
    }
}

/// Normalizes a stored `wire_api` value into CodexQ's internal two-value domain.
///
/// Legacy `chat` / `chat_completions` / `chat-completions` / `completions` all fold
/// into [`CHAT_WIRE_API`]; anything unrecognised falls back to [`CODEX_WIRE_API`],
/// because writing an unknown value into Codex's config is strictly worse than
/// silently using the standard protocol.
#[must_use]
pub fn normalize_wire_api(value: &str) -> String {
    let cleaned = value.trim().to_ascii_lowercase();
    match cleaned.as_str() {
        "chat" | "chat_completions" | "chat-completions" | "completions" | "completion" => {
            CHAT_WIRE_API.to_string()
        }
        _ => CODEX_WIRE_API.to_string(),
    }
}

/// Returns whether the upstream needs the gateway (i.e. it is not a Responses endpoint).
#[must_use]
pub fn needs_gateway(wire_api: &str) -> bool {
    normalize_wire_api(wire_api) == CHAT_WIRE_API
}

/// Resolves the upstream wire protocol for one model slug.
///
/// Protocol is a **per-model** property, not only a per-provider one: a single upstream
/// can expose both protocols simultaneously. `opencode-go` is the motivating case — it
/// serves Grok / GPT / Muse on `/v1/responses` while GLM / Kimi / DeepSeek / MiMo only
/// exist on `/v1/chat/completions`, all behind one `base_url` and one API key.
///
/// The provider-level `wire_api` is the default; a per-model entry overrides it. Lookup is
/// by trimmed exact slug first, then case-insensitively, because slugs are case-sensitive
/// on the wire but hand-typed configuration is not.
///
/// # Arguments
///
/// * `gateway_enabled` - Provider-level local gateway switch.
/// * `provider_default` - Provider-level `wire_api` value.
/// * `model_overrides` - Optional per-model `wire_api` overrides.
/// * `model` - Model slug from the incoming request; empty falls back to the default.
#[must_use]
pub fn resolve_model_wire_api(
    provider_default: &str,
    model_overrides: Option<&HashMap<String, String>>,
    model: &str,
) -> String {
    let slug = model.trim();
    if !slug.is_empty() {
        if let Some(overrides) = model_overrides {
            if let Some(value) = overrides.get(slug) {
                return normalize_wire_api(value);
            }
            if let Some((_, value)) = overrides
                .iter()
                .find(|(key, _)| key.trim().eq_ignore_ascii_case(slug))
            {
                return normalize_wire_api(value);
            }
        }
    }
    normalize_wire_api(provider_default)
}

/// Returns whether a provider needs the gateway for **at least one** of its models.
///
/// Once any model speaks Chat Completions the whole provider is routed through the
/// gateway, because Codex's `config.toml` holds a single `base_url` per provider table.
/// Models that speak native Responses are not harmed: the gateway passes them through
/// untouched. Deciding this per provider (rather than per model) is also what makes
/// in-session model switching work — Codex lets the user change models without rewriting
/// `config.toml`, so the protocol must be resolved per request instead.
///
/// # Arguments
///
/// * `gateway_enabled` - Provider-level local gateway switch.
/// * `provider_default` - Provider-level `wire_api` value.
/// * `model_overrides` - Optional per-model `wire_api` overrides.
#[must_use]
pub fn provider_needs_gateway(
    gateway_enabled: bool,
    provider_default: &str,
    model_overrides: Option<&HashMap<String, String>>,
) -> bool {
    if !gateway_enabled {
        return false;
    }
    needs_gateway(provider_default)
        || model_overrides.is_some_and(|overrides| {
            overrides.values().any(|value| needs_gateway(value))
        })
}

/// Resolves the `base_url` Codex should be configured with for a provider.
///
/// Starts the loopback gateway when the provider serves at least one Chat Completions
/// model, and returns the direct upstream address otherwise. This is the single decision
/// point shared by the slot-switch path and the "edit the active provider" path, so the two
/// can never disagree about whether a provider is routed through the gateway.
///
/// # Arguments
///
/// * `provider` - Provider record whose protocol configuration is being applied.
///
/// # Returns
///
/// A tuple of `(base_url, warning)`. The address is always safe to write: when the gateway
/// is required but cannot be started, the direct upstream address is returned instead of an
/// unreachable loopback URL, and `warning` carries the reason so the caller can surface it.
pub async fn routed_base_url(
    provider: &super::provider::Provider,
) -> (String, Option<String>) {
    let direct = provider.base_url.trim().to_string();
    if !provider.needs_gateway() {
        // A direct provider must not leave a previously started listener behind.
        stop();
        return (direct, None);
    }
    match ensure_started().await {
        Ok(base_url) => (base_url, None),
        Err(err) => (direct, Some(err)),
    }
}

/// Returns whether `id` collides with a Codex-reserved built-in provider id.
///
/// CodexQ slugifies provider ids to lowercase, so `Ollama` becomes `ollama` and
/// would corrupt the user's whole Codex config if written verbatim.
#[must_use]
pub fn is_reserved_provider_id(id: &str) -> bool {
    let trimmed = id.trim();
    CODEX_RESERVED_PROVIDER_IDS.iter().any(|reserved| *reserved == trimmed)
}

/// Returns the loopback `base_url` Codex should be pointed at for a given port.
#[must_use]
pub fn local_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

/// Returns `true` when `url` already points at a local protocol gateway.
///
/// Used to avoid writing a gateway that forwards to itself.
#[must_use]
pub fn is_local_gateway_url(url: &str) -> bool {
    let lowered = url.trim().to_ascii_lowercase();
    lowered.starts_with("http://127.0.0.1:") || lowered.starts_with("http://localhost:")
}

/// Reads the `CODEXQ_PROXY_PORT` override as a raw string, if it is set.
fn env_port_override() -> Option<String> {
    std::env::var(PROXY_PORT_ENV).ok()
}

/// Resolves the effective gateway port and reports which source supplied it.
///
/// The environment override is injected rather than read here so the precedence rule stays
/// testable without mutating process-wide environment variables, which would race with the other
/// parallel unit tests.
#[must_use]
pub fn resolve_port_with_source(cfg: &AppConfig, env_override: Option<&str>) -> (u16, PortSource) {
    if let Some(raw) = env_override {
        if let Ok(port) = raw.trim().parse::<u16>() {
            if port > 0 {
                return (port, PortSource::Env);
            }
        }
    }
    if cfg.proxy.port > 0 {
        (cfg.proxy.port, PortSource::Config)
    } else {
        (DEFAULT_PROXY_PORT, PortSource::Default)
    }
}

/// Resolves the effective gateway port: env override, then `config.json`, then default.
#[must_use]
pub fn resolve_port(cfg: &AppConfig) -> u16 {
    resolve_port_with_source(cfg, env_port_override().as_deref()).0
}

/// Probes whether `port` can be bound on the loopback interface.
///
/// Deliberately does **not** identify the holder: a stale CodexQ process and an unrelated program
/// look identical without an HTTP round-trip, and making the synchronous [`status`] async only
/// buys a slightly nicer message.
#[must_use]
pub fn probe_port(port: u16) -> PortState {
    if port == 0 {
        return PortState::Free;
    }
    {
        let guard = lock_runtime();
        if guard.as_ref().is_some_and(|runtime| runtime.port == port) {
            return PortState::Running;
        }
    }
    match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)) {
        // Binding *is* the probe; release it immediately so the real listener can take over.
        Ok(listener) => {
            drop(listener);
            PortState::Free
        }
        Err(_) => PortState::Occupied,
    }
}

/// Generates a Responses-style `resp_*` id for a translated completion.
///
/// Uniqueness only has to hold within a process lifetime: Codex treats the id as an
/// opaque handle for the duration of one turn.
#[must_use]
pub fn new_response_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "resp_{:x}{:x}{:x}",
        chrono::Utc::now().timestamp_millis(),
        std::process::id(),
        seq
    )
}

/// Returns a shared HTTP client for upstream traffic.
///
/// No global request timeout is configured: Responses streams may legitimately stay
/// open for minutes. Only connection establishment is bounded.
///
/// # Errors
///
/// Returns `Err` if the TLS/connection backend cannot be initialized.
pub fn upstream_client() -> Result<reqwest::Client, String> {
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to build upstream HTTP client: {e}"))?;
    let _ = CLIENT.set(client.clone());
    Ok(client)
}

/// Starts the gateway if it is not already running and returns its base URL.
///
/// # Errors
///
/// Returns `Err` if the gateway is disabled in settings, or if the loopback port
/// cannot be bound. Callers MUST treat an error as "do not take over routing" and
/// keep the provider connected directly.
pub async fn ensure_started() -> Result<String, String> {
    {
        let guard = lock_runtime();
        if let Some(runtime) = guard.as_ref() {
            return Ok(local_base_url(runtime.port));
        }
    }

    let cfg = super::config::load_config();
    if !cfg.proxy.enabled {
        return Err("Local protocol gateway is disabled in CodexQ settings.".to_string());
    }
    let port = resolve_port(&cfg);

    let listener = match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
    {
        Ok(listener) => listener,
        Err(e) => {
            // Another task may have won the race and bound this exact port between our
            // first check and this bind; prefer the live runtime over a spurious
            // "port occupied" error that would push callers onto a direct connection.
            if let Some(runtime) = lock_runtime().as_ref() {
                if runtime.port == port {
                    return Ok(local_base_url(runtime.port));
                }
            }
            return Err(format!(
                "Failed to bind the protocol gateway on 127.0.0.1:{port} ({e}). \
                 The provider was left on a direct connection."
            ));
        }
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let app = server::router().into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
        {
            log::warn!("protocol gateway stopped: {err}");
        }
    });

    let mut guard = lock_runtime();
    // Another task may have won the race while we were binding; keep the first runtime.
    if let Some(runtime) = guard.as_ref() {
        let _ = shutdown_tx.send(());
        return Ok(local_base_url(runtime.port));
    }
    let base_url = local_base_url(port);
    log::info!("protocol gateway listening on {base_url}");
    guard.replace(GatewayRuntime {
        port,
        shutdown: shutdown_tx,
    });
    Ok(base_url)
}

/// Stops the gateway if it is running. Idempotent.
pub fn stop() {
    let mut guard = lock_runtime();
    if let Some(runtime) = guard.take() {
        let _ = runtime.shutdown.send(());
        log::info!("protocol gateway stopping (port {})", runtime.port);
    }
}

/// Returns the current gateway status without starting it.
#[must_use]
pub fn status() -> GatewayStatus {
    let cfg = super::config::load_config();
    let env_override = env_port_override();
    let (port, source) = resolve_port_with_source(&cfg, env_override.as_deref());
    // Read the runtime before probing: `probe_port` takes the same mutex.
    let listen_port = lock_runtime().as_ref().map(|runtime| runtime.port);
    GatewayStatus {
        running: listen_port.is_some(),
        port,
        base_url: local_base_url(port),
        enabled: cfg.proxy.enabled,
        listen_port,
        config_port: cfg.proxy.port,
        port_source: source.as_str(),
        env_override: source == PortSource::Env,
        port_state: probe_port(port).as_str(),
    }
}

/// Restarts the gateway so a changed port takes effect.
///
/// # Errors
///
/// Returns `Err` if the new port cannot be bound.
pub async fn restart() -> Result<String, String> {
    stop();
    ensure_started().await
}

/// Reconciles the listener and the active provider's address with the current configuration.
///
/// Starts the gateway only when the active provider actually needs translation, stops it
/// otherwise, and rewrites `config.toml` so Codex targets whatever this process can really
/// serve. When the gateway cannot serve the provider at all (globally disabled, or the port
/// cannot be bound) the provider is left on a **direct** connection instead of an unreachable
/// loopback address — the same rule [`routed_base_url`] follows for a slot switch, see
/// `AGENTS.md` §3 invariant 5.
///
/// # Errors
///
/// Returns `Err` if the active runtime cannot be inspected, or `config.toml` cannot be rewritten.
pub async fn reconcile_active_routing() -> Result<(), String> {
    let cfg = super::config::load_config();

    let provider = match super::switch::get_active_runtime_mode()? {
        super::switch::ActiveRuntimeMode::Provider { provider_id, .. } => {
            super::db::get_provider(&provider_id)?
        }
        super::switch::ActiveRuntimeMode::Official { .. } => None,
    };

    // Official mode has no provider table to point anywhere.
    let Some(provider) = provider else {
        stop();
        return Ok(());
    };

    // A user-authored provider table owns its own base_url; the gateway must not take
    // over — same rule as `switch_to_provider` ("kept as-is"). The listener is only
    // reaped when the configuration no longer calls for one; it is never started here,
    // and the user's table is never rewritten.
    let uses_custom_config = provider
        .custom_config_toml
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if uses_custom_config {
        if !cfg.proxy.enabled || !provider.needs_gateway() {
            stop();
        }
        return Ok(());
    }

    let base_url = if cfg.proxy.enabled {
        let (base_url, warning) = routed_base_url(&provider).await;
        if let Some(err) = warning {
            // Reported, not fatal: the provider is left reachable directly.
            log::warn!("protocol gateway unavailable: {err}");
        }
        base_url
    } else {
        stop();
        provider.base_url.trim().to_string()
    };

    let host_config_path = super::paths::codex_home().join("config.toml");
    if !host_config_path.is_file() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&host_config_path)
        .map_err(|e| format!("Failed to read ~/.codex/config.toml: {e}"))?;
    let mut doc: toml_edit::DocumentMut = content
        .parse()
        .map_err(|e| format!("Failed to parse ~/.codex/config.toml: {e}"))?;

    // A user-authored provider table is left untouched by `apply_provider_routing`.
    super::switch::apply_provider_routing(&mut doc, &provider.id, &base_url);
    super::auth::atomic_write(&host_config_path, doc.to_string().as_bytes())
        .map_err(|e| format!("Failed to write ~/.codex/config.toml: {e}"))?;
    Ok(())
}

/// Whether the currently active runtime slot requires a bound gateway listener.
///
/// Mirrors `reconcile_active_routing`: official mode never needs one, a provider with a
/// user-authored `custom_config_toml` is never taken over, and anything else needs the
/// listener only when the global switch is on and at least one model speaks Chat.
fn active_slot_expects_listener(cfg: &AppConfig) -> bool {
    if !cfg.proxy.enabled {
        return false;
    }
    let provider = match super::switch::get_active_runtime_mode() {
        Ok(super::switch::ActiveRuntimeMode::Provider { provider_id, .. }) => {
            super::db::get_provider(&provider_id).ok().flatten()
        }
        _ => None,
    };
    provider.is_some_and(|p| {
        let uses_custom_config = p
            .custom_config_toml
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        !uses_custom_config && p.needs_gateway()
    })
}

/// Applies a new loopback gateway port and makes Codex's `config.toml` follow.
///
/// The single entry point for port changes: validation, probing, persistence, restarting the
/// listener and rewriting the active provider's `base_url` happen together, and a hard failure
/// rolls the port back so the process never rests in a half-applied state. Intentionally has no
/// `port + 1` fallback — a silently drifting port is exactly what makes the address in
/// `config.toml` disagree with the user's expectation.
///
/// # Arguments
///
/// * `port` - New loopback port, `1..=65535`.
///
/// # Returns
///
/// The gateway status after the change.
///
/// # Errors
///
/// Returns `Err` if:
/// - `port` is `0`;
/// - `CODEXQ_PROXY_PORT` overrides the configured port, so `config.json` would be ignored;
/// - another process already holds `port`;
/// - the listener cannot be bound or `config.toml` cannot be rewritten (the port is rolled back).
pub async fn set_gateway_port(port: u16) -> Result<GatewayStatus, String> {
    if port == 0 {
        return Err("端口必须在 1-65535 之间。".to_string());
    }

    let cfg = super::config::load_config();
    let env_override = env_port_override();
    let (current, source) = resolve_port_with_source(&cfg, env_override.as_deref());

    if source == PortSource::Env {
        return Err(format!(
            "端口由环境变量 {PROXY_PORT_ENV}（{current}）指定，CodexQ 不会改写 config.json。\
             请先清除该环境变量。"
        ));
    }
    if current == port {
        return Ok(status());
    }
    if probe_port(port) == PortState::Occupied {
        return Err(format!(
            "端口 {port} 已被占用（可能是残留的 CodexQ 进程或其它程序），\
             请更换端口或先结束占用进程。"
        ));
    }

    super::config::set_setting("proxy.port", &port.to_string())?;

    if let Err(err) = reconcile_active_routing().await {
        let _ = super::config::set_setting("proxy.port", &current.to_string());
        let _ = reconcile_active_routing().await;
        return Err(format!("{err}；端口已回滚到 {current}。"));
    }

    // `reconcile` deliberately degrades a bind failure into a direct connection (warning
    // only), so an Ok return alone would report success while every Chat model is now
    // unreachable. Verify the listener actually bound when one is expected.
    if active_slot_expects_listener(&super::config::load_config())
        && status().listen_port != Some(port)
    {
        let _ = super::config::set_setting("proxy.port", &current.to_string());
        let _ = reconcile_active_routing().await;
        return Err(format!(
            "端口 {port} 未能绑定（可能已被其它进程占用），已回滚到 {current}。"
        ));
    }

    Ok(status())
}

/// Globally enables or disables the local protocol gateway and reconciles the listener.
///
/// Disabling must actively `stop()`: the previous behaviour left an already-running listener
/// alive because [`ensure_started_if_needed`] only returns an error for a disabled gateway
/// instead of tearing the running one down.
///
/// # Arguments
///
/// * `enabled` - New value of `proxy.enabled`.
///
/// # Returns
///
/// The gateway status after the change.
///
/// # Errors
///
/// Returns `Err` if the setting cannot be persisted or a hard failure occurs while reconciling
/// (the previous value is restored first).
pub async fn set_gateway_enabled(enabled: bool) -> Result<GatewayStatus, String> {
    let cfg = super::config::load_config();
    let previous = cfg.proxy.enabled;

    if previous != enabled {
        super::config::set_setting("proxy.enabled", if enabled { "true" } else { "false" })?;
    }

    if let Err(err) = reconcile_active_routing().await {
        let _ = super::config::set_setting("proxy.enabled", if previous { "true" } else { "false" });
        let _ = reconcile_active_routing().await;
        return Err(format!("{err}；网关开关已回滚。"));
    }

    // Enabling must actually produce a listener: `reconcile` degrades a bind failure
    // into a direct connection (warning only), so verify instead of trusting the Ok.
    if enabled
        && active_slot_expects_listener(&super::config::load_config())
        && status().listen_port.is_none()
    {
        let _ = super::config::set_setting("proxy.enabled", if previous { "true" } else { "false" });
        let _ = reconcile_active_routing().await;
        return Err("网关未能启动（监听端口绑定失败），开关已回滚。".to_string());
    }

    Ok(status())
}

/// Reconciles the gateway with the currently active runtime slot at app startup.
///
/// A stale `config.toml` that already points at the loopback gateway is kept alive;
/// otherwise the listener starts only when the active provider actually needs Chat
/// Completions translation. Official mode and direct providers stop any listener.
///
/// # Errors
///
/// Returns `Err` if the active runtime cannot be inspected or the required gateway
/// cannot be bound.
pub async fn ensure_started_if_needed() -> Result<Option<String>, String> {
    let mode = super::switch::get_active_runtime_mode()?;
    let provider_id = match mode {
        super::switch::ActiveRuntimeMode::Provider { provider_id, .. } => provider_id,
        super::switch::ActiveRuntimeMode::Official { .. } => {
            stop();
            return Ok(None);
        }
    };

    let provider = super::db::get_provider(&provider_id)?.ok_or_else(|| {
        format!("Active provider '{provider_id}' is no longer registered.")
    })?;

    if !provider.needs_gateway() {
        stop();
        return Ok(None);
    }

    ensure_started().await.map(Some)
}

/// Locks the runtime mutex, recovering from a poisoned mutex instead of panicking.
fn lock_runtime() -> std::sync::MutexGuard<'static, Option<GatewayRuntime>> {
    RUNTIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overrides(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn model_without_an_override_uses_the_provider_default() {
        let map = overrides(&[("deepseek-v4.1-flash", "chat")]);
        assert_eq!(
            resolve_model_wire_api("responses", Some(&map), "gpt-5.6-luna"),
            "responses"
        );
        assert_eq!(resolve_model_wire_api("responses", Some(&map), ""), "responses");
        assert_eq!(resolve_model_wire_api("chat", None, "anything"), "chat");
    }

    #[test]
    fn model_override_beats_the_provider_default_in_both_directions() {
        let map = overrides(&[
            ("gpt-5.6-luna", "responses"),
            ("deepseek-v4.1-flash", "chat"),
        ]);
        // Provider default is chat; one model opts back into native Responses.
        assert_eq!(
            resolve_model_wire_api("chat", Some(&map), "gpt-5.6-luna"),
            "responses"
        );
        // Provider default is responses; one model opts into Chat Completions.
        assert_eq!(
            resolve_model_wire_api("responses", Some(&map), "deepseek-v4.1-flash"),
            "chat"
        );
    }

    #[test]
    fn model_lookup_is_trimmed_and_case_insensitive_as_a_fallback() {
        let map = overrides(&[("Grok-4.7", "responses")]);
        assert_eq!(
            resolve_model_wire_api("chat", Some(&map), "  Grok-4.7  "),
            "responses"
        );
        assert_eq!(
            resolve_model_wire_api("chat", Some(&map), "grok-4.7"),
            "responses",
            "a hand-typed slug casing must still resolve"
        );
    }

    #[test]
    fn legacy_override_values_are_normalized() {
        let map = overrides(&[("legacy", "chat_completions"), ("vague", "nonsense")]);
        assert_eq!(resolve_model_wire_api("responses", Some(&map), "legacy"), "chat");
        assert_eq!(
            resolve_model_wire_api("responses", Some(&map), "vague"),
            "responses",
            "an unrecognised value degrades to the safe protocol"
        );
    }

    #[test]
    fn a_single_chat_model_pulls_the_whole_provider_through_the_gateway() {
        let map = overrides(&[
            ("gpt-5.6-luna", "responses"),
            ("deepseek-v4.1-flash", "chat"),
        ]);
        assert!(
            provider_needs_gateway(true, "responses", Some(&map)),
            "a Responses-default provider must still route through the gateway for its chat models"
        );
    }

    #[test]
    fn an_all_responses_provider_stays_direct() {
        let map = overrides(&[("gpt-5.6-luna", "responses")]);
        assert!(!provider_needs_gateway(true, "responses", Some(&map)));
        assert!(!provider_needs_gateway(true, "responses", None));
        assert!(!provider_needs_gateway(true, "responses", Some(&overrides(&[]))));
    }

    #[test]
    fn an_all_chat_provider_needs_the_gateway() {
        assert!(provider_needs_gateway(true, "chat", None));
        assert!(provider_needs_gateway(true, "completions", None));
    }

    #[test]
    fn master_switch_off_forces_direct_everywhere() {
        let map = overrides(&[("deepseek-v4.1-flash", "chat")]);
        assert!(!provider_needs_gateway(false, "responses", Some(&map)));
        assert!(!provider_needs_gateway(false, "chat", None));
    }

    #[test]
    fn enabled_but_all_responses_provider_stays_direct() {
        let map = overrides(&[("gpt-5.6-luna", "responses")]);
        assert!(!provider_needs_gateway(true, "responses", Some(&map)));
        assert!(!provider_needs_gateway(true, "responses", None));
    }
}
