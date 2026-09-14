import React, { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import type { AccountData } from '../types';
import { X, Tag, Loader2, RotateCcw } from 'lucide-react';

interface AliasModalProps {
  account: AccountData | null;
  isOpen: boolean;
  onClose: () => void;
  onSaved: () => void;
}

export const AliasModal: React.FC<AliasModalProps> = ({
  account,
  isOpen,
  onClose,
  onSaved,
}) => {
  const { t } = useTranslation();
  const [alias, setAlias] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen && account) {
      setAlias(account.alias || '');
      setError(null);
    }
  }, [isOpen, account]);

  if (!isOpen || !account) return null;

  const handleSave = async () => {
    setLoading(true);
    setError(null);
    try {
      const target = account.email || account.identity_key;
      const cleanAlias = alias.trim();
      await invoke('set_alias', {
        target,
        alias: cleanAlias ? cleanAlias : null,
      });
      onSaved();
      onClose();
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to set alias');
    } finally {
      setLoading(false);
    }
  };

  const handleClear = async () => {
    setLoading(true);
    setError(null);
    try {
      const target = account.email || account.identity_key;
      await invoke('set_alias', {
        target,
        alias: null,
      });
      onSaved();
      onClose();
    } catch (err: any) {
      setError(typeof err === 'string' ? err : err?.message || 'Failed to clear alias');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 p-4 backdrop-blur-sm animate-in fade-in duration-150">
      <div
        className="relative w-full max-w-md overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl animate-in zoom-in-95 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-slate-100 px-6 py-4">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-xl bg-blue-50 text-blue-600 border border-blue-100">
              <Tag className="w-5 h-5" />
            </div>
            <div>
              <h3 className="text-base font-bold text-slate-900">{t('modals.alias.title')}</h3>
              <p className="text-xs text-slate-500">{t('modals.alias.desc')}</p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-slate-400 hover:bg-slate-100 hover:text-slate-700 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 space-y-4">
          {/* Account Details */}
          <div className="rounded-xl border border-slate-200/80 bg-slate-50 p-3 text-xs space-y-1">
            <div className="text-slate-500">{account.email || account.display_name}</div>
            <div className="text-slate-400 font-mono">Profile ID: {account.profile_id}</div>
          </div>

          <div>
            <label className="block text-xs font-semibold text-slate-700 uppercase tracking-wider mb-2">
              {t('modals.alias.title')}
            </label>
            <input
              type="text"
              value={alias}
              onChange={(e) => setAlias(e.target.value)}
              placeholder={t('modals.alias.placeholder')}
              className="w-full rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm text-slate-900 placeholder-slate-400 focus:border-blue-500 focus:outline-none focus:ring-2 focus:ring-blue-500/20 transition-all"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleSave();
              }}
            />
            <p className="text-[11px] text-slate-400 mt-1.5">
              {t('modals.alias.hint')}
            </p>
          </div>

          {error && (
            <div className="text-xs text-rose-700 bg-rose-50 border border-rose-200 px-3 py-2 rounded-lg">
              {error}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-slate-100 bg-slate-50/50 px-6 py-4">
          {account.alias ? (
            <button
              onClick={handleClear}
              disabled={loading}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl border border-slate-200 bg-white hover:bg-slate-50 text-xs font-semibold text-slate-600 hover:text-slate-900 shadow-xs transition-all disabled:opacity-50"
              title={t('modals.alias.hint')}
            >
              <RotateCcw className="w-3.5 h-3.5 text-slate-500" />
              <span>{t('modals.alias.hint')}</span>
            </button>
          ) : (
            <div />
          )}

          <div className="flex items-center gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 rounded-xl text-xs font-semibold text-slate-700 bg-white border border-slate-200 hover:bg-slate-50 transition-colors"
            >
              {t('modals.alias.cancel')}
            </button>
            <button
              onClick={handleSave}
              disabled={loading}
              className="flex items-center gap-1.5 px-4 py-2 rounded-xl text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 shadow-xs transition-all disabled:opacity-50"
            >
              {loading && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
              <span>{t('modals.alias.save')}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
