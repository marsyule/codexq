export interface RateLimitWindow {
  label: string;
  used_percent: number | null;
  remaining_percent: number | null;
  resets_at: number | null;
}

export interface AccountData {
  identity_key: string;
  profile_id: string;
  user_id: string;
  account_id: string;
  email: string | null;
  plan: string | null;
  org_title: string | null;
  alias: string | null;
  display_name: string;
  is_current: boolean;
  credential_status: string;
  last_seen_at: string;
  primary: RateLimitWindow;
  secondary: RateLimitWindow;
  reset_credits: number | null;
  last_error: string | null;
}

export interface SnapshotRecord {
  id: number;
  identity_key: string;
  limit_id: string;
  observed_at: string;
  primary_used_percent: number | null;
  primary_window_minutes: number | null;
  primary_resets_at: number | null;
  secondary_used_percent: number | null;
  secondary_window_minutes: number | null;
  secondary_resets_at: number | null;
  raw_json?: string;
}

export interface TrashAccountData {
  identity_key: string;
  profile_id: string;
  email: string | null;
  user_id: string | null;
  plan: string | null;
  display_name: string;
  removed_at: string;
  has_credentials: boolean;
}

export interface AutoRefreshSettings {
  enabled: boolean;
  intervalMinutes: number;
  refreshOnStartup: boolean;
  notifyOnUpdate: boolean;
  dynamicResetEnabled: boolean;
  notifyOnQuotaRestored: boolean;
}

export const DEFAULT_AUTO_REFRESH_SETTINGS: AutoRefreshSettings = {
  enabled: true,
  intervalMinutes: 15,
  refreshOnStartup: true,
  notifyOnUpdate: false,
  dynamicResetEnabled: true,
  notifyOnQuotaRestored: true,
};

export interface AccountAlarm {
  id: string;
  identity_key: string;
  time_of_day: string; // "HH:MM"
  days_of_week: string; // "1,2,3,4,5"
  enabled: boolean | number;
  model_override: string | null;
  prompt_override: string | null;
  last_triggered_at: string | null;
  last_status: 'success' | 'failed' | 'skipped' | null;
  created_at: string;
}

export interface AccountRolloverConfig {
  enabled: boolean;
  min_weekly_remaining: number;
}

export interface WarmupAppSettings {
  default_model: string;
  preset_models: string[];
  prompt: string;
  skip_if_active: boolean;
  min_interval_hours: number;
}

export type ToastPayload = string | { key: string; params?: Record<string, any> };

export interface ProviderData {
  id: string;
  name: string;
  base_url: string;
  wire_api: string;
  /** Provider-level local protocol gateway switch. */
  gateway_enabled: boolean;
  active_model: string;
  models: string[];
  context_window?: number;
  model_context_windows?: Record<string, number>;
  reasoning_levels?: string[] | null;
  model_reasoning_levels?: Record<string, string[]> | null;
  /**
   * Per-model upstream protocol overrides (`model slug -> 'responses' | 'chat'`).
   *
   * A single upstream can expose both protocols at once, so `wire_api` alone is not
   * expressive enough. Models absent from this map inherit `wire_api`.
   */
  model_wire_apis?: Record<string, string> | null;
  notes: string | null;
  custom_config_toml?: string | null;
  custom_auth_json?: string | null;
  key_masked: string;
  created_at: string;
  updated_at: string;
}

/**
 * Lifecycle and availability snapshot of the loopback protocol gateway.
 *
 * `port` is the effective port (environment > `config.json` > default) and is what `base_url`
 * is built from; `listen_port` is the port this process actually bound and is `null` while the
 * listener is stopped, so the UI never claims a loopback address is live just because it is
 * configured.
 */
export interface GatewayStatus {
  /** Whether this process currently owns the loopback listener. */
  running: boolean;
  /** Effective port that `base_url` is built from. */
  port: number;
  /** Loopback base URL Codex is pointed at, e.g. `http://127.0.0.1:17871/v1`. */
  base_url: string;
  /** Global `proxy.enabled` switch value. */
  enabled: boolean;
  /** Port actually bound by this process, or null when stopped. */
  listen_port: number | null;
  /** Port stored in `~/.codexq/config.json`, ignoring the environment override. */
  config_port: number;
  /** Which source supplied the effective port. */
  port_source: 'env' | 'config' | 'default';
  /** Whether `CODEXQ_PROXY_PORT` currently overrides `config.json`. */
  env_override: boolean;
  /** Availability of the effective port. */
  port_state: 'running' | 'free' | 'occupied';
}

export interface ConnectivityResult {
  success: boolean;
  status_code?: number;
  latency_ms?: number;
  message: string;
  available_models: string[];
}

export type ActiveRuntimeMode =
  | {
      mode: 'official';
      identity_key?: string | null;
      email?: string | null;
      plan?: string | null;
      display_name?: string | null;
    }
  | {
      mode: 'provider';
      provider_id: string;
      name: string;
      active_model: string;
      base_url: string;
      models: string[];
    };

