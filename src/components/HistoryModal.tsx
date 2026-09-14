import React, { useEffect, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import type { AccountData, SnapshotRecord } from '../types';
import { formatIsoTime, formatRelativeTime } from '../utils';
import { X, Clock, AlertCircle, Loader2 } from 'lucide-react';

interface HistoryModalProps {
  account: AccountData | null;
  isOpen: boolean;
  onClose: () => void;
}

export const HistoryModal: React.FC<HistoryModalProps> = ({
  account,
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation();
  const [snapshots, setSnapshots] = useState<SnapshotRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadHistory = useCallback(async () => {
    if (!account) return;
    setLoading(true);
    setError(null);
    try {
      // Pass the display_name, email, or identity_key
      const target = account.email || account.identity_key;
      const res = await invoke<SnapshotRecord[]>('get_history', {
        target,
        limit: 50,
      });
      setSnapshots(res);
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to load history');
    } finally {
      setLoading(false);
    }
  }, [account]);

  useEffect(() => {
    if (isOpen && account) {
      loadHistory();
    } else {
      setSnapshots([]);
      setError(null);
    }
  }, [isOpen, account, loadHistory]);

  if (!isOpen || !account) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 p-4 backdrop-blur-sm animate-in fade-in duration-150">
      <div
        className="relative flex flex-col max-h-[85vh] w-full max-w-3xl overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Modal Header */}
        <div className="flex items-center justify-between border-b border-slate-100 px-6 py-4">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-xl bg-blue-50 text-blue-600 border border-blue-100">
              <Clock className="w-5 h-5" />
            </div>
            <div>
              <h3 className="text-lg font-bold text-slate-900">{t('modals.history.title')}</h3>
              <p className="text-xs text-slate-500">
                <span className="font-semibold text-slate-800">{account.display_name}</span>
              </p>
            </div>
          </div>

          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-700 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Modal Body */}
        <div className="flex-1 overflow-y-auto p-6">
          {loading ? (
            <div className="flex flex-col items-center justify-center py-16 text-slate-400">
              <Loader2 className="w-8 h-8 animate-spin text-blue-600 mb-3" />
              <p className="text-sm">{t('app.status.refreshing')}</p>
            </div>
          ) : error ? (
            <div className="flex items-center gap-3 rounded-xl border border-rose-200 bg-rose-50 p-4 text-rose-700">
              <AlertCircle className="w-5 h-5 shrink-0 text-rose-500" />
              <p className="text-sm">{error}</p>
            </div>
          ) : snapshots.length === 0 ? (
            <div className="text-center py-16 text-slate-400">
              <Clock className="w-10 h-10 mx-auto mb-2 opacity-40 text-slate-400" />
              <p className="text-sm text-slate-600">{t('modals.history.empty')}</p>
            </div>
          ) : (
            <div className="overflow-x-auto rounded-xl border border-slate-200">
              <table className="w-full text-left text-xs text-slate-700">
                <thead className="bg-slate-50 text-[11px] font-semibold uppercase text-slate-500 border-b border-slate-200">
                  <tr>
                    <th className="py-3 px-4">{t('modals.history.time')}</th>
                    <th className="py-3 px-4">Limit</th>
                    <th className="py-3 px-4">{t('accounts.primaryWindow')}</th>
                    <th className="py-3 px-4">{t('modals.history.resetsAt')}</th>
                    <th className="py-3 px-4">{t('accounts.secondaryWindow')}</th>
                    <th className="py-3 px-4">{t('modals.history.resetsAt')}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-100 font-mono">
                  {snapshots.map((snap) => {
                    const primaryRem = snap.primary_used_percent !== null
                      ? 100 - snap.primary_used_percent
                      : null;
                    const secondaryRem = snap.secondary_used_percent !== null
                      ? 100 - snap.secondary_used_percent
                      : null;

                    return (
                      <tr key={snap.id} className="hover:bg-slate-50/80 transition-colors">
                        <td className="py-2.5 px-4 font-sans text-slate-500">
                          {formatIsoTime(snap.observed_at)}
                        </td>
                        <td className="py-2.5 px-4">
                          <span className="inline-block px-2 py-0.5 rounded bg-slate-100 text-slate-600 text-[10px] font-medium border border-slate-200">
                            {snap.limit_id}
                          </span>
                        </td>
                        <td className="py-2.5 px-4 font-semibold">
                          {primaryRem !== null ? (
                            <span className={primaryRem > 20 ? 'text-emerald-600' : primaryRem > 0 ? 'text-amber-600' : 'text-rose-600'}>
                              {primaryRem.toFixed(0)}%
                            </span>
                          ) : (
                            <span className="text-slate-400">-</span>
                          )}
                        </td>
                        <td className="py-2.5 px-4 text-slate-500 font-sans">
                          {snap.primary_resets_at ? formatRelativeTime(snap.primary_resets_at) : '-'}
                        </td>
                        <td className="py-2.5 px-4 font-semibold">
                          {secondaryRem !== null ? (
                            <span className={secondaryRem > 20 ? 'text-emerald-600' : secondaryRem > 0 ? 'text-amber-600' : 'text-rose-600'}>
                              {secondaryRem.toFixed(0)}%
                            </span>
                          ) : (
                            <span className="text-slate-400">-</span>
                          )}
                        </td>
                        <td className="py-2.5 px-4 text-slate-500 font-sans">
                          {snap.secondary_resets_at ? formatRelativeTime(snap.secondary_resets_at) : '-'}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>

        {/* Modal Footer */}
        <div className="flex items-center justify-between border-t border-slate-100 bg-slate-50/60 px-6 py-3">
          <span className="text-xs text-slate-500">
            {snapshots.length}
          </span>
          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-xl text-xs font-semibold text-slate-700 bg-white border border-slate-200 hover:bg-slate-50 transition-colors"
          >
            {t('modals.history.close')}
          </button>
        </div>
      </div>
    </div>
  );
};
