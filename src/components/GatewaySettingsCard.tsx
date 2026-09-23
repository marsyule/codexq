import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import type { ActiveRuntimeMode, GatewayStatus, ToastPayload } from '../types';
import { Server, RefreshCw, Loader2, AlertTriangle } from 'lucide-react';

interface GatewaySettingsCardProps {
  /** Callback used to surface successes and failures as app-level toasts. */
  showToast: (msg: ToastPayload, type?: 'success' | 'error' | 'info') => void;
  /** Currently active runtime slot; changes force a status refresh. */
  activeMode: ActiveRuntimeMode | null;
}

/** Availability of a port as reported by the backend probe. */
type PortProbeState = 'running' | 'free' | 'occupied';

/**
 * Settings card for the loopback protocol gateway (enable switch, listen port, availability).
 *
 * @remarks
 * The port is a **global** setting, not a per-provider one: the gateway is a process-wide
 * singleton, so a single listener serves every provider. The card therefore owns the only
 * editable port field in the app — the provider editor shows the same values read-only.
 *
 * Applying a port is atomic on the backend: it persists the value, restarts the listener and
 * rewrites the address in Codex's `config.toml`, rolling back if any step fails. The UI never
 * shows a loopback address as usable without asking the backend for its real binding state.
 *
 * @param props - Component props object.
 * @param props.showToast - Toast dispatcher owned by the app shell.
 * @param props.activeMode - Active runtime slot, used to re-poll the status after switches.
 */
