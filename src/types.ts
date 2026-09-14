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

export interface WarmupAppSettings {
  default_model: string;
  preset_models: string[];
  prompt: string;
  skip_if_active: boolean;
  min_interval_hours: number;
}

export type ToastPayload = string | { key: string; params?: Record<string, any> };


