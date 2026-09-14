import { type AutoRefreshSettings, DEFAULT_AUTO_REFRESH_SETTINGS } from '../types';

const STORAGE_KEY = 'codexq_auto_refresh_settings';

export function loadAutoRefreshSettings(): AutoRefreshSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return DEFAULT_AUTO_REFRESH_SETTINGS;
    }
    const parsed = JSON.parse(raw);
    return {
      enabled: typeof parsed.enabled === 'boolean' ? parsed.enabled : DEFAULT_AUTO_REFRESH_SETTINGS.enabled,
      intervalMinutes:
        typeof parsed.intervalMinutes === 'number' && parsed.intervalMinutes >= 1
          ? Math.min(1440, Math.floor(parsed.intervalMinutes))
          : DEFAULT_AUTO_REFRESH_SETTINGS.intervalMinutes,
      refreshOnStartup:
        typeof parsed.refreshOnStartup === 'boolean'
          ? parsed.refreshOnStartup
          : DEFAULT_AUTO_REFRESH_SETTINGS.refreshOnStartup,
      notifyOnUpdate:
        typeof parsed.notifyOnUpdate === 'boolean'
          ? parsed.notifyOnUpdate
          : DEFAULT_AUTO_REFRESH_SETTINGS.notifyOnUpdate,
      dynamicResetEnabled:
        typeof parsed.dynamicResetEnabled === 'boolean'
          ? parsed.dynamicResetEnabled
          : DEFAULT_AUTO_REFRESH_SETTINGS.dynamicResetEnabled,
      notifyOnQuotaRestored:
        typeof parsed.notifyOnQuotaRestored === 'boolean'
          ? parsed.notifyOnQuotaRestored
          : DEFAULT_AUTO_REFRESH_SETTINGS.notifyOnQuotaRestored,
    };
  } catch (err) {
    console.warn('Failed to parse auto-refresh settings from localStorage:', err);
    return DEFAULT_AUTO_REFRESH_SETTINGS;
  }
}

export function saveAutoRefreshSettings(settings: AutoRefreshSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch (err) {
    console.warn('Failed to save auto-refresh settings to localStorage:', err);
  }
}
