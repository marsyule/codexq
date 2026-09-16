import React, { useState, useEffect, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import type { AccountData, AccountAlarm, AccountRolloverConfig, ToastPayload } from '../types';
import {
  Plus,
  Trash2,
  Clock,
  ShieldCheck,
  AlertCircle,
  Play,
  Sparkles,
} from 'lucide-react';

interface SchedulerViewProps {
  accounts: AccountData[];
  onRefreshAccounts: () => void;
  showToast: (msg: ToastPayload, type?: 'success' | 'error' | 'info') => void;
}

/**
 * Cleans verbose CLI header output from Codex exec to extract the core error message.
 */
function formatWarmupError(raw: string): string {
  if (!raw) return 'Unknown error';

  const match = raw.match(/ERROR:\s*(.+)/i);
  if (match && match[1]) {
    let clean = match[1].trim();
    const subIdx = clean.indexOf('ERROR:');
    if (subIdx !== -1) {
      clean = clean.slice(0, subIdx).trim();
    }
    return clean;
  }

  const bannerEnd = raw.indexOf('--------');
  if (bannerEnd !== -1) {
    const afterFirst = raw.slice(bannerEnd + 8);
    const secondBanner = afterFirst.indexOf('--------');
    if (secondBanner !== -1) {
      return afterFirst.slice(secondBanner + 8).trim();
    }
    return afterFirst.trim();
  }

  return raw.trim();
}

export const SchedulerView: React.FC<SchedulerViewProps> = ({
  accounts,
  onRefreshAccounts,
  showToast,
}) => {
  const { t } = useTranslation();
  // Selected account for alarms view (defaults to current active account on application launch)
  const [selectedKey, setSelectedKey] = useState<string>(() => {
    const active = accounts.find((a) => a.is_current);
    return active ? active.identity_key : accounts[0]?.identity_key || '';
  });

  // Keep selectedKey in sync if accounts load after initial mount or selected account is removed
  useEffect(() => {
    if (accounts.length > 0) {
      if (!selectedKey || !accounts.some((a) => a.identity_key === selectedKey)) {
        const active = accounts.find((a) => a.is_current);
        setSelectedKey(active ? active.identity_key : accounts[0].identity_key);
      }
    }
  }, [accounts, selectedKey]);

  // Alarms state
  const [alarms, setAlarms] = useState<AccountAlarm[]>([]);
  const [loadingAlarms, setLoadingAlarms] = useState<boolean>(false);
  const [defaultModel, setDefaultModel] = useState<string>('gpt-5.6-luna');

  // Account Rollover state
  const [rolloverEnabled, setRolloverEnabled] = useState<boolean>(false);
  const [minWeeklyRemaining, setMinWeeklyRemaining] = useState<number>(0);
  const [savingRollover, setSavingRollover] = useState<boolean>(false);

  // Add alarm form state
  const [showAddAlarm, setShowAddAlarm] = useState<boolean>(false);
  const [newAlarmTime, setNewAlarmTime] = useState<string>('08:00');
  const [newAlarmDays, setNewAlarmDays] = useState<string>('1,2,3,4,5');
  const [newAlarmModel, setNewAlarmModel] = useState<string>('');
  const [alarmFormError, setAlarmFormError] = useState<string | null>(null);

  // Test trigger action state
  const [testingTrigger, setTestingTrigger] = useState<boolean>(false);

  // Load default model for display
  useEffect(() => {
    invoke<Record<string, string>>('get_app_settings')
      .then((res) => {
        if (res && res['warmup.default_model']) {
          setDefaultModel(res['warmup.default_model']);
        }
      })
      .catch((err) => console.warn('Failed to load settings', err));
  }, []);

  // Fetch account rollover config on selected account change
  const fetchRolloverConfig = useCallback(async () => {
    if (!selectedKey) return;
    try {
      const cfg = await invoke<AccountRolloverConfig>('get_account_rollover', { identityKey: selectedKey });
      if (cfg) {
        setRolloverEnabled(cfg.enabled);
        setMinWeeklyRemaining(cfg.min_weekly_remaining);
      }
    } catch (err) {
      console.warn('Failed to load account rollover config', err);
    }
  }, [selectedKey]);

  useEffect(() => {
    fetchRolloverConfig();
  }, [fetchRolloverConfig]);

  const handleToggleRollover = async (enabled: boolean) => {
    if (!selectedKey) return;
    setSavingRollover(true);
    try {
      await invoke('save_account_rollover', {
        identityKey: selectedKey,
        enabled,
        minWeeklyRemaining,
      });
      setRolloverEnabled(enabled);
      showToast(enabled ? { key: 'toasts.autoRolloverEnabled' } : { key: 'toasts.autoRolloverDisabled' });
    } catch (err) {
      console.error('Failed to update account rollover', err);
      showToast(`保存失败: ${err}`, 'error');
    } finally {
      setSavingRollover(false);
    }
  };

  const handleUpdateMinRemaining = async (val: number) => {
    if (!selectedKey) return;
    const clamped = Math.min(100, Math.max(0, val));
    setMinWeeklyRemaining(clamped);
    setSavingRollover(true);
    try {
      await invoke('save_account_rollover', {
        identityKey: selectedKey,
        enabled: rolloverEnabled,
        minWeeklyRemaining: clamped,
      });
      showToast({ key: 'toasts.autoRolloverSaved' }, 'success');
    } catch (err) {
      console.error('Failed to update account rollover threshold', err);
      showToast(`保存失败: ${err}`, 'error');
    } finally {
      setSavingRollover(false);
    }
  };

  // Load alarms from backend
  const fetchAlarms = useCallback(async () => {
    if (!selectedKey) return;
    setLoadingAlarms(true);
    try {
      const res = await invoke<AccountAlarm[]>('list_account_alarms', { target: selectedKey });
      setAlarms(res || []);
    } catch (err: any) {
      console.warn('Failed to load alarms', err);
    } finally {
      setLoadingAlarms(false);
    }
  }, [selectedKey]);

  useEffect(() => {
    fetchAlarms();
  }, [fetchAlarms]);

  // Selected account data
  const selectedAccount = useMemo(() => {
    return accounts.find((a) => a.identity_key === selectedKey);
  }, [accounts, selectedKey]);

  // Weekly quota remaining percent (0.0 - 100.0)
  const secondaryRemaining = useMemo(() => {
    if (selectedAccount?.secondary?.used_percent != null) {
      return Math.max(0, Math.min(100, 100 - selectedAccount.secondary.used_percent));
    }
    return null;
  }, [selectedAccount]);

  // Convert time "HH:MM" to minutes
  const timeToMinutes = (t: string): number => {
    const parts = t.split(':').map(Number);
    return (parts[0] || 0) * 60 + (parts[1] || 0);
  };

  // Check non-overlapping conflict (>= 300 minutes / 5 hours)
  const validateOverlap = (candidateTime: string, excludeId?: string): { conflict: boolean; conflictTime?: string; diffHours?: string } => {
    const candMin = timeToMinutes(candidateTime);
    for (const a of alarms) {
      const isEnabled = typeof a.enabled === 'boolean' ? a.enabled : a.enabled === 1;
      if (!isEnabled || a.id === excludeId) continue;
      const existMin = timeToMinutes(a.time_of_day);
      let diff = Math.abs(candMin - existMin);
      if (diff > 12 * 60) diff = 24 * 60 - diff;
      if (diff < 5 * 60) {
        return {
          conflict: true,
          conflictTime: a.time_of_day,
          diffHours: (diff / 60).toFixed(1),
        };
      }
    }
    return { conflict: false };
  };

  // Overall account overlap status
  const overallOverlapStatus = useMemo(() => {
    const activeAlarms = alarms.filter((a) => (typeof a.enabled === 'boolean' ? a.enabled : a.enabled === 1));
    if (activeAlarms.length <= 1) {
      return { hasConflict: false, message: '闹钟配置良好，无时间冲突' };
    }
    for (let i = 0; i < activeAlarms.length; i++) {
      for (let j = i + 1; j < activeAlarms.length; j++) {
        let diff = Math.abs(timeToMinutes(activeAlarms[i].time_of_day) - timeToMinutes(activeAlarms[j].time_of_day));
        if (diff > 12 * 60) diff = 24 * 60 - diff;
        if (diff < 5 * 60) {
          return {
            hasConflict: true,
            message: `防重叠冲突：闹钟 ${activeAlarms[i].time_of_day} 与 ${activeAlarms[j].time_of_day} 间隔仅 ${(diff / 60).toFixed(1)} 小时（需 ≥ 5 小时）`,
          };
        }
      }
    }
    return { hasConflict: false, message: '闹钟配置良好，无时间冲突' };
  }, [alarms]);

  // Toggle alarm enabled
  const handleToggleAlarm = async (alarm: AccountAlarm) => {
    const currentEnabled = typeof alarm.enabled === 'boolean' ? alarm.enabled : alarm.enabled === 1;
    const targetState = !currentEnabled;

    if (targetState) {
      const check = validateOverlap(alarm.time_of_day, alarm.id);
      if (check.conflict) {
        showToast({ key: 'scheduler.intervalError' }, 'error');
        return;
      }
    }

    try {
      await invoke('save_account_alarm', {
        alarm: {
          ...alarm,
          enabled: targetState ? 1 : 0,
        },
      });
      await fetchAlarms();
      showToast({ key: 'toasts.alarmSaved' }, 'success');
    } catch (err: any) {
      showToast(`更新失败: ${err}`, 'error');
    }
  };

  // Delete alarm
  const handleDeleteAlarm = async (alarmId: string) => {
    try {
      await invoke('delete_account_alarm', { id: alarmId, alarmId });
      await fetchAlarms();
      showToast({ key: 'toasts.alarmDeleted' }, 'success');
    } catch (err: any) {
      showToast(`删除失败: ${err}`, 'error');
    }
  };

  // Save new alarm
  const handleSaveNewAlarm = async () => {
    if (!selectedKey) return;
    setAlarmFormError(null);

    const check = validateOverlap(newAlarmTime);
    if (check.conflict) {
      setAlarmFormError(t('scheduler.intervalError'));
      return;
    }

    try {
      await invoke('save_account_alarm', {
        alarm: {
          identity_key: selectedKey,
          time_of_day: newAlarmTime,
          days_of_week: newAlarmDays,
          enabled: 1,
          model_override: newAlarmModel.trim() || null,
        },
      });
      setShowAddAlarm(false);
      setNewAlarmModel('');
      await fetchAlarms();
      showToast({ key: 'toasts.alarmSaved' }, 'success');
    } catch (err: any) {
      setAlarmFormError(typeof err === 'string' ? err : err?.message || '保存失败');
    }
  };

  // Test trigger now
  const handleTestTrigger = async () => {
    if (!selectedKey) return;
    setTestingTrigger(true);
    try {
      const res: any = await invoke('trigger_warmup', {
        target: selectedKey,
        force: true,
      });

      const item = Array.isArray(res) ? res[0] : res;
      if (item?.status === 'skipped') {
        showToast({ key: 'toasts.testTriggerSkipped' }, 'info');
      } else if (item?.status === 'success') {
        showToast({ key: 'toasts.testTriggerSuccess' }, 'success');
        onRefreshAccounts();
      } else {
        const rawErr = item?.error || item?.message || 'Execution failed';
        const cleanErr = formatWarmupError(rawErr);
        showToast({ key: 'toasts.testTriggerFailed', params: { error: cleanErr } }, 'error');
      }
    } catch (err: any) {
      showToast({ key: 'toasts.testTriggerFailed', params: { error: String(err) } }, 'error');
    } finally {
      setTestingTrigger(false);
    }
  };

  return (
    <div className="space-y-4 max-w-4xl mx-auto">
      {/* Account Header & Action Bar */}
      <div className="rounded-2xl border border-slate-200 bg-white p-5 shadow-xs space-y-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-3 border-b border-slate-100">
          {/* Account Selector */}
          <div className="flex items-center gap-2.5 flex-wrap">
            <span className="text-xs font-semibold text-slate-500">{t('scheduler.accountSelectorLabel')}</span>
            <select
              value={selectedKey}
              onChange={(e) => setSelectedKey(e.target.value)}
              className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-1.5 text-xs font-bold text-slate-800 focus:border-blue-500 focus:bg-white focus:outline-none shadow-2xs"
            >
              {accounts.map((acc) => (
                <option key={acc.identity_key} value={acc.identity_key}>
                  {acc.display_name} {acc.is_current ? `★ (${t('accounts.activeBadge')})` : ''}
                </option>
              ))}
            </select>

            {selectedAccount?.plan && (
              <span className="text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded-full bg-blue-50 text-blue-700 border border-blue-200">
                {selectedAccount.plan}
              </span>
            )}
            {selectedAccount?.is_current && (
              <span className="text-[10px] font-bold px-2 py-0.5 rounded-full bg-emerald-50 text-emerald-700 border border-emerald-200">
                {t('accounts.activeBadge')}
              </span>
            )}
          </div>

          {/* Action Buttons */}
          <div className="flex items-center gap-2">
            <button
              onClick={handleTestTrigger}
              disabled={testingTrigger || !selectedKey}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold bg-amber-50 text-amber-800 border border-amber-200/80 hover:bg-amber-100 transition-colors shadow-2xs disabled:opacity-50"
              title={t('scheduler.testTrigger')}
            >
              <Play className={`w-3.5 h-3.5 fill-amber-500 ${testingTrigger ? 'animate-spin' : ''}`} />
              <span>{testingTrigger ? t('scheduler.testingTrigger') : t('scheduler.testTrigger')}</span>
            </button>
            <button
              onClick={() => {
                setShowAddAlarm(true);
                setAlarmFormError(null);
              }}
              className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg text-xs font-semibold bg-blue-600 text-white hover:bg-blue-700 transition-colors shadow-xs"
            >
              <Plus className="w-3.5 h-3.5" />
              <span>{t('scheduler.addAlarm')}</span>
            </button>
          </div>
        </div>

        {/* 5-Hour Restore Auto-Rollover Section */}
        <div className="rounded-xl border border-slate-200/80 bg-slate-50/50 p-4 space-y-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className="p-2 rounded-xl bg-emerald-50 text-emerald-600">
                <Sparkles className="w-4 h-4" />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <h4 className="text-sm font-bold text-slate-900">{t('scheduler.autoRolloverCardTitle')}</h4>
                  <span className="text-[10px] bg-emerald-50 text-emerald-700 border border-emerald-200/80 px-1.5 py-0.5 rounded font-bold">
                    {t('scheduler.autoRolloverCardTag')}
                  </span>
                  {savingRollover && (
                    <span className="text-[10px] bg-blue-50 text-blue-600 px-1.5 py-0.5 rounded font-medium animate-pulse">
                      ...
                    </span>
                  )}
                </div>
                <p className="text-xs text-slate-500 mt-0.5">{t('scheduler.autoRolloverCardDesc')}</p>
              </div>
            </div>

            <label className="relative inline-flex items-center cursor-pointer">
              <input
                type="checkbox"
                checked={rolloverEnabled}
                onChange={(e) => handleToggleRollover(e.target.checked)}
                disabled={!selectedKey || savingRollover}
                className="sr-only peer"
              />
              <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
            </label>
          </div>

          {rolloverEnabled && (
            <div className="p-3.5 bg-white rounded-xl border border-slate-200/80 space-y-3 text-xs animate-in fade-in duration-150">
              <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                <div>
                  <span className="font-semibold text-slate-700">{t('scheduler.autoRolloverMinWeeklyRemaining')}</span>
                  <p className="text-[11px] text-slate-500">{t('scheduler.autoRolloverMinWeeklyRemainingDesc')}</p>
                </div>
                <div className="flex items-center gap-1.5 shrink-0">
                  <span className="text-slate-500 text-xs font-mono font-bold">≥</span>
                  <input
                    type="number"
                    min="0"
                    max="100"
                    value={minWeeklyRemaining}
                    onChange={(e) => {
                      const n = parseFloat(e.target.value);
                      setMinWeeklyRemaining(isNaN(n) ? 0 : Math.min(100, Math.max(0, n)));
                    }}
                    onBlur={(e) => {
                      const n = parseFloat(e.target.value);
                      handleUpdateMinRemaining(isNaN(n) ? 0 : n);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        const n = parseFloat((e.target as HTMLInputElement).value);
                        handleUpdateMinRemaining(isNaN(n) ? 0 : n);
                      }
                    }}
                    className="w-16 px-2 py-1 text-xs rounded-lg border border-slate-300 bg-white font-mono font-bold text-center focus:border-blue-500 focus:outline-none shadow-2xs"
                  />
                  <span className="text-slate-500 text-xs font-medium">%</span>
                </div>
              </div>

              {/* Current Weekly Quota Health Indicator */}
              <div className="pt-2 border-t border-slate-100 flex items-center justify-between flex-wrap gap-2 text-[11px]">
                <span className="text-slate-500">
                  {t('scheduler.autoRolloverCurrentRemaining')}:{' '}
                  <span className="font-bold font-mono text-slate-800">
                    {secondaryRemaining !== null ? `${secondaryRemaining.toFixed(1)}%` : t('scheduler.autoRolloverStatusUnlimited')}
                  </span>
                </span>
                <span
                  className={`font-semibold px-2 py-0.5 rounded-full border ${
                    secondaryRemaining === null || secondaryRemaining > minWeeklyRemaining
                      ? 'bg-emerald-50 text-emerald-700 border-emerald-200'
                      : secondaryRemaining <= 0
                      ? 'bg-rose-50 text-rose-700 border-rose-200'
                      : 'bg-amber-50 text-amber-700 border-amber-200'
                  }`}
                >
                  {secondaryRemaining === null
                    ? t('scheduler.autoRolloverStatusUnlimited')
                    : secondaryRemaining <= 0
                    ? t('scheduler.autoRolloverStatusExhausted')
                    : secondaryRemaining < minWeeklyRemaining
                    ? t('scheduler.autoRolloverStatusBelowMin')
                    : t('scheduler.autoRolloverStatusOk')}
                </span>
              </div>
            </div>
          )}
        </div>

        {/* Overlap Status Bar */}
        <div
          className={`flex items-center justify-between px-3.5 py-2 rounded-xl border text-xs transition-colors ${
            overallOverlapStatus.hasConflict
              ? 'bg-rose-50 border-rose-200 text-rose-800'
              : 'bg-slate-50/80 border-slate-200/70 text-slate-600'
          }`}
        >
          <div className="flex items-center gap-2">
            {overallOverlapStatus.hasConflict ? (
              <AlertCircle className="w-4 h-4 text-rose-600 shrink-0" />
            ) : (
              <ShieldCheck className="w-4 h-4 text-emerald-600 shrink-0" />
            )}
            <span className="font-medium">{overallOverlapStatus.message}</span>
          </div>
          <div className="flex items-center gap-2.5 shrink-0">
            <span
              className={`text-[10px] font-semibold px-2 py-0.5 rounded-full border transition-all ${
                rolloverEnabled
                  ? 'bg-emerald-50 text-emerald-700 border-emerald-200'
                  : 'bg-slate-100 text-slate-400 border-slate-200/70'
              }`}
            >
              {rolloverEnabled ? `● ${t('scheduler.autoRolloverActive')}` : `○ ${t('scheduler.autoRolloverInactive')}`}
            </span>
            <span className="text-[11px] text-slate-400 font-mono">窗口: 300m (5h)</span>
          </div>
        </div>

        {/* Inline Add Alarm Form */}
        {showAddAlarm && (
          <div className="p-4 rounded-xl border border-blue-200 bg-blue-50/30 space-y-3 animate-in fade-in duration-150">
            <div className="flex items-center justify-between pb-1 border-b border-blue-100">
              <span className="text-xs font-bold text-blue-900">{t('scheduler.addAlarm')}</span>
              <button
                onClick={() => setShowAddAlarm(false)}
                className="text-xs text-slate-400 hover:text-slate-600 font-bold"
              >
                ✕
              </button>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              <div>
                <label className="block text-[11px] font-medium text-slate-600 mb-1">{t('scheduler.table.time')}</label>
                <input
                  type="time"
                  value={newAlarmTime}
                  onChange={(e) => {
                    setNewAlarmTime(e.target.value);
                    const check = validateOverlap(e.target.value);
                    if (check.conflict) {
                      setAlarmFormError(t('scheduler.intervalError'));
                    } else {
                      setAlarmFormError(null);
                    }
                  }}
                  className="w-full px-2.5 py-1.5 text-xs rounded-lg border border-slate-300 bg-white font-mono font-bold focus:border-blue-500 focus:outline-none"
                />
              </div>
              <div>
                <label className="block text-[11px] font-medium text-slate-600 mb-1">{t('scheduler.table.days')}</label>
                <select
                  value={newAlarmDays}
                  onChange={(e) => setNewAlarmDays(e.target.value)}
                  className="w-full px-2.5 py-1.5 text-xs rounded-lg border border-slate-300 bg-white font-medium focus:border-blue-500 focus:outline-none"
                >
                  <option value="once">{t('scheduler.days.once')}</option>
                  <option value="1,2,3,4,5">{t('scheduler.days.workdays')}</option>
                  <option value="1,2,3,4,5,6,7">{t('scheduler.days.all')}</option>
                </select>
              </div>
              <div>
                <label className="block text-[11px] font-medium text-slate-600 mb-1">
                  Model
                </label>
                <input
                  type="text"
                  placeholder={`${defaultModel}`}
                  value={newAlarmModel}
                  onChange={(e) => setNewAlarmModel(e.target.value)}
                  className="w-full px-2.5 py-1.5 text-xs rounded-lg border border-slate-300 bg-white font-mono focus:border-blue-500 focus:outline-none"
                />
              </div>
            </div>

            {alarmFormError && (
              <div className="text-xs text-rose-700 bg-rose-50 p-2 rounded-lg border border-rose-200 flex items-center gap-1.5 font-medium">
                <AlertCircle className="w-3.5 h-3.5 text-rose-600 shrink-0" />
                <span>{alarmFormError}</span>
              </div>
            )}

            <div className="flex justify-end gap-2 pt-1">
              <button
                onClick={() => setShowAddAlarm(false)}
                className="px-3 py-1.5 rounded-lg text-xs font-semibold text-slate-600 hover:bg-slate-100"
              >
                {t('modals.alias.cancel')}
              </button>
              <button
                onClick={handleSaveNewAlarm}
                className="px-3.5 py-1.5 rounded-lg text-xs font-semibold bg-blue-600 text-white hover:bg-blue-700 shadow-xs"
              >
                {t('modals.alias.save')}
              </button>
            </div>
          </div>
        )}

        {/* Alarms List */}
        {loadingAlarms ? (
          <div className="py-8 text-center text-xs text-slate-400">{t('app.status.refreshing')}</div>
        ) : alarms.length === 0 ? (
          <div className="text-center py-10 border border-dashed border-slate-200 rounded-xl space-y-2 bg-slate-50/40">
            <Clock className="w-7 h-7 text-slate-300 mx-auto" />
            <p className="text-xs text-slate-500 font-medium">{t('scheduler.emptyTitle')}</p>
            <button
              onClick={() => {
                setShowAddAlarm(true);
                setAlarmFormError(null);
              }}
              className="inline-flex items-center gap-1 px-3 py-1 rounded-lg text-xs font-medium text-blue-600 bg-blue-50 hover:bg-blue-100 transition-colors"
            >
              <Plus className="w-3.5 h-3.5" />
              <span>{t('scheduler.addAlarm')}</span>
            </button>
          </div>
        ) : (
          <div className="space-y-2.5">
            {alarms.map((alarm) => {
              const isEnabled = typeof alarm.enabled === 'boolean' ? alarm.enabled : alarm.enabled === 1;
              const activeModel = alarm.model_override || defaultModel;

              return (
                <div
                  key={alarm.id}
                  className={`flex items-center justify-between p-3.5 rounded-xl border transition-all ${
                    isEnabled
                      ? 'bg-white border-slate-200 shadow-2xs hover:border-blue-200'
                      : 'bg-slate-50/70 border-slate-200/60 opacity-60'
                  }`}
                >
                  <div className="flex items-center gap-3.5">
                    <div
                      className={`p-2 rounded-xl ${
                        isEnabled ? 'bg-blue-50 text-blue-600' : 'bg-slate-100 text-slate-400'
                      }`}
                    >
                      <Clock className="w-4 h-4" />
                    </div>

                    <div>
                      <div className="flex items-center gap-2">
                        <span className="text-base font-bold font-mono text-slate-900 tracking-tight">
                          {alarm.time_of_day}
                        </span>

                        <span
                          className={`text-[10px] font-semibold px-2 py-0.5 rounded-full border ${
                            alarm.days_of_week === 'once'
                              ? 'bg-amber-50 text-amber-700 border-amber-200'
                              : alarm.days_of_week === '1,2,3,4,5'
                              ? 'bg-indigo-50 text-indigo-700 border-indigo-200'
                              : 'bg-purple-50 text-purple-700 border-purple-200'
                          }`}
                        >
                          {alarm.days_of_week === 'once'
                            ? t('scheduler.days.once')
                            : alarm.days_of_week === '1,2,3,4,5'
                            ? t('scheduler.days.workdays')
                            : t('scheduler.days.all')}
                        </span>

                        <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-slate-100 text-slate-600 border border-slate-200">
                          {alarm.model_override ? `${alarm.model_override}` : `${activeModel}`}
                        </span>
                      </div>

                      {alarm.last_status && (
                        <div className="flex items-center gap-2 text-[11px] text-slate-400 mt-0.5">
                          <span>
                            {t('scheduler.table.lastRun')}:{' '}
                            <span
                              className={`font-medium ${
                                alarm.last_status === 'success'
                                  ? 'text-emerald-600'
                                  : alarm.last_status === 'skipped'
                                  ? 'text-amber-600'
                                  : 'text-rose-600'
                              }`}
                            >
                              {alarm.last_status === 'success'
                                ? t('scheduler.table.success')
                                : alarm.last_status === 'skipped'
                                ? t('scheduler.table.skipped')
                                : t('scheduler.table.failed')}
                            </span>
                          </span>
                          {alarm.last_triggered_at && (
                            <span>({alarm.last_triggered_at.replace('T', ' ').split('.')[0]})</span>
                          )}
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-3">
                    {/* Toggle Switch */}
                    <label className="relative inline-flex items-center cursor-pointer">
                      <input
                        type="checkbox"
                        checked={isEnabled}
                        onChange={() => handleToggleAlarm(alarm)}
                        className="sr-only peer"
                      />
                      <div className="w-9 h-5 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-blue-600"></div>
                    </label>

                    {/* Delete Button */}
                    <button
                      onClick={() => handleDeleteAlarm(alarm.id)}
                      className="p-1.5 text-slate-400 hover:text-rose-600 hover:bg-rose-50 rounded-lg transition-colors"
                      title={t('scheduler.table.actions')}
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
};