/**
 * Formats a context window token count into human-readable notation (e.g. 256K, 1M).
 * Clamps to a minimum of 256,000 tokens.
 */
export function formatContextWindow(tokens?: number | null): string {
  const val = Math.max(256_000, tokens ?? 256_000);
  if (val >= 1_000_000) {
    const m = val / 1_000_000;
    return Number.isInteger(m) ? `${m}M` : `${m.toFixed(1)}M`;
  }
  if (val >= 1_000) {
    const k = val / 1_000;
    return Number.isInteger(k) ? `${k}K` : `${k.toFixed(1)}K`;
  }
  return `${val}`;
}

export const DEFAULT_REASONING_LEVELS: readonly string[] = ['low', 'medium', 'high'];

/**
 * Canonical reasoning effort progression scale from weakest to strongest:
 * none (0) < minimal (1) < low (2) < medium (3) < high (4) < xhigh (5) < max (6) < ultra (7) < persistent (8)
 */
export const CANONICAL_REASONING_ORDER: Record<string, number> = {
  none: 0,
  minimal: 1,
  low: 2,
  medium: 3,
  high: 4,
  xhigh: 5,
  max: 6,
  ultra: 7,
  persistent: 8,
};

/**
 * Sorts reasoning effort levels strictly in ascending order of reasoning depth.
 */
export function sortReasoningLevels(levels: string[]): string[] {
  return [...levels].sort((a, b) => {
    const aClean = a.trim().toLowerCase();
    const bClean = b.trim().toLowerCase();
    const rankA = CANONICAL_REASONING_ORDER[aClean] ?? 99;
    const rankB = CANONICAL_REASONING_ORDER[bClean] ?? 99;
    if (rankA !== rankB) return rankA - rankB;
    return aClean.localeCompare(bClean);
  });
}

/**
 * Resolves the effective reasoning effort levels for a specific model under a provider.
 */
export function getModelReasoningLevels(provider: ProviderData, modelSlug?: string): string[] {
  if (modelSlug && provider.model_reasoning_levels?.[modelSlug]?.length) {
    return sortReasoningLevels(provider.model_reasoning_levels[modelSlug]);
  }
  if (provider.reasoning_levels?.length) {
    return sortReasoningLevels(provider.reasoning_levels);
  }
  return [...DEFAULT_REASONING_LEVELS];
}

/**
 * Returns whether a given list of reasoning effort levels contains 'max'.
 */
export function hasMaxReasoning(levels?: string[] | null): boolean {
  return Boolean(levels?.some((l) => l.trim().toLowerCase() === 'max'));
}

/**
 * Normalizes a stored wire protocol value into CodexQ's internal two-value domain.
 *
 * Mirrors `protocol_proxy::normalize_wire_api` on the Rust side. Legacy
 * `chat_completions` / `completions` values fold into `'chat'`; anything unrecognised
 * degrades to `'responses'`, because an unknown value written into Codex's config is
 * strictly worse than silently using the standard protocol.
 */
export function normalizeWireApi(value?: string | null): 'responses' | 'chat' {
  const clean = (value ?? '').trim().toLowerCase();
  return ['chat', 'chat_completions', 'chat-completions', 'completions', 'completion'].includes(
    clean,
  )
    ? 'chat'
    : 'responses';
}

/**
 * Resolves the effective upstream wire protocol for a specific model under a provider.
 *
 * Mirrors `Provider::wire_api_for_model` on the Rust side so the UI can show what the
 * gateway will actually do. Lookup is by trimmed exact slug first, then
 * case-insensitively, then it falls back to the provider-level `wire_api`.
 */
export function getModelWireApi(provider: ProviderData, modelSlug?: string): string {
  if (provider.gateway_enabled === false) return 'responses';
  const slug = (modelSlug ?? '').trim();
  const overrides = provider.model_wire_apis;
  if (slug && overrides) {
    if (overrides[slug]) return normalizeWireApi(overrides[slug]);
    const match = Object.keys(overrides).find(
      (key) => key.trim().toLowerCase() === slug.toLowerCase(),
    );
    if (match && overrides[match]) return normalizeWireApi(overrides[match]);
  }
  return normalizeWireApi(provider.wire_api);
}

/**
 * Returns whether any model of a provider needs the loopback protocol gateway.
 *
 * Once one model speaks Chat Completions the whole provider is routed through the
 * gateway, because Codex's config holds a single `base_url` per provider table.
 */
export function providerNeedsGateway(provider: ProviderData): boolean {
  return (
    provider.gateway_enabled &&
    (getModelWireApi(provider) === 'chat' ||
      Object.keys(provider.model_wire_apis ?? {}).some(
        (model) => getModelWireApi(provider, model) === 'chat',
      ))
  );
}


