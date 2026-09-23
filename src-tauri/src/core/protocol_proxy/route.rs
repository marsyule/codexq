//! Upstream route resolution for the protocol gateway.
//!
//! `~/.codex/config.toml` holds a single active `model_provider` slot, so the
//! gateway does not need to distinguish providers by path or by inbound bearer
//! token: it resolves the active third-party provider from the very same source of
//! truth that drives the runtime slot switch.
//!
//! Resolution failures MUST surface as an explicit error. Silently falling back to
//! another upstream would send the user's traffic to the wrong provider and bill
//! them for it.

use super::super::db::get_provider;
use super::super::provider::{self, Provider};
use super::super::switch::{get_active_runtime_mode, ActiveRuntimeMode};

/// Fully resolved upstream target for a single gateway request.
#[derive(Debug, Clone)]
pub struct UpstreamRoute {
    /// CodexQ provider id the request is routed to.
    pub provider_id: String,
    /// Human readable provider name, for error messages only.
    pub provider_name: String,
    /// Raw upstream endpoint as stored in CodexQ (never rewritten to the gateway).
    pub base_url: String,
    /// Upstream protocol for this request: `"responses"` or `"chat"`.
    ///
    /// Resolved per model — see [`resolve_active_route_for_model`].
    pub protocol: String,
    /// Plaintext provider API key read from the `0600` sandbox.
    pub api_key: String,
}

impl UpstreamRoute {
    /// Returns whether the upstream only speaks Chat Completions.
    #[must_use]
    pub fn is_chat(&self) -> bool {
        self.protocol == super::CHAT_WIRE_API
    }

    /// `POST` target for a Responses request.
    #[must_use]
    pub fn responses_url(&self) -> String {
        join_url(&self.base_url, "responses")
    }

    /// `POST` target for a Chat Completions request.
    #[must_use]
    pub fn chat_completions_url(&self) -> String {
        join_url(&self.base_url, "chat/completions")
    }

    /// `GET` target for the upstream model list.
    #[must_use]
    pub fn models_url(&self) -> String {
        join_url(&self.base_url, "models")
    }

    /// The URL the gateway should actually call for the resolved protocol.
    #[must_use]
    pub fn target_url(&self) -> String {
        if self.is_chat() {
            self.chat_completions_url()
        } else {
            self.responses_url()
        }
    }
}

/// Resolves the upstream route using the provider-level protocol default.
///
/// Used by endpoints that carry no model (for example `GET /v1/models`). Request-time
/// routing uses [`resolve_active_route_for_model`] instead.
///
/// # Errors
///
/// Returns `Err` under the same conditions as [`resolve_active_route_for_model`].
pub fn resolve_active_route() -> Result<UpstreamRoute, String> {
    resolve_active_route_for_model("")
}

/// Resolves the upstream route for the currently active provider and one model slug.
///
/// `~/.codex/config.toml` holds a single active `model_provider` slot, so the gateway does
/// not distinguish providers by path or by inbound bearer token: it resolves the active
/// third-party provider from the very same source of truth that drives the runtime slot
/// switch, then picks the protocol for `model`.
///
/// Per-model resolution happens here, at request time, rather than when `config.toml` is
/// written — Codex lets the user switch models mid-session without CodexQ re-running the
/// slot switch, so the only way one `base_url` can serve a mixed-protocol provider is to
/// decide per request.
///
/// Resolution failures MUST surface as an explicit error. Silently falling back to
/// another upstream would send the user's traffic to the wrong provider and bill them
/// for it.
///
/// # Arguments
///
/// * `model` - Model slug from the incoming request; empty means "provider default".
///
/// # Errors
///
/// Returns `Err` if CodexQ is in official-account mode, if the active provider is no
/// longer registered (for example it was deleted while Codex kept the stale config),
/// if its `base_url` is empty, or if the sandboxed API key cannot be read.
pub fn resolve_active_route_for_model(model: &str) -> Result<UpstreamRoute, String> {
    let mode = get_active_runtime_mode()?;
    let provider_id = match mode {
        ActiveRuntimeMode::Provider { provider_id, .. } => provider_id,
        ActiveRuntimeMode::Official { .. } => {
            return Err(
                "CodexQ is in official-account mode; the protocol gateway only serves \
                 third-party providers."
                    .to_string(),
            )
        }
    };

    let resolved = get_provider(&provider_id)?.ok_or_else(|| {
        format!(
            "The active provider '{provider_id}' is no longer registered in CodexQ. \
             Re-activate a provider or switch back to an official account."
        )
    })?;

    let api_key = provider::read_provider_key(&provider_id)?;
    build_route(&resolved, api_key, model)
}

