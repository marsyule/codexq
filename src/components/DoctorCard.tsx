import React, { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import {
  Activity,
  CheckCircle2,
  AlertTriangle,
  AlertCircle,
  RefreshCw,
  Terminal,
  ShieldCheck,
  Database,
  Globe2,
} from 'lucide-react';

/**
 * Single diagnostic item returned from `run_diagnostics` command.
 */
interface DiagnosticItem {
  key: string;
  title: string;
  status: 'ok' | 'warning' | 'error';
  message: string;
  detail?: string | null;
}

/**
 * Structured diagnostic report returned from Rust `doctor::run_diagnostics`.
 */
interface DiagnosticsReport {
  items: DiagnosticItem[];
  overall_healthy: boolean;
  timestamp: string;
}

/**
 * View card component providing on-demand and startup environment diagnostics.
 *
 * Checks Codex CLI accessibility, configuration store sandbox enforcement,
 * local SQLite database health, and OpenAI Auth connectivity.
 *
 * @returns Rendered Diagnostics Card component.
 */
export const DoctorCard: React.FC = () => {
  const { t } = useTranslation();
  const [report, setReport] = useState<DiagnosticsReport | null>(null);
  const [loading, setLoading] = useState<boolean>(false);

  const runCheck = useCallback(async () => {
    setLoading(true);
    try {
      const res = await invoke<DiagnosticsReport>('run_diagnostics');
      setReport(res);
    } catch (err) {
      console.error('Failed to run diagnostics:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    runCheck();
  }, [runCheck]);

  const getItemIcon = (key: string) => {
    switch (key) {
      case 'codex_cli':
        return <Terminal className="h-4 w-4 text-slate-500" />;
      case 'credentials_store':
        return <ShieldCheck className="h-4 w-4 text-slate-500" />;
      case 'storage_health':
        return <Database className="h-4 w-4 text-slate-500" />;
      case 'network_connectivity':
        return <Globe2 className="h-4 w-4 text-slate-500" />;
      default:
        return <Activity className="h-4 w-4 text-slate-500" />;
    }
  };

  const getStatusBadge = (status: 'ok' | 'warning' | 'error') => {
    if (status === 'ok') {
      return (
        <span className="inline-flex items-center gap-1 rounded-full bg-emerald-50 px-2 py-0.5 text-[10px] font-semibold text-emerald-700 border border-emerald-200/60">
          <CheckCircle2 className="h-3 w-3 text-emerald-600" />
          <span>{t('doctor.statusOk')}</span>
        </span>
      );
    }
    if (status === 'warning') {
      return (
        <span className="inline-flex items-center gap-1 rounded-full bg-amber-50 px-2 py-0.5 text-[10px] font-semibold text-amber-700 border border-amber-200/60">
          <AlertTriangle className="h-3 w-3 text-amber-600" />
          <span>{t('doctor.statusWarning')}</span>
        </span>
      );
    }
    return (
      <span className="inline-flex items-center gap-1 rounded-full bg-rose-50 px-2 py-0.5 text-[10px] font-semibold text-rose-700 border border-rose-200/60">
        <AlertCircle className="h-3 w-3 text-rose-600" />
        <span>{t('doctor.statusError')}</span>
      </span>
    );
  };

  return (
    <div className="rounded-2xl border border-slate-200/90 bg-white p-5 shadow-xs space-y-4">
      {/* Header */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-blue-50 text-blue-600 border border-blue-100">
            <Activity className="h-5 w-5" />
          </div>
          <div>
            <h3 className="text-sm font-bold text-slate-900">{t('doctor.title')}</h3>
            <p className="text-[11px] text-slate-500">{t('doctor.subtitle')}</p>
          </div>
        </div>

        <button
          type="button"
          onClick={runCheck}
          disabled={loading}
          className="flex items-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-xs font-semibold text-slate-700 hover:bg-slate-50 hover:text-slate-900 shadow-xs transition-all disabled:opacity-50"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin text-blue-600' : 'text-slate-500'}`} />
          <span>{loading ? t('doctor.checking') : t('doctor.recheck')}</span>
        </button>
      </div>

      {/* Report items list */}
      <div className="space-y-2.5">
        {loading && !report ? (
          <div className="space-y-2 py-2">
            {[1, 2, 3, 4].map((i) => (
              <div key={i} className="h-14 rounded-xl border border-slate-100 bg-slate-50/60 animate-pulse" />
            ))}
          </div>
        ) : report ? (
          report.items.map((item) => (
            <div
              key={item.key}
              className={`rounded-xl border p-3 text-xs transition-all ${
                item.status === 'ok'
                  ? 'border-slate-200/80 bg-slate-50/40'
                  : item.status === 'warning'
                  ? 'border-amber-200/80 bg-amber-50/40'
                  : 'border-rose-200/80 bg-rose-50/40'
              }`}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="flex items-start gap-2.5">
                  <div className="mt-0.5">{getItemIcon(item.key)}</div>
                  <div className="space-y-0.5">
                    <p className="font-semibold text-slate-800">{item.title}</p>
                    <p className="text-[11px] text-slate-600 font-mono select-text">{item.message}</p>
                    {item.detail && (
                      <p className="text-[11px] text-slate-500 pt-0.5 leading-relaxed">{item.detail}</p>
                    )}
                  </div>
                </div>
                <div>{getStatusBadge(item.status)}</div>
              </div>
            </div>
          ))
        ) : null}
      </div>

      {/* Timestamp footer */}
      {report && (
        <div className="text-[10px] text-slate-400 pt-1 text-right">
          {t('doctor.lastChecked')}: {new Date(report.timestamp).toLocaleTimeString()}
        </div>
      )}
    </div>
  );
};
