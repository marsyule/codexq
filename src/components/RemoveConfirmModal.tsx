import React from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountData } from '../types';
import { Trash2, Loader2, X } from 'lucide-react';

interface RemoveConfirmModalProps {
  isOpen: boolean;
  account: AccountData | null;
  onClose: () => void;
  onConfirm: () => void;
  loading: boolean;
}

export const RemoveConfirmModal: React.FC<RemoveConfirmModalProps> = ({
  isOpen,
  account,
  onClose,
  onConfirm,
  loading,
}) => {
  const { t } = useTranslation();
  if (!isOpen || !account) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 p-4 backdrop-blur-sm animate-in fade-in duration-150">
      <div
        className="relative w-full max-w-md overflow-hidden rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <div className="rounded-xl bg-rose-50 p-2.5 text-rose-600 border border-rose-200/80">
              <Trash2 className="h-5 w-5" />
            </div>
            <div>
              <h3 className="text-base font-bold text-slate-900">{t('modals.remove.title')}</h3>
              <p className="text-xs text-slate-500 mt-0.5">{account.email || account.display_name}</p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-700 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Target Account Summary */}
        <div className="mt-4 rounded-xl border border-slate-200/90 bg-slate-50/80 p-3.5 space-y-1">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-slate-800 truncate" title={account.display_name}>
              {account.display_name}
            </span>
            {account.plan && (
              <span className="text-[10px] font-bold uppercase tracking-wider px-2 py-0.5 rounded-full border bg-slate-200/60 text-slate-700 border-slate-300">
                {account.plan}
              </span>
            )}
          </div>
          {account.email && (
            <p className="text-xs text-slate-500 font-mono">{account.email}</p>
          )}
          <div className="flex items-center gap-2 pt-1 text-[11px] text-slate-400 font-mono">
            <span>Profile ID: {account.profile_id.slice(0, 10)}</span>
            {account.is_current && (
              <span className="text-amber-600 font-medium">· {t('accounts.activeBadge')}</span>
            )}
          </div>
        </div>

        {/* Description */}
        <p className="mt-3 text-xs text-slate-600 leading-relaxed">
          {t('modals.remove.desc', { email: account.email || account.display_name })}
        </p>

        {/* Footer actions */}
        <div className="mt-6 flex items-center justify-end gap-3">
          <button
            onClick={onClose}
            disabled={loading}
            className="rounded-xl border border-slate-200 bg-white px-4 py-2 text-xs font-semibold text-slate-700 hover:bg-slate-50 transition-colors"
          >
            {t('modals.remove.cancel')}
          </button>
          <button
            onClick={onConfirm}
            disabled={loading}
            className="flex items-center gap-1.5 rounded-xl bg-rose-600 px-4 py-2 text-xs font-semibold text-white hover:bg-rose-700 shadow-sm shadow-rose-600/20 transition-all disabled:opacity-50"
          >
            {loading && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
            <span>{t('modals.remove.confirm')}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
