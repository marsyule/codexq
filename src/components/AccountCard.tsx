import React from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountData } from '../types';
import { getQuotaStatus, getStatusColors, formatRelativeTime } from '../utils';
import { Zap, Edit3, History, ShieldCheck, ArrowRightLeft, AlertCircle, Trash2 } from 'lucide-react';

interface AccountCardProps {
  account: AccountData;
  onSwitch: (acc: AccountData) => void;
  onEditAlias: (acc: AccountData) => void;
  onViewHistory: (acc: AccountData) => void;
  onRemove: (acc: AccountData) => void;
  isSwitching: boolean;
}

export const AccountCard: React.FC<AccountCardProps> = ({
  account,
  onSwitch,
  onEditAlias,
  onViewHistory,
  onRemove,
  isSwitching,
}) => {
  const { t } = useTranslation();
  const primaryStatus = getQuotaStatus(account.primary.remaining_percent);
  const primaryColors = getStatusColors(primaryStatus);

  const secondaryStatus = getQuotaStatus(account.secondary.remaining_percent);
  const secondaryColors = getStatusColors(secondaryStatus);

  const getPlanBadgeClass = (plan: string | null) => {
    switch (plan?.toLowerCase()) {
      case 'team':
        return 'bg-blue-50 text-blue-700 border-blue-200';
      case 'plus':
        return 'bg-purple-50 text-purple-700 border-purple-200';
      case 'enterprise':
        return 'bg-indigo-50 text-indigo-700 border-indigo-200';
      case 'free':
        return 'bg-slate-100 text-slate-600 border-slate-200';
      default:
        return 'bg-slate-100 text-slate-600 border-slate-200';
    }
  };

  return (
    <div className="group relative flex flex-col justify-between overflow-hidden rounded-xl border border-slate-200/90 bg-white p-3.5 shadow-xs transition-all duration-200 hover:border-blue-300 hover:shadow-sm">
      {/* Top Header */}
      <div>
        <div className="flex items-start justify-between gap-2.5">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5 flex-wrap">
              <h3 className="truncate text-sm font-semibold text-slate-900 group-hover:text-blue-600 transition-colors" title={account.display_name}>
                {account.display_name}
              </h3>
              {account.plan && (
                <span className={`text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.2 rounded-full border ${getPlanBadgeClass(account.plan)}`}>
                  {account.plan}
                </span>
              )}
              {account.credential_status === 'reauth_required' && (
                <span className="text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.2 rounded-full border bg-rose-50 text-rose-700 border-rose-200">
                  Reauth
                </span>
              )}
            </div>
            {account.alias && account.email && (
              <p className="truncate text-[11px] text-slate-400 font-mono mt-0.5" title={account.email}>
                {account.email}
              </p>
            )}
          </div>

          {/* Available Resets */}
          <div className="flex shrink-0 items-center gap-1 rounded-md bg-amber-50 border border-amber-200/80 px-1.5 py-0.5 text-[11px] font-semibold text-amber-800">
            <Zap className="h-3 w-3 text-amber-500 fill-amber-500/20" />
            <span>{account.reset_credits ?? 0}</span>
          </div>
        </div>

        {/* Quota Progress Bars */}
        <div className="mt-2.5 space-y-2">
          {/* 5H Limit */}
          <div>
            <div className="flex items-center justify-between text-xs mb-1">
              <span className="text-slate-500 font-medium text-[11px]">{t('accounts.primaryWindow')}</span>
              <div className="flex items-center gap-1.5">
                <span className={`font-bold text-xs ${primaryColors.text}`}>
                  {account.primary.remaining_percent !== null ? `${Math.round(account.primary.remaining_percent)}%` : '-'}
                </span>
                {account.primary.resets_at && (
                  <span className="text-[10px] text-slate-400">
                    ({formatRelativeTime(account.primary.resets_at)})
                  </span>
                )}
              </div>
            </div>
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-slate-100">
              <div
                className="h-full rounded-full transition-all duration-500"
                style={{
                  width: `${account.primary.remaining_percent !== null ? Math.max(0, Math.min(100, account.primary.remaining_percent)) : 0}%`,
                  backgroundColor: primaryColors.stroke,
                }}
              />
            </div>
          </div>

          {/* Weekly Limit */}
          <div>
            <div className="flex items-center justify-between text-xs mb-1">
              <span className="text-slate-500 font-medium text-[11px]">{t('accounts.secondaryWindow')}</span>
              <div className="flex items-center gap-1.5">
                <span className={`font-bold text-xs ${secondaryColors.text}`}>
                  {account.secondary.remaining_percent !== null ? `${Math.round(account.secondary.remaining_percent)}%` : '-'}
                </span>
                {account.secondary.resets_at && (
                  <span className="text-[10px] text-slate-400">
                    ({formatRelativeTime(account.secondary.resets_at)})
                  </span>
                )}
              </div>
            </div>
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-slate-100">
              <div
                className="h-full rounded-full transition-all duration-500"
                style={{
                  width: `${account.secondary.remaining_percent !== null ? Math.max(0, Math.min(100, account.secondary.remaining_percent)) : 0}%`,
                  backgroundColor: secondaryColors.stroke,
                }}
              />
            </div>
          </div>
        </div>

        {account.last_error && (
          <div className="mt-2 flex items-start gap-1.5 text-xs text-rose-700 bg-rose-50 border border-rose-200 px-2 py-1 rounded-md" title={account.last_error}>
            <AlertCircle className="w-3 h-3 shrink-0 mt-0.5 text-rose-500" />
            <span className="leading-snug text-[11px]">{account.last_error}</span>
          </div>
        )}
      </div>

      {/* Card Footer: Metadata & Actions */}
      <div className="mt-3 flex items-center justify-between border-t border-slate-100/90 pt-2.5">
        <div className="flex items-center gap-1 text-[11px] font-mono text-slate-400">
          <ShieldCheck className="h-3 w-3 text-slate-400" />
          <span>{account.profile_id.slice(0, 8)}</span>
        </div>

        <div className="flex items-center gap-1">
          <button
            onClick={() => onEditAlias(account)}
            className="p-1 rounded-md text-slate-400 hover:text-slate-700 hover:bg-slate-100 transition-colors"
            title={t('accounts.editAlias')}
          >
            <Edit3 className="w-3 h-3" />
          </button>

          <button
            onClick={() => onViewHistory(account)}
            className="p-1 rounded-md text-slate-400 hover:text-slate-700 hover:bg-slate-100 transition-colors"
            title={t('accounts.viewHistory')}
          >
            <History className="w-3 h-3" />
          </button>

          {/* Remove Account Button */}
          <button
            onClick={() => onRemove(account)}
            className="p-1 rounded-md text-slate-400 hover:text-rose-600 hover:bg-rose-50 transition-colors"
            title={t('accounts.moveToTrash')}
          >
            <Trash2 className="w-3 h-3" />
          </button>

          {/* Instant Switch Button */}
          <button
            onClick={() => onSwitch(account)}
            disabled={isSwitching}
            className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-semibold text-white bg-blue-600 hover:bg-blue-700 shadow-2xs transition-all disabled:opacity-50"
            title={t('accounts.switchTo')}
          >
            <ArrowRightLeft className={`w-3 h-3 ${isSwitching ? 'animate-spin' : ''}`} />
            <span>{t('accounts.switchTo')}</span>
          </button>
        </div>
      </div>
    </div>
  );
};