/// Builds a route from a provider record, its plaintext key, and a model slug.
///
/// # Arguments
///
/// * `provider` - Provider record.
/// * `api_key` - Plaintext provider API key.
/// * `model` - Model slug to resolve the protocol for; empty uses the provider default.
///
/// # Errors
///
/// Returns `Err` when the provider has no usable `base_url`.
pub fn build_route(
    provider: &Provider,
    api_key: String,
    model: &str,
) -> Result<UpstreamRoute, String> {
    let base_url = provider.base_url.trim().to_string();
    if base_url.is_empty() {
        return Err(format!(
            "Provider '{}' has no base_url configured, so the gateway cannot reach an upstream.",
            provider.name
        ));
    }
    Ok(UpstreamRoute {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        base_url,
        protocol: provider.wire_api_for_model(model),
        api_key,
    })
}

/// Joins a base URL with an API path, tolerating a trailing slash.
///
/// Mirrors how Codex itself composes `base_url` + endpoint, so a provider configured
/// for direct Responses use behaves identically once routed through the gateway.
#[must_use]
fn join_url(base: &str, suffix: &str) -> String {
    format!("{}/{suffix}", base.trim().trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_provider(wire_api: &str) -> Provider {
        Provider {
            id: "deepseek".to_string(),
            name: "DeepSeek".to_string(),
            base_url: "https://api.deepseek.com/v1/".to_string(),
            wire_api: wire_api.to_string(),
            gateway_enabled: true,
            active_model: "deepseek-v4-flash".to_string(),
            models: vec!["deepseek-v4-flash".to_string()],
            context_window: None,
            model_context_windows: None::<HashMap<String, u64>>,
            reasoning_levels: None,
            model_reasoning_levels: None,
            model_wire_apis: None,
            notes: None,
            custom_config_toml: None,
            custom_auth_json: None,
            key_masked: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn chat_provider_targets_chat_completions() {
        let route =
            build_route(&sample_provider("chat"), "sk-test".to_string(), "").expect("route");
        assert!(route.is_chat());
        assert_eq!(route.target_url(), "https://api.deepseek.com/v1/chat/completions");
        assert_eq!(route.models_url(), "https://api.deepseek.com/v1/models");
    }

    #[test]
    fn responses_provider_targets_responses() {
        let route =
            build_route(&sample_provider("responses"), "sk-test".to_string(), "").expect("route");
        assert!(!route.is_chat());
        assert_eq!(route.target_url(), "https://api.deepseek.com/v1/responses");
    }

    #[test]
    fn legacy_completions_value_is_treated_as_chat() {
        let route =
            build_route(&sample_provider("completions"), "sk-test".to_string(), "").expect("route");
        assert!(route.is_chat());
    }

    #[test]
    fn empty_base_url_is_rejected() {
        let mut provider = sample_provider("chat");
        provider.base_url = "   ".to_string();
        assert!(build_route(&provider, "sk-test".to_string(), "").is_err());
    }

    /// The motivating case: one upstream, one `base_url`, two protocols, selected by model.
    #[test]
    fn one_provider_routes_each_model_to_its_own_protocol() {
        let mut provider = sample_provider("chat");
        provider.base_url = "https://opencode.ai/zen/go/v1".to_string();
        provider.model_wire_apis = Some(HashMap::from([
            ("gpt-5.6-luna".to_string(), "responses".to_string()),
            ("deepseek-v4.1-flash".to_string(), "chat".to_string()),
        ]));

        let native =
            build_route(&provider, "sk-test".to_string(), "gpt-5.6-luna").expect("route");
        assert!(!native.is_chat());
        assert_eq!(
            native.target_url(),
            "https://opencode.ai/zen/go/v1/responses",
            "a Responses model must bypass the Chat Completions translation"
        );

        let converted =
            build_route(&provider, "sk-test".to_string(), "deepseek-v4.1-flash").expect("route");
        assert!(converted.is_chat());
        assert_eq!(
            converted.target_url(),
            "https://opencode.ai/zen/go/v1/chat/completions"
        );

        // An unknown model inherits the provider default.
        let unknown =
            build_route(&provider, "sk-test".to_string(), "mystery-model").expect("route");
        assert!(unknown.is_chat());
    }
}