export const GatewaySettingsCard: React.FC<GatewaySettingsCardProps> = ({ showToast, activeMode }) => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<GatewayStatus | null>(null);
  const [portInput, setPortInput] = useState<string>('');
  const [busy, setBusy] = useState<boolean>(false);
  const [probing, setProbing] = useState<boolean>(false);
  const [probe, setProbe] = useState<{ port: number; state: PortProbeState } | null>(null);

  /** Reloads the gateway status from the backend. */
  const refresh = useCallback(async () => {
    try {
      const res = await invoke<GatewayStatus>('get_gateway_status');
      setStatus(res);
      setPortInput(String(res.config_port));
      setProbe(null);
    } catch (err) {
      console.warn('Failed to load gateway status', err);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh, activeMode]);

  const parsedPort = Number.parseInt(portInput, 10);

  const describeState = (state: PortProbeState | undefined): { label: string; className: string } => {
    switch (state) {
      case 'running':
        return { label: t('settings.gateway.statusRunning', '运行中'), className: 'bg-blue-50 text-blue-700 border-blue-200/80' };
      case 'free':
        return { label: t('settings.gateway.statusFree', '可用'), className: 'bg-emerald-50 text-emerald-700 border-emerald-200/80' };
      case 'occupied':
        return { label: t('settings.gateway.statusOccupied', '被占用'), className: 'bg-rose-50 text-rose-700 border-rose-200/80' };
      default:
        return { label: t('settings.gateway.statusStopped', '未运行'), className: 'bg-slate-100 text-slate-500 border-slate-200/80' };
    }
  };

  const effectiveState: PortProbeState | undefined = probe
    ? probe.state
    : (status?.port_state as PortProbeState | undefined);
  const stateStyle = describeState(effectiveState);

  const handleToggle = async (enabled: boolean) => {
    setBusy(true);
    try {
      const res = await invoke<GatewayStatus>('set_gateway_enabled', { enabled });
      setStatus(res);
      setProbe(null);
      showToast(
        enabled
          ? t('settings.gateway.gatewayEnabled', '已启用本地协议网关')
          : t('settings.gateway.gatewayDisabled', '已关闭本地协议网关'),
        'success'
      );
    } catch (err) {
      showToast(typeof err === 'string' ? err : String(err), 'error');
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const handleCheck = async () => {
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      showToast(t('settings.gateway.portRange', '端口必须在 1-65535 之间'), 'error');
      return;
    }
    setProbing(true);
    try {
      const res = await invoke<{ port: number; state: PortProbeState }>('check_gateway_port', { port: parsedPort });
      setProbe(res);
      const label = describeState(res.state).label;
      showToast(`${res.port} · ${label}`, res.state === 'occupied' ? 'error' : 'success');
    } catch (err) {
      showToast(typeof err === 'string' ? err : String(err), 'error');
    } finally {
      setProbing(false);
    }
  };

  const handleApply = async () => {
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      showToast(t('settings.gateway.portRange', '端口必须在 1-65535 之间'), 'error');
      return;
    }
    setBusy(true);
    try {
      const res = await invoke<GatewayStatus>('set_gateway_port', { port: parsedPort });
      setStatus(res);
      setPortInput(String(res.config_port));
      setProbe(null);
      showToast(t('settings.gateway.portApplied', { port: res.port, defaultValue: '端口已切换为 {{port}}' }), 'success');
    } catch (err) {
      showToast(typeof err === 'string' ? err : String(err), 'error');
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  const envOverride = status?.env_override === true;

  return (
    <div className="rounded-2xl border border-slate-200 bg-white p-6 shadow-xs space-y-4">
      <div className="flex items-center justify-between pb-2 border-b border-slate-100">
        <div className="flex items-center gap-2.5">
          <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
            <Server className="w-4 h-4" />
          </div>
          <div>
            <h3 className="text-base font-bold text-slate-900">{t('settings.gateway.title', '本地协议网关')}</h3>
            <p className="text-xs text-slate-500 mt-0.5">
              {t('settings.gateway.desc', '把 Chat Completions 上游经本机回环网关转换为 Responses')}
            </p>
          </div>
        </div>
        <span className={`px-2 py-0.5 rounded-full text-[11px] font-semibold border shrink-0 ${status?.running ? 'bg-blue-50 text-blue-700 border-blue-200/80' : 'bg-slate-100 text-slate-500 border-slate-200/80'}`}>
          {status?.running
            ? t('settings.gateway.statusRunning', '运行中')
            : t('settings.gateway.statusStopped', '未运行')}
        </span>
      </div>

      {/* Global enable switch */}
      <div className="flex items-center justify-between py-3 border-b border-slate-100">
        <div>
          <h4 className="text-sm font-semibold text-slate-800">{t('settings.gateway.enableLabel', '启用本地协议网关')}</h4>
          <p className="text-xs text-slate-500">{t('settings.gateway.enableDesc')}</p>
        </div>
        <label className="relative inline-flex items-center cursor-pointer shrink-0">
          <input
            type="checkbox"
            checked={status?.enabled === true}
            disabled={busy || !status}
            onChange={(e) => handleToggle(e.target.checked)}
            className="sr-only peer"
          />
          <div className="w-11 h-6 bg-slate-200 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-slate-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
        </label>
      </div>

      {/* Listen port + availability */}
      <div className="py-3 border-b border-slate-100 space-y-2">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="min-w-0">
            <h4 className="text-sm font-semibold text-slate-800">{t('settings.gateway.portLabel', '监听端口')}</h4>
            <p className="text-xs text-slate-500">{t('settings.gateway.portDesc')}</p>
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            <input
              type="number"
              min={1}
              max={65535}
              value={envOverride ? String(status?.port ?? '') : portInput}
              disabled={envOverride || busy}
              onChange={(e) => setPortInput(e.target.value)}
              className="w-24 rounded-lg border border-slate-200 bg-white py-1.5 px-2 text-xs text-center font-mono text-slate-800 focus:border-blue-500 focus:outline-none disabled:bg-slate-100 disabled:text-slate-500"
            />
            <span className={`px-2 py-0.5 rounded-full text-[11px] font-semibold border ${stateStyle.className}`}>
              {stateStyle.label}
            </span>
            <button
              type="button"
              onClick={handleCheck}
              disabled={envOverride || probing || !status}
              className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium text-slate-700 bg-white hover:bg-slate-50 border border-slate-200 shadow-2xs transition-all disabled:opacity-40"
            >
              {probing ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
              <span>{t('settings.gateway.check', '检测')}</span>
            </button>
            <button
              type="button"
              onClick={handleApply}
              disabled={envOverride || busy || !status}
              className="inline-flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 shadow-2xs transition-all disabled:opacity-40"
            >
              {busy && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
              <span>{t('settings.gateway.apply', '应用')}</span>
            </button>
          </div>
        </div>

        {envOverride ? (
          <p className="flex items-start gap-1.5 text-[11px] text-amber-700 bg-amber-50 border border-amber-200/80 rounded-lg p-2">
            <AlertTriangle className="w-3.5 h-3.5 shrink-0 mt-px" />
            <span>{t('settings.gateway.envOverride', { port: status?.port, defaultValue: '端口由环境变量决定' })}</span>
          </p>
        ) : (
          effectiveState === 'occupied' && (
            <p className="flex items-start gap-1.5 text-[11px] text-rose-700 bg-rose-50 border border-rose-200/80 rounded-lg p-2">
              <AlertTriangle className="w-3.5 h-3.5 shrink-0 mt-px" />
              <span>{t('settings.gateway.occupiedHint')}</span>
            </p>
          )
        )}
      </div>

      {/* Address actually written into Codex's config */}
      <div className="flex items-center justify-between py-3">
        <div>
          <h4 className="text-sm font-semibold text-slate-800">{t('settings.gateway.addressLabel', '写入 Codex 的地址')}</h4>
          <p className="text-xs text-slate-500">
            {status?.listen_port !== null && status?.listen_port !== undefined
              ? `127.0.0.1:${status.listen_port}`
              : t('settings.gateway.statusStopped', '未运行')}
          </p>
        </div>
        <code className="text-xs font-mono text-slate-700 bg-slate-50 border border-slate-200 rounded-lg px-2 py-1 shrink-0">
          {status?.base_url ?? '—'}
        </code>
      </div>
    </div>
  );
};

export default GatewaySettingsCard;