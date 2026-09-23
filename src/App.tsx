import { useState, useEffect, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import type { AccountData, TrashAccountData, AutoRefreshSettings, ToastPayload, ProviderData, ActiveRuntimeMode } from './types';
import { loadAutoRefreshSettings, saveAutoRefreshSettings } from './utils/settings';
import { setAppLanguage } from './i18n';
import { ActiveHeroCard } from './components/ActiveHeroCard';
import { AccountCard } from './components/AccountCard';
import { HistoryModal } from './components/HistoryModal';
import { AliasModal } from './components/AliasModal';
import { ResetConfirmModal } from './components/ResetConfirmModal';
import { RemoveConfirmModal } from './components/RemoveConfirmModal';
import { SchedulerView } from './components/SchedulerView';
import { TriggerSettingsCard } from './components/TriggerSettingsCard';
import { AddAccountModal } from './components/AddAccountModal';
import { DoctorCard } from './components/DoctorCard';
import { ProvidersView } from './components/ProvidersView';
import { GatewaySettingsCard } from './components/GatewaySettingsCard';
import {
  RefreshCw,
  RotateCcw,
  Search,
  Zap,
  Users,
  AlertCircle,
  Sparkles,
  ShieldCheck,
  LayoutGrid,
  Settings,
  Trash2,
  Clock,
  AlarmClock,
  Globe,
  Info,
  Plus,
  Server,
} from 'lucide-react';

/**
 * Main application entry component for CodexQ Desktop Dashboard.
 *
 * Coordinates multi-account management, trigger scheduling, auto-refresh polling,
 * recycle bin management, and internationalized system settings.
 *
 * @returns The rendered React application root.
 */
export function App() {
  const { t } = useTranslation();
  const [accounts, setAccounts] = useState<AccountData[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [refreshing, setRefreshing] = useState<boolean>(false);
  const [restartingCodex, setRestartingCodex] = useState<boolean>(false);
  const [resetting, setResetting] = useState<boolean>(false);
  const [removing, setRemoving] = useState<boolean>(false);
  const [switchingId, setSwitchingId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  const [toastMessage, setToastMessage] = useState<{
    text?: string;
    key?: string;
    params?: Record<string, any>;
    type: 'success' | 'error' | 'info';
  } | null>(null);
  const toastTimerRef = useRef<any>(null);
  const [currentTab, setCurrentTab] = useState<'accounts' | 'providers' | 'scheduler' | 'settings'>('accounts');
  const [localePreference, setLocalePreference] = useState<string>('auto');

  // Third-party providers & active mode states
  const [providers, setProviders] = useState<ProviderData[]>([]);
  const [activeMode, setActiveMode] = useState<ActiveRuntimeMode | null>(null);

  // Auto-refresh state & settings
  const [autoRefreshSettings, setAutoRefreshSettings] = useState<AutoRefreshSettings>(() => loadAutoRefreshSettings());
  const refreshingRef = useRef<boolean>(false);
  const lastRefreshedTimeRef = useRef<number>(0);
  const dynamicResetTimerRef = useRef<any>(null);
  const prevAccountsRef = useRef<AccountData[]>([]);
  const [, setRelativeTimeTick] = useState<number>(0);

  // Periodic UI tick to keep relative time strings (e.g. "(just now)", "(in 5m)") accurate
  useEffect(() => {
    const timer = setInterval(() => {
      setRelativeTimeTick((prev) => (prev + 1) % 1000000);
    }, 15000);
    return () => clearInterval(timer);
  }, []);

  // Recycle bin states
  const [trashList, setTrashList] = useState<TrashAccountData[]>([]);
  const [trashLoading, setTrashLoading] = useState<boolean>(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [purgingId, setPurgingId] = useState<string | null>(null);

  // Modals state
  const [historyAccount, setHistoryAccount] = useState<AccountData | null>(null);
  const [aliasAccount, setAliasAccount] = useState<AccountData | null>(null);
  const [removeTargetAccount, setRemoveTargetAccount] = useState<AccountData | null>(null);
  const [showResetModal, setShowResetModal] = useState<boolean>(false);
  const [showAddAccountModal, setShowAddAccountModal] = useState<boolean>(false);

  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  const showToast = useCallback(
    (msg: ToastPayload, type: 'success' | 'error' | 'info' = 'success') => {
      if (toastTimerRef.current) {
        clearTimeout(toastTimerRef.current);
      }
      if (typeof msg === 'string') {
        setToastMessage({ text: msg, type });
      } else {
        setToastMessage({ key: msg.key, params: msg.params, type });
      }
      toastTimerRef.current = setTimeout(() => setToastMessage(null), 5000);
    },
    []
  );

  // Send desktop notification helper
  const notify = useCallback(async (title: string, body: string) => {
    try {
      let permitted = await isPermissionGranted();
      if (!permitted) {
        const permission = await requestPermission();
        permitted = permission === 'granted';
      }
      if (permitted) {
        sendNotification({ title, body });
      }
    } catch (e) {
      console.warn('Desktop notification not available or denied:', e);
    }
  }, []);

  // Fetch accounts list from codexq
  const fetchAccounts = useCallback(async () => {
    try {
      setError(null);
      const res = await invoke<AccountData[]>('list_accounts');
      setAccounts(res);
      prevAccountsRef.current = res;
      setLastUpdated(new Date());
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Failed to load accounts';
      if (msg.trim().startsWith('[') || msg.trim().startsWith('{')) {
        setError(t('accounts.emptyDesc'));
      } else {
        setError(msg);
      }
    } finally {
      setLoading(false);
    }
  }, [t]);

  // Fetch third-party providers
  const fetchProviders = useCallback(async () => {
    try {
      const res = await invoke<ProviderData[]>('list_providers');
      setProviders(res);
    } catch (err) {
      console.warn('Failed to load providers:', err);
    }
  }, []);

  // Fetch current active runtime mode (official vs provider)
  const fetchActiveMode = useCallback(async () => {
    try {
      const res = await invoke<ActiveRuntimeMode>('get_active_runtime_mode');
      setActiveMode(res);
    } catch (err) {
      console.warn('Failed to get active runtime mode:', err);
    }
  }, []);

  // Helper to update and persist auto-refresh settings
  const handleUpdateAutoRefreshSettings = (patch: Partial<AutoRefreshSettings>) => {
    setAutoRefreshSettings((prev) => {
      const updated = { ...prev, ...patch };
      saveAutoRefreshSettings(updated);
      for (const [k, v] of Object.entries(patch)) {
        if (k === 'enabled') invoke('set_app_setting', { key: 'auto_refresh.enabled', value: String(v) }).catch(() => {});
        if (k === 'intervalMinutes') invoke('set_app_setting', { key: 'auto_refresh.interval_minutes', value: String(v) }).catch(() => {});
        if (k === 'refreshOnStartup') invoke('set_app_setting', { key: 'auto_refresh.refresh_on_startup', value: String(v) }).catch(() => {});
        if (k === 'dynamicResetEnabled') invoke('set_app_setting', { key: 'auto_refresh.dynamic_reset_enabled', value: String(v) }).catch(() => {});
        if (k === 'notifyOnQuotaRestored') invoke('set_app_setting', { key: 'auto_refresh.notify_on_quota_restored', value: String(v) }).catch(() => {});
        if (k === 'notifyOnUpdate') invoke('set_app_setting', { key: 'auto_refresh.notify_on_update', value: String(v) }).catch(() => {});
      }
      return updated;
    });
  };

  // Load saved locale setting on startup
  useEffect(() => {
    invoke<Record<string, string>>('get_app_settings')
      .then((res) => {
        if (res && res['general.locale']) {
          setLocalePreference(res['general.locale']);
          setAppLanguage(res['general.locale']);
        }
      })
      .catch((err) => console.warn('Failed to load locale setting', err));
  }, []);

  const handleLanguageChange = async (newLocale: string) => {
    setLocalePreference(newLocale);
    await setAppLanguage(newLocale);
    try {
      await invoke('set_app_setting', { key: 'general.locale', value: newLocale });
    } catch (err) {
      console.warn('Failed to save locale preference', err);
    }
    showToast({ key: 'toasts.languageChanged' });
  };

  // Refresh all quotas via OpenAI codex RPC
  const handleRefreshAll = useCallback(async (options?: { silent?: boolean; toastMessage?: ToastPayload }) => {
    if (refreshingRef.current) return;
    refreshingRef.current = true;

    // Immediately cancel and invalidate any pending dynamic reset timer
    if (dynamicResetTimerRef.current) {
      clearTimeout(dynamicResetTimerRef.current);
      dynamicResetTimerRef.current = null;
    }

    const silent = options?.silent ?? false;
    const customToast = options?.toastMessage;
    setRefreshing(true);
    setError(null);
    try {
      const res = await invoke<AccountData[]>('refresh_all', { concurrency: 5 });
      setAccounts(res);
      setLastUpdated(new Date());
      lastRefreshedTimeRef.current = Date.now();

      // Check if any account's quota was restored
      const currentSettings = loadAutoRefreshSettings();
      if (currentSettings.notifyOnQuotaRestored && prevAccountsRef.current.length > 0) {
        for (const curr of res) {
          const prev = prevAccountsRef.current.find((p) => p.identity_key === curr.identity_key);
          if (!prev) continue;

          const prevPUsed = prev.primary?.used_percent ?? 0;
          const currPUsed = curr.primary?.used_percent ?? 0;
          const primaryRestored = prevPUsed >= 50 && currPUsed < 20;

          const prevSUsed = prev.secondary?.used_percent ?? 0;
          const currSUsed = curr.secondary?.used_percent ?? 0;
          const secRestored = prevSUsed >= 50 && currSUsed < 20;

          if (primaryRestored || secRestored) {
            const label = primaryRestored && secRestored ? '5h & Weekly Quota' : primaryRestored ? '5h Primary Quota' : 'Weekly Quota';
            notify('CodexQ Quota Restored', `Account [${curr.display_name}] ${label} has been fully restored!`);
          }
        }
      }
      prevAccountsRef.current = res;

      if (!silent) {
        if (customToast) {
          showToast(customToast);
        } else {
          const hasReauth = res.some((a) => a.credential_status === 'reauth_required');
          if (hasReauth) {
            showToast({ key: 'toasts.refreshPartialWithReauth' });
          } else {
            showToast({ key: 'toasts.refreshSuccess' });
          }
        }
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message || 'Quota refresh failed';
      if (!silent) {
        if (msg.trim().startsWith('[') || msg.trim().startsWith('{')) {
          setError(t('toasts.refreshFailed', { error: 'Service error' }));
        } else {
          setError(msg);
        }
      } else {
        console.warn('Auto-refresh failed silently:', msg);
      }
    } finally {
      refreshingRef.current = false;
      setRefreshing(false);
    }
  }, [showToast, notify, t]);

  // Switch active account (pure switch, no restart)
  const handleSwitchAccount = async (targetAccount: AccountData) => {
    const targetKey = targetAccount.email || targetAccount.identity_key;
    setSwitchingId(targetAccount.identity_key);
    setError(null);
    try {
      await invoke('switch_account', { target: targetKey, restart: false });
      await fetchAccounts();
      await fetchActiveMode();
      showToast({ key: 'toasts.switchSuccess', params: { name: targetAccount.display_name } });
      notify('CodexQ', t('toasts.switchSuccess', { name: targetAccount.display_name }));
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to switch account');
    } finally {
      setSwitchingId(null);
    }
  };

  // Switch active provider slot
  const handleSwitchProvider = async (providerId: string, modelOverride?: string) => {
    setError(null);
    try {
      const msg = await invoke<string>('switch_to_provider', {
        providerId,
        modelOverride: modelOverride || null,
        restart: false,
      });
      await fetchActiveMode();
      await fetchProviders();
      await fetchAccounts();
      showToast(msg || t('providers.switchedSuccess', 'Switched provider successfully'));
      notify('CodexQ', t('providers.switchedSuccess', 'Switched provider successfully'));
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to switch provider');
    }
  };

  // Delete a provider
  const handleDeleteProvider = async (id: string) => {
    await invoke('delete_provider', { id });
    await fetchProviders();
    await fetchActiveMode();
  };

  // Switch back to official account
  const handleSwitchToOfficial = async () => {
    if (accounts.length > 0) {
      const target = accounts.find((a) => a.is_current) || accounts[0];
      await handleSwitchAccount(target);
    } else {
      showToast(t('providers.noOfficialAccounts', 'No official accounts saved yet to switch to'), 'info');
    }
  };

  // Dedicated manual Codex restart
  const handleRestartCodex = async () => {
    setRestartingCodex(true);
    setError(null);
    try {
      const msg = await invoke<string>('restart_codex', { relaunch: true });
      showToast(msg || { key: 'toasts.codexRestarted' });
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to restart Codex');
    } finally {
      setRestartingCodex(false);
    }
  };

  // Reset all aliases
  const handleResetAllAliases = async () => {
    setResetting(true);
    setError(null);
    try {
      await invoke('reset_all_aliases');
      await fetchAccounts();
      setShowResetModal(false);
      showToast({ key: 'toasts.aliasReset' });
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to reset aliases');
    } finally {
      setResetting(false);
    }
  };

  // Remove account
  const handleConfirmRemove = async () => {
    if (!removeTargetAccount) return;
    setRemoving(true);
    setError(null);
    try {
      const target = removeTargetAccount.email || removeTargetAccount.identity_key;
      await invoke('remove_account', { target });
      await fetchAccounts();
      await fetchTrash();
      showToast({ key: 'toasts.accountRemoved' });
      setRemoveTargetAccount(null);
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to remove account');
    } finally {
      setRemoving(false);
    }
  };

  // Fetch trash list
  const fetchTrash = useCallback(async () => {
    try {
      setTrashLoading(true);
      const res = await invoke<TrashAccountData[]>('list_trash');
      setTrashList(res || []);
    } catch (err: any) {
      console.error('Failed to load trash list:', err);
    } finally {
      setTrashLoading(false);
    }
  }, []);

  // Restore account from trash
  const handleRestoreAccount = async (item: TrashAccountData) => {
    setRestoringId(item.identity_key);
    setError(null);
    try {
      const target = item.identity_key;
      await invoke('restore_account', { target });
      showToast({ key: 'toasts.accountRestored', params: { name: item.display_name } });
      await fetchAccounts();
      await fetchTrash();
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to restore account');
    } finally {
      setRestoringId(null);
    }
  };

  // Purge trash account
  const handlePurgeAccount = async (item?: TrashAccountData) => {
    if (item) {
      if (!window.confirm(t('modals.purge.desc', { email: item.display_name }))) return;
      setPurgingId(item.identity_key);
    } else {
      if (!window.confirm(t('modals.emptyTrash.desc'))) return;
      setTrashLoading(true);
    }
    setError(null);
    try {
      await invoke('purge_trash', { target: item ? item.identity_key : null });
      showToast(item ? { key: 'toasts.trashPurged', params: { name: item.display_name } } : { key: 'toasts.trashEmptied' });
      await fetchTrash();
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to purge trash');
    } finally {
      setPurgingId(null);
      setTrashLoading(false);
    }
  };

  // Initial load and tray event listener
  useEffect(() => {
    const init = async () => {
      await fetchAccounts();
      await fetchTrash();
      await fetchProviders();
      await fetchActiveMode();
      try {
        const remoteCfg = await invoke<Record<string, string>>('get_app_settings');
        if (remoteCfg && remoteCfg['auto_refresh.interval_minutes']) {
          const mergedSettings: AutoRefreshSettings = {
            enabled: remoteCfg['auto_refresh.enabled'] ? remoteCfg['auto_refresh.enabled'] === 'true' : true,
            intervalMinutes: remoteCfg['auto_refresh.interval_minutes'] ? parseInt(remoteCfg['auto_refresh.interval_minutes'], 10) || 15 : 15,
            refreshOnStartup: remoteCfg['auto_refresh.refresh_on_startup'] ? remoteCfg['auto_refresh.refresh_on_startup'] === 'true' : true,
            dynamicResetEnabled: remoteCfg['auto_refresh.dynamic_reset_enabled'] ? remoteCfg['auto_refresh.dynamic_reset_enabled'] === 'true' : true,
            notifyOnQuotaRestored: remoteCfg['auto_refresh.notify_on_quota_restored'] ? remoteCfg['auto_refresh.notify_on_quota_restored'] === 'true' : true,
            notifyOnUpdate: remoteCfg['auto_refresh.notify_on_update'] ? remoteCfg['auto_refresh.notify_on_update'] === 'true' : false,
          };
          setAutoRefreshSettings(mergedSettings);
          saveAutoRefreshSettings(mergedSettings);
        }
      } catch (e) {
        console.warn('Failed to sync auto_refresh settings from backend:', e);
      }
      const currentSettings = loadAutoRefreshSettings();
      if (currentSettings.refreshOnStartup) {
        await handleRefreshAll({ silent: false, toastMessage: { key: 'toasts.startupSync' } });
      } else {
        lastRefreshedTimeRef.current = Date.now();
      }
    };
    init();

    // Listen to tray "Refresh All Quotas" menu click
    const unlistenPromise = listen('tray-refresh', () => {
      handleRefreshAll();
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [fetchAccounts, fetchTrash, handleRefreshAll, t]);

  // Periodic auto-refresh timer & visibility change listener
  useEffect(() => {
    if (!autoRefreshSettings.enabled || autoRefreshSettings.intervalMinutes <= 0) {
      return;
    }

    const intervalMs = autoRefreshSettings.intervalMinutes * 60 * 1000;

    const checkAndTick = () => {
      if (refreshingRef.current || lastRefreshedTimeRef.current === 0) {
        return;
      }

      const now = Date.now();
      const elapsed = now - lastRefreshedTimeRef.current;

      // 1. Standard interval auto-refresh
      if (elapsed >= intervalMs) {
        handleRefreshAll({ silent: !autoRefreshSettings.notifyOnUpdate });
        return;
      }

      // 2. Overdue dynamic reset check: if an account's quota reset time has arrived (+10s buffer)
      // but its quota is still marked as consumed, trigger a prompt sync (min 30s debounce between calls)
      if (autoRefreshSettings.dynamicResetEnabled && elapsed >= 30000) {
        let hasOverdue = false;
        for (const a of accounts) {
          const pResets = a.primary?.resets_at;
          const pUsed = a.primary?.used_percent;
          if (pResets && typeof pUsed === 'number' && pUsed > 0 && now >= (pResets + 10) * 1000) {
            hasOverdue = true;
            break;
          }
          const sResets = a.secondary?.resets_at;
          const sUsed = a.secondary?.used_percent;
          if (sResets && typeof sUsed === 'number' && sUsed > 0 && now >= (sResets + 10) * 1000) {
            hasOverdue = true;
            break;
          }
        }

        if (hasOverdue) {
          handleRefreshAll({
            silent: !autoRefreshSettings.notifyOnUpdate,
            toastMessage: { key: 'toasts.jitSync' },
          });
        }
      }
    };

    const timer = setInterval(checkAndTick, 2000);

    const handleVisibilityChange = () => {
      if (document.visibilityState === 'visible' && !refreshingRef.current && lastRefreshedTimeRef.current > 0) {
        const now = Date.now();
        const elapsed = now - lastRefreshedTimeRef.current;
        const isIntervalOverdue = elapsed >= intervalMs;

        let hasOverdueReset = false;
        if (autoRefreshSettings.dynamicResetEnabled && elapsed >= 30000) {
          for (const a of accounts) {
            const pResets = a.primary?.resets_at;
            const pUsed = a.primary?.used_percent;
            if (pResets && typeof pUsed === 'number' && pUsed > 0 && now >= (pResets + 10) * 1000) {
              hasOverdueReset = true;
              break;
            }
            const sResets = a.secondary?.resets_at;
            const sUsed = a.secondary?.used_percent;
            if (sResets && typeof sUsed === 'number' && sUsed > 0 && now >= (sResets + 10) * 1000) {
              hasOverdueReset = true;
              break;
            }
          }
        }

        if (isIntervalOverdue || hasOverdueReset) {
          handleRefreshAll({
            silent: !autoRefreshSettings.notifyOnUpdate,
            toastMessage: hasOverdueReset ? { key: 'toasts.jitSync' } : undefined,
          });
        }
      }
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    window.addEventListener('focus', handleVisibilityChange);

    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.removeEventListener('focus', handleVisibilityChange);
    };
  }, [accounts, autoRefreshSettings.dynamicResetEnabled, autoRefreshSettings.enabled, autoRefreshSettings.intervalMinutes, autoRefreshSettings.notifyOnUpdate, handleRefreshAll, t]);

  // JIT dynamic quota reset scheduler: listens for the earliest upcoming reset_at (+10s) among consumed accounts
  useEffect(() => {
    if (dynamicResetTimerRef.current) {
      clearTimeout(dynamicResetTimerRef.current);
      dynamicResetTimerRef.current = null;
    }

    if (!autoRefreshSettings.dynamicResetEnabled || accounts.length === 0) {
      return;
    }

    const now = Date.now();
    let earliestTargetSeconds: number | null = null;
    let hasOverdueAccount = false;

    for (const a of accounts) {
      // Primary window (5h)
      if (
        a.primary &&
        typeof a.primary.used_percent === 'number' &&
        a.primary.used_percent > 0 &&
        a.primary.resets_at
      ) {
        const targetSec = a.primary.resets_at + 10;
        if (targetSec * 1000 > now) {
          if (earliestTargetSeconds === null || targetSec < earliestTargetSeconds) {
            earliestTargetSeconds = targetSec;
          }
        } else {
          hasOverdueAccount = true;
        }
      }

      // Secondary window (1w)
      if (
        a.secondary &&
        typeof a.secondary.used_percent === 'number' &&
        a.secondary.used_percent > 0 &&
        a.secondary.resets_at
      ) {
        const targetSec = a.secondary.resets_at + 10;
        if (targetSec * 1000 > now) {
          if (earliestTargetSeconds === null || targetSec < earliestTargetSeconds) {
            earliestTargetSeconds = targetSec;
          }
        } else {
          hasOverdueAccount = true;
        }
      }
    }

    if (hasOverdueAccount && !refreshingRef.current && (now - lastRefreshedTimeRef.current >= 30000)) {
      dynamicResetTimerRef.current = setTimeout(async () => {
        dynamicResetTimerRef.current = null;
        if (!refreshingRef.current) {
          await handleRefreshAll({
            silent: !autoRefreshSettings.notifyOnUpdate,
            toastMessage: { key: 'toasts.jitSync' },
          });
        }
      }, 1500);
    } else if (earliestTargetSeconds !== null) {
      const delayMs = Math.max(1000, earliestTargetSeconds * 1000 - now);
      if (delayMs < 7 * 24 * 60 * 60 * 1000) {
        dynamicResetTimerRef.current = setTimeout(async () => {
          dynamicResetTimerRef.current = null;
          if (!refreshingRef.current) {
            await handleRefreshAll({
              silent: !autoRefreshSettings.notifyOnUpdate,
              toastMessage: { key: 'toasts.jitSync' },
            });
          }
        }, delayMs);
      }
    }

    return () => {
      if (dynamicResetTimerRef.current) {
        clearTimeout(dynamicResetTimerRef.current);
        dynamicResetTimerRef.current = null;
      }
    };
  }, [accounts, autoRefreshSettings.dynamicResetEnabled, autoRefreshSettings.notifyOnUpdate, handleRefreshAll, t]);

  // Refetch trash when switching to settings tab
  useEffect(() => {
    if (currentTab === 'settings') {
      fetchTrash();
    }
  }, [currentTab, fetchTrash]);

  // Separate active account and inactive accounts
  const activeAccount = accounts.find((a) => a.is_current);
  const inactiveAccounts = accounts.filter((a) => !a.is_current);

  // Filter accounts by search query
  const filteredInactive = inactiveAccounts.filter((a) => {
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return (
      a.display_name.toLowerCase().includes(q) ||
      (a.email && a.email.toLowerCase().includes(q)) ||
      (a.alias && a.alias.toLowerCase().includes(q)) ||
      (a.plan && a.plan.toLowerCase().includes(q))
    );
  });

  return (
    <div className="flex h-full w-full overflow-hidden bg-[#f5f7fa] text-slate-800 font-sans select-none">
      {/* Clash Verge Style Left Sidebar */}
      <aside className="w-44 shrink-0 h-full bg-white border-r border-slate-200/90 flex flex-col justify-between p-3 z-20 shadow-xs overflow-hidden">
        <div className="flex flex-col min-h-0 overflow-y-auto">
          {/* Brand Logo & Name */}
          <div className="flex items-center gap-2.5 px-1 py-1 mb-4">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-blue-600 shadow-sm shadow-blue-500/20 text-white font-bold">
              <Zap className="h-4 w-4 fill-white" />
            </div>
            <div>
              <div className="flex items-center gap-1">
                <h1 className="text-sm font-bold tracking-tight text-slate-900">CodexQ</h1>
                <span className="rounded bg-blue-50 text-blue-600 border border-blue-200/60 px-1 py-0.1 text-[9px] font-semibold">
                  v1.0.0
                </span>
              </div>
              <p className="text-[10px] text-slate-400">{t('app.subtitle')}</p>
            </div>
          </div>

          {/* Navigation Pill Menu */}
          <nav className="space-y-1">
            <button
              onClick={() => setCurrentTab('accounts')}
              className={`flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-xs font-semibold transition-all ${
                currentTab === 'accounts'
                  ? 'bg-blue-100/70 text-blue-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
              }`}
            >
              <LayoutGrid className="w-3.5 h-3.5" />
              <span>{t('nav.accounts')}</span>
            </button>

            <button
              onClick={() => setCurrentTab('providers')}
              className={`flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-xs font-semibold transition-all ${
                currentTab === 'providers'
                  ? 'bg-blue-100/70 text-blue-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
              }`}
            >
              <Server className="w-3.5 h-3.5" />
              <span>{t('nav.providers')}</span>
            </button>

            <button
              onClick={() => setCurrentTab('scheduler')}
              className={`flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-xs font-semibold transition-all ${
                currentTab === 'scheduler'
                  ? 'bg-blue-100/70 text-blue-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
              }`}
            >
              <AlarmClock className="w-3.5 h-3.5" />
              <span>{t('nav.scheduler')}</span>
            </button>

            <button
              onClick={() => setCurrentTab('settings')}
              className={`flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-xs font-semibold transition-all ${
                currentTab === 'settings'
                  ? 'bg-blue-100/70 text-blue-700 shadow-xs'
                  : 'text-slate-600 hover:text-slate-900 hover:bg-slate-100'
              }`}
            >
              <Settings className="w-3.5 h-3.5" />
              <span>{t('nav.settings')}</span>
            </button>
          </nav>
        </div>

        {/* Sidebar Bottom Mini Status Card */}
        <div className="shrink-0 mt-3 rounded-lg border border-slate-200/80 bg-slate-50/80 p-2.5 space-y-1.5 text-xs">
          <div className="flex items-center justify-between text-[11px] text-slate-500 font-medium">
            <span>{t('accounts.connectedPool')}</span>
            <span className="font-semibold text-slate-800">{accounts.length}</span>
          </div>
          <div className="flex items-center justify-between text-[11px] text-slate-500 font-medium">
            <span>{t('accounts.availableResets')}</span>
            <span className="font-semibold text-amber-700 flex items-center gap-1">
              <Zap className="w-3 h-3 text-amber-500 fill-amber-500/30" />
              {accounts.reduce((acc, a) => acc + (a.reset_credits || 0), 0)}
            </span>
          </div>
          {lastUpdated && (
            <div className="text-[10px] text-slate-400 pt-1 border-t border-slate-200/60 truncate">
              {t('accounts.synced')}: {lastUpdated.toLocaleTimeString()}
            </div>
          )}
        </div>
      </aside>

      {/* Main Content Area */}
      <div className="flex-1 flex flex-col min-w-0 h-full overflow-hidden">
        {/* Top Header Bar */}
        <header className="shrink-0 z-10 border-b border-slate-200/80 bg-[#f5f7fa] px-5 py-2.5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            {/* View Title */}
            <div>
              <h2 className="text-base font-bold tracking-tight text-slate-900">
                {currentTab === 'accounts'
                  ? t('nav.accounts')
                  : currentTab === 'providers'
                  ? t('nav.providers')
                  : currentTab === 'scheduler'
                  ? t('nav.scheduler')
                  : t('nav.settings')}
              </h2>
              <p className="text-[11px] text-slate-500">
                {currentTab === 'accounts'
                  ? t('app.header.accountsDesc')
                  : currentTab === 'providers'
                  ? t('app.header.providersDesc')
                  : currentTab === 'scheduler'
                  ? t('app.header.schedulerDesc')
                  : t('app.header.settingsDesc')}
              </p>
            </div>

            {/* Right Tools & Actions */}
            <div className="flex items-center gap-2">
              {/* Search Bar */}
              {currentTab === 'accounts' && accounts.length > 2 && (
                <div className="relative w-44 hidden sm:block">
                  <Search className="absolute left-2.5 top-2 h-3.5 w-3.5 text-slate-400" />
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={t('accounts.searchPlaceholder')}
                    className="w-full rounded-lg border border-slate-200 bg-white py-1 pl-7 pr-2.5 text-xs text-slate-800 placeholder-slate-400 focus:border-blue-500 focus:outline-none focus:ring-2 focus:ring-blue-500/20 shadow-xs transition-all"
                  />
                </div>
              )}

              {/* Restart Codex Button */}
              <button
                onClick={handleRestartCodex}
                disabled={restartingCodex}
                className="flex items-center gap-1 rounded-lg border border-slate-200 bg-white px-2.5 py-1 text-xs font-semibold text-slate-700 hover:bg-slate-50 hover:text-slate-900 shadow-xs transition-all disabled:opacity-50"
                title={t('settings.dataProcess.restartCodexDesc')}
              >
                <RotateCcw className={`h-3 w-3 ${restartingCodex ? 'animate-spin text-blue-600' : 'text-slate-500'}`} />
                <span>{restartingCodex ? t('app.status.restarting') : t('app.actions.restartCodex')}</span>
              </button>

              {/* Refresh Quotas Button */}
              <button
                onClick={() => handleRefreshAll()}
                disabled={refreshing}
                className="flex items-center gap-1 rounded-lg bg-blue-600 px-3 py-1 text-xs font-semibold text-white hover:bg-blue-700 shadow-xs shadow-blue-600/20 transition-all disabled:opacity-50"
                title={t('app.actions.refreshQuotas')}
              >
                <RefreshCw className={`h-3 w-3 ${refreshing ? 'animate-spin' : ''}`} />
                <span>{refreshing ? t('app.status.refreshing') : t('app.actions.refreshQuotas')}</span>
              </button>
            </div>
          </div>
        </header>

        {/* Main Body */}
        <main className="flex-1 overflow-y-auto min-h-0 p-4 sm:p-5 space-y-3.5 max-w-6xl w-full mx-auto">
          {/* Status Toast Banner */}
          {toastMessage && (
            <div
              className={`flex items-start sm:items-center justify-between gap-2.5 rounded-xl border px-3.5 py-2.5 text-xs font-medium animate-in fade-in slide-in-from-top-2 duration-200 shadow-xs ${
                toastMessage.type === 'error'
                  ? 'border-rose-200 bg-rose-50/95 text-rose-800'
                  : toastMessage.type === 'info'
                  ? 'border-blue-200 bg-blue-50/95 text-blue-800'
                  : 'border-emerald-200 bg-emerald-50/95 text-emerald-800'
              }`}
            >
              <div className="flex items-start sm:items-center gap-2 min-w-0 flex-1">
                {toastMessage.type === 'error' ? (
                  <AlertCircle className="h-4 w-4 shrink-0 text-rose-600 mt-0.5 sm:mt-0" />
                ) : toastMessage.type === 'info' ? (
                  <Info className="h-4 w-4 shrink-0 text-blue-600 mt-0.5 sm:mt-0" />
                ) : (
                  <Sparkles className="h-4 w-4 shrink-0 text-emerald-600 mt-0.5 sm:mt-0" />
                )}
                <span className="break-all whitespace-pre-wrap leading-relaxed">
                  {toastMessage.key ? t(toastMessage.key, toastMessage.params) : toastMessage.text}
                </span>
              </div>
              <button
                onClick={() => setToastMessage(null)}
                className="text-slate-400 hover:text-slate-600 font-bold ml-2 shrink-0 p-0.5 transition-colors"
                title="Close"
              >
                ✕
              </button>
            </div>
          )}

          {/* Error Alert Banner */}
          {error && (
            <div className="flex items-center gap-1.5 rounded-lg border border-rose-200 bg-rose-50 px-3.5 py-2 text-xs font-medium text-rose-800 shadow-xs">
              <AlertCircle className="h-3.5 w-3.5 shrink-0 text-rose-600" />
              <span className="flex-1">{error}</span>
              <button
                onClick={() => setError(null)}
                className="text-rose-500 hover:text-rose-700 ml-2 font-bold"
              >
                ✕
              </button>
            </div>
          )}

          {/* Tab 1: Accounts Pool */}
          <div className={currentTab === 'accounts' ? 'space-y-3.5' : 'hidden'}>
            {/* Section 1: Active Account Hero Banner */}
            {activeMode?.mode === 'provider' || activeAccount ? (
              <div>
                <div className="flex items-center justify-between mb-1.5 px-0.5">
                  <span className="text-[11px] font-semibold uppercase tracking-wider text-slate-500 flex items-center gap-1">
                    <ShieldCheck className="w-3.5 h-3.5 text-emerald-600" />
                    {t('accounts.activeSession')}
                  </span>
                  {lastUpdated && (
                    <span className="text-[10px] text-slate-400">
                      {t('accounts.synced')}: {lastUpdated.toLocaleTimeString()}
                    </span>
                  )}
                </div>
                <ActiveHeroCard
                  account={activeAccount}
                  activeMode={activeMode}
                  providers={providers}
                  onEditAlias={(acc) => setAliasAccount(acc)}
                  onViewHistory={(acc) => setHistoryAccount(acc)}
                  onSwitchToOfficial={handleSwitchToOfficial}
                />
              </div>
            ) : !loading && (
              <div className="rounded-xl border border-dashed border-amber-300 bg-amber-50/70 p-5 text-center shadow-xs">
                <AlertCircle className="w-7 h-7 text-amber-600 mx-auto mb-2" />
                <h3 className="text-xs font-semibold text-slate-800">{t('accounts.noActiveTitle')}</h3>
                <p className="text-[11px] text-slate-500 mt-1 max-w-md mx-auto leading-relaxed">
                  {t('accounts.noActiveDesc')}
                </p>
              </div>
            )}

            {/* Section 2: Inactive Account Pool Cards */}
            <div className="space-y-2.5">
              <div className="flex items-center justify-between px-0.5">
                <div className="flex items-center gap-1.5">
                  <span className="text-[11px] font-semibold uppercase tracking-wider text-slate-500 flex items-center gap-1">
                    <Users className="w-3.5 h-3.5 text-blue-600" />
                    {t('accounts.availablePool')}
                  </span>
                  <span className="rounded-full bg-slate-200/80 px-1.5 py-0.2 text-[10px] font-semibold text-slate-700">
                    {filteredInactive.length}
                  </span>
                </div>

                <div className="flex items-center gap-2">
                  {searchQuery && (
                    <span className="text-xs text-slate-400">
                      {t('accounts.filterCriteria')}: "<span className="text-slate-700 font-medium">{searchQuery}</span>"
                    </span>
                  )}
                  <button
                    onClick={() => setShowAddAccountModal(true)}
                    className="flex items-center gap-1 rounded-lg border border-blue-200 bg-blue-50/80 px-2.5 py-1 text-xs font-semibold text-blue-700 hover:bg-blue-100 hover:border-blue-300 shadow-xs transition-all"
                  >
                    <Plus className="h-3 w-3" />
                    <span>{t('accounts.addBtn')}</span>
                  </button>
                </div>
              </div>

              {loading ? (
                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                  {[1, 2].map((i) => (
                    <div
                      key={i}
                      className="h-32 rounded-xl border border-slate-200/80 bg-white shadow-xs animate-pulse"
                    />
                  ))}
                </div>
              ) : filteredInactive.length === 0 ? (
                <div className="rounded-xl border border-slate-200/90 bg-white p-6 text-center shadow-xs">
                  {accounts.length <= 1 ? (
                    <div className="space-y-2.5 max-w-md mx-auto py-1">
                      <p className="text-xs text-slate-700 font-semibold">{t('accounts.noOtherAccounts')}</p>
                      <p className="text-[11px] text-slate-500 leading-relaxed">
                        {t('accounts.noOtherAccountsDesc')}
                      </p>
                      <button
                        onClick={() => setShowAddAccountModal(true)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-xs font-semibold text-slate-700 hover:bg-slate-50 hover:text-slate-900 shadow-xs transition-all mt-1"
                      >
                        <Plus className="h-3.5 w-3.5 text-blue-600" />
                        <span>{t('accounts.addBtn')}</span>
                      </button>
                    </div>
                  ) : (
                    <p className="text-xs text-slate-500">{t('accounts.noSearchResults')}</p>
                  )}
                </div>
              ) : (
                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                  {filteredInactive.map((account) => (
                    <AccountCard
                      key={account.identity_key}
                      account={account}
                      onSwitch={handleSwitchAccount}
                      onEditAlias={(acc) => setAliasAccount(acc)}
                      onViewHistory={(acc) => setHistoryAccount(acc)}
                      onRemove={(acc) => setRemoveTargetAccount(acc)}
                      isSwitching={switchingId === account.identity_key}
                    />
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Tab: Third-Party Providers */}
          <div className={currentTab === 'providers' ? 'space-y-4' : 'hidden'}>
            <ProvidersView
              providers={providers}
              activeMode={activeMode}
              onRefresh={async () => {
                await fetchProviders();
                await fetchActiveMode();
              }}
              onSwitchProvider={handleSwitchProvider}
              onDeleteProvider={handleDeleteProvider}
              showToast={showToast}
            />
          </div>

          {/* Tab 2: Scheduled Trigger */}
          <div className={currentTab === 'scheduler' ? 'space-y-4' : 'hidden'}>
            <SchedulerView
              accounts={accounts}
              onRefreshAccounts={fetchAccounts}
              showToast={showToast}
            />
          </div>

          {/* Tab 3: Settings */}
          <div className={currentTab === 'settings' ? 'space-y-6' : 'hidden'}>
              {/* General & Preferences Card */}
              <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-xs space-y-4">
                <div className="flex items-center justify-between pb-2 border-b border-slate-100">
                  <div className="flex items-center gap-2.5">
                    <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                      <Globe className="w-4 h-4" />
                    </div>
                    <div>
                      <h3 className="text-base font-bold text-slate-900">{t('settings.general.title')}</h3>
                      <p className="text-xs text-slate-500 mt-0.5">
                        {t('settings.general.languageDesc')}
                      </p>
                    </div>
                  </div>
                </div>

                <div className="flex items-center justify-between py-3">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.general.language')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.general.languageDesc')}</p>
                  </div>
                  <div className="w-60">
                    <select
                      value={localePreference}
                      onChange={(e) => handleLanguageChange(e.target.value)}
                      className="w-full rounded-xl border border-slate-200 bg-white py-2 px-3 text-xs font-semibold text-slate-700 shadow-xs focus:border-blue-500 focus:outline-none focus:ring-2 focus:ring-blue-500/20 transition-all cursor-pointer"
                    >
                      <option value="auto">{t('settings.general.auto')}</option>
                      <option value="zh-CN">{t('settings.general.zhCN')}</option>
                      <option value="en-US">{t('settings.general.enUS')}</option>
                    </select>
                  </div>
                </div>
              </div>

              {/* Local Protocol Gateway Card */}
              <GatewaySettingsCard showToast={showToast} activeMode={activeMode} />
              {/* Auto-Refresh Settings Card */}
              <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-xs space-y-4">
                <div className="flex items-center justify-between pb-2 border-b border-slate-100">
                  <div className="flex items-center gap-2.5">
                    <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                      <Clock className="w-4 h-4" />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <h3 className="text-base font-bold text-slate-900">{t('settings.refresh.title')}</h3>
                        <span
                          className={`px-2 py-0.5 rounded-full text-[11px] font-semibold border ${
                            autoRefreshSettings.enabled
                              ? 'bg-blue-50 text-blue-700 border-blue-200/80'
                              : 'bg-slate-100 text-slate-500 border-slate-200/80'
                          }`}
                        >
                          {autoRefreshSettings.enabled
                            ? t('settings.refresh.statusEvery', { mins: autoRefreshSettings.intervalMinutes })
                            : t('settings.refresh.statusPaused')}
                        </span>
                      </div>
                      <p className="text-xs text-slate-500 mt-0.5">
                        {t('settings.refresh.enableDesc')}
                      </p>
                    </div>
                  </div>
                </div>

                {/* Item 1: Enable Auto-Refresh Toggle */}
                <div className="flex items-center justify-between py-3 border-b border-slate-100">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.refresh.enableLabel')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.refresh.enableDesc')}</p>
                  </div>
                  <label className="relative inline-flex items-center cursor-pointer">
                    <input
                      type="checkbox"
                      checked={autoRefreshSettings.enabled}
                      onChange={(e) => {
                        const updated = e.target.checked;
                        handleUpdateAutoRefreshSettings({ enabled: updated });
                        showToast(
                          updated
                            ? { key: 'toasts.autoRefreshEnabled', params: { mins: autoRefreshSettings.intervalMinutes } }
                            : { key: 'toasts.autoRefreshPaused' }
                        );
                      }}
                      className="sr-only peer"
                    />
                    <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
                  </label>
                </div>

                {/* Item 2: Refresh Interval Buttons & Custom Input */}
                <div className={`flex flex-col sm:flex-row sm:items-center justify-between gap-3 py-3 border-b border-slate-100 transition-opacity ${!autoRefreshSettings.enabled ? 'opacity-40 pointer-events-none' : ''}`}>
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.refresh.intervalLabel')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.refresh.intervalCustom')}</p>
                  </div>
                  <div className="flex items-center gap-1.5 flex-wrap justify-end">
                    {[5, 10, 15, 30, 60].map((mins) => (
                      <button
                        key={mins}
                        onClick={() => handleUpdateAutoRefreshSettings({ intervalMinutes: mins })}
                        className={`px-2.5 py-1 text-xs rounded-lg font-semibold transition-all border ${
                          autoRefreshSettings.intervalMinutes === mins
                            ? 'bg-blue-600 text-white border-blue-600 shadow-xs'
                            : 'bg-slate-50 text-slate-700 border-slate-200 hover:bg-slate-100'
                        }`}
                      >
                        {mins} {t('settings.refresh.minutes')}
                      </button>
                    ))}
                    <div className="flex items-center gap-1 ml-1">
                      <input
                        type="number"
                        min={1}
                        max={1440}
                        value={autoRefreshSettings.intervalMinutes}
                        onChange={(e) => {
                          const val = parseInt(e.target.value, 10);
                          if (!isNaN(val) && val >= 1 && val <= 1440) {
                            handleUpdateAutoRefreshSettings({ intervalMinutes: val });
                          }
                        }}
                        className="w-14 rounded-lg border border-slate-200 bg-white py-1 px-1.5 text-xs text-center font-medium focus:border-blue-500 focus:outline-none"
                      />
                      <span className="text-xs text-slate-500">{t('settings.refresh.minUnit')}</span>
                    </div>
                  </div>
                </div>

                {/* Item 3: Refresh on Startup */}
                <div className="flex items-center justify-between py-3 border-b border-slate-100">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.refresh.startupLabel')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.refresh.startupDesc')}</p>
                  </div>
                  <label className="relative inline-flex items-center cursor-pointer">
                    <input
                      type="checkbox"
                      checked={autoRefreshSettings.refreshOnStartup}
                      onChange={(e) => handleUpdateAutoRefreshSettings({ refreshOnStartup: e.target.checked })}
                      className="sr-only peer"
                    />
                    <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
                  </label>
                </div>

                {/* Item 4: Dynamic JIT Reset Refresh */}
                <div className="flex items-center justify-between py-3 border-b border-slate-100">
                  <div>
                    <div className="flex items-center gap-1.5">
                      <h4 className="text-sm font-semibold text-slate-800">{t('settings.refresh.dynamicResetLabel')}</h4>
                      <span className="text-[10px] bg-blue-50 text-blue-600 border border-blue-200/60 px-1.5 py-0.5 rounded font-medium">
                        {t('settings.refresh.dynamicResetTag')}
                      </span>
                    </div>
                    <p className="text-xs text-slate-500 mt-0.5">{t('settings.refresh.dynamicResetDesc')}</p>
                  </div>
                  <label className="relative inline-flex items-center cursor-pointer">
                    <input
                      type="checkbox"
                      checked={autoRefreshSettings.dynamicResetEnabled}
                      onChange={(e) => {
                        const updated = e.target.checked;
                        handleUpdateAutoRefreshSettings({ dynamicResetEnabled: updated });
                        showToast(updated ? { key: 'toasts.dynamicResetEnabled' } : { key: 'toasts.dynamicResetDisabled' });
                      }}
                      className="sr-only peer"
                    />
                    <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
                  </label>
                </div>

                {/* Item 5: Quota Restored Desktop Notification */}
                <div className="flex items-center justify-between py-3 border-b border-slate-100">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.refresh.notifyRestoredLabel')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.refresh.notifyRestoredDesc')}</p>
                  </div>
                  <label className="relative inline-flex items-center cursor-pointer">
                    <input
                      type="checkbox"
                      checked={autoRefreshSettings.notifyOnQuotaRestored}
                      onChange={(e) => handleUpdateAutoRefreshSettings({ notifyOnQuotaRestored: e.target.checked })}
                      className="sr-only peer"
                    />
                    <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
                  </label>
                </div>

                {/* Item 6: Notification on Auto-Refresh */}
                <div className="flex items-center justify-between py-3">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.refresh.notifyUpdateLabel')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.refresh.notifyUpdateDesc')}</p>
                  </div>
                  <label className="relative inline-flex items-center cursor-pointer">
                    <input
                      type="checkbox"
                      checked={autoRefreshSettings.notifyOnUpdate}
                      onChange={(e) => handleUpdateAutoRefreshSettings({ notifyOnUpdate: e.target.checked })}
                      className="sr-only peer"
                    />
                    <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
                  </label>
                </div>
              </div>

              {/* Trigger Settings Card */}
              <TriggerSettingsCard showToast={showToast} />

              {/* Recycle Bin Card */}
              <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-xs space-y-4">
                <div className="flex items-center justify-between pb-2 border-b border-slate-100">
                  <div className="flex items-center gap-2.5">
                    <div className="p-2 rounded-xl bg-slate-100 text-slate-700">
                      <Trash2 className="w-4 h-4" />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <h3 className="text-base font-bold text-slate-900">{t('trash.title')}</h3>
                        <span className="px-2 py-0.5 rounded-full text-[11px] font-semibold bg-slate-100 text-slate-600 border border-slate-200/80">
                          {trashList.length}
                        </span>
                      </div>
                      <p className="text-xs text-slate-500 mt-0.5">
                        {t('trash.desc')}
                      </p>
                    </div>
                  </div>

                  {trashList.length > 0 && (
                    <button
                      onClick={() => handlePurgeAccount()}
                      disabled={trashLoading}
                      className="px-3 py-1.5 rounded-xl border border-rose-200 bg-rose-50/50 hover:bg-rose-100/80 text-xs font-semibold text-rose-600 transition-all flex items-center gap-1.5 shadow-xs"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                      {t('trash.emptyButton')}
                    </button>
                  )}
                </div>

                {trashLoading && trashList.length === 0 ? (
                  <div className="py-6 text-center text-xs text-slate-400">Loading...</div>
                ) : trashList.length === 0 ? (
                  <div className="py-8 text-center text-xs text-slate-400 border border-dashed border-slate-200 rounded-xl bg-slate-50/50">
                    <Trash2 className="w-6 h-6 mx-auto mb-2 text-slate-300 opacity-60" />
                    {t('trash.emptyDesc')}
                  </div>
                ) : (
                  <div className="space-y-2.5">
                    {trashList.map((item) => (
                      <div
                        key={item.identity_key}
                        className="flex items-center justify-between p-3.5 rounded-xl border border-slate-100 bg-slate-50/60 hover:bg-slate-50 hover:border-slate-200 transition-all"
                      >
                        <div className="min-w-0 flex-1 pr-4">
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-semibold text-slate-900 truncate">
                              {item.display_name}
                            </span>
                            {item.plan && (
                              <span className="px-2 py-0.5 rounded-full text-[10px] font-bold bg-blue-50 text-blue-700 border border-blue-200/60 uppercase">
                                {item.plan}
                              </span>
                            )}
                            {!item.has_credentials && (
                              <span className="px-1.5 py-0.5 rounded text-[10px] bg-amber-50 text-amber-700 border border-amber-200">
                                {t('trash.reauthRequired')}
                              </span>
                            )}
                          </div>
                          <div className="flex items-center gap-3 text-xs text-slate-400 mt-1">
                            {item.email && item.email !== item.display_name && (
                              <span className="truncate">{item.email}</span>
                            )}
                            <span className="text-[11px]">
                              {t('trash.deletedAt')}: {item.removed_at ? item.removed_at.replace('T', ' ').split('.')[0] : '-'}
                            </span>
                          </div>
                        </div>

                        <div className="flex items-center gap-2 shrink-0">
                          <button
                            onClick={() => handleRestoreAccount(item)}
                            disabled={restoringId === item.identity_key}
                            title={t('trash.restore')}
                            className="px-3.5 py-1.5 rounded-xl border border-blue-200 bg-blue-50 hover:bg-blue-100 text-xs font-semibold text-blue-700 shadow-xs transition-all flex items-center gap-1.5 disabled:opacity-50"
                          >
                            <RotateCcw className={`w-3.5 h-3.5 ${restoringId === item.identity_key ? 'animate-spin' : ''}`} />
                            {restoringId === item.identity_key ? t('trash.restoring') : t('trash.restore')}
                          </button>
                          <button
                            onClick={() => handlePurgeAccount(item)}
                            disabled={purgingId === item.identity_key}
                            title={t('trash.purge')}
                            className="p-1.5 rounded-xl text-slate-400 hover:text-rose-600 hover:bg-rose-50 transition-colors"
                          >
                            <Trash2 className="w-4 h-4" />
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              {/* Data & Process Management Card */}
              <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-xs space-y-4">
                <h3 className="text-base font-bold text-slate-900">{t('settings.dataProcess.title')}</h3>
                <div className="flex items-center justify-between py-3 border-b border-slate-100">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.dataProcess.restartCodex')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.dataProcess.restartCodexDesc')}</p>
                  </div>
                  <button
                    onClick={handleRestartCodex}
                    disabled={restartingCodex}
                    className="px-3.5 py-1.5 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-xs font-semibold text-slate-700 shadow-xs transition-all flex items-center gap-1.5 disabled:opacity-50"
                  >
                    <RotateCcw className={`w-3.5 h-3.5 ${restartingCodex ? 'animate-spin text-blue-600' : ''}`} />
                    <span>{restartingCodex ? t('app.status.restarting') : t('app.actions.restartCodex')}</span>
                  </button>
                </div>

                <div className="flex items-center justify-between py-3 border-b border-slate-100">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.dataProcess.resetAliases')}</h4>
                    <p className="text-xs text-slate-500">{t('settings.dataProcess.resetAliasesDesc')}</p>
                  </div>
                  <button
                    onClick={() => setShowResetModal(true)}
                    className="px-3.5 py-1.5 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-xs font-semibold text-slate-700 shadow-xs transition-all"
                  >
                    {t('settings.dataProcess.resetAliases')}
                  </button>
                </div>

                <div className="flex items-center justify-between py-3">
                  <div>
                    <h4 className="text-sm font-semibold text-slate-800">{t('settings.dataProcess.storageDir')}</h4>
                    <p className="text-xs text-slate-500 font-mono">~/.codexq</p>
                  </div>
                  <span className="text-xs text-slate-400 font-medium">{t('settings.dataProcess.storageDirDesc')}</span>
                </div>
              </div>

              {/* System Diagnostics (Doctor) Card */}
              <DoctorCard />

              {/* About CodexQ Card */}
              <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-xs space-y-2">
                <h3 className="text-base font-bold text-slate-900">{t('settings.about.title')}</h3>
                <p className="text-xs text-slate-600 leading-relaxed">
                  {t('settings.about.desc')}
                </p>
                <div className="pt-2 text-[11px] text-slate-400 flex items-center gap-4 font-mono">
                  <span>{t('settings.about.version')}: v1.0.2</span>
                  <span>UI: Clash Verge Light</span>
                </div>
              </div>
            </div>
        </main>
      </div>

      {/* Modals */}
      <HistoryModal
        account={historyAccount}
        isOpen={!!historyAccount}
        onClose={() => setHistoryAccount(null)}
      />

      <AliasModal
        account={aliasAccount}
        isOpen={!!aliasAccount}
        onClose={() => setAliasAccount(null)}
        onSaved={fetchAccounts}
      />

      <ResetConfirmModal
        isOpen={showResetModal}
        onClose={() => setShowResetModal(false)}
        onConfirm={handleResetAllAliases}
        loading={resetting}
      />

      <RemoveConfirmModal
        isOpen={!!removeTargetAccount}
        account={removeTargetAccount}
        onClose={() => setRemoveTargetAccount(null)}
        onConfirm={handleConfirmRemove}
        loading={removing}
      />

      <AddAccountModal
        isOpen={showAddAccountModal}
        onClose={() => setShowAddAccountModal(false)}
        onSuccess={() => {
          fetchAccounts();
          handleRefreshAll();
        }}
        showToast={showToast}
      />
    </div>
  );
}

export default App;
