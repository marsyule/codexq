import React from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountData } from '../types';
import { getQuotaStatus, getStatusColors, formatRelativeTime } from '../utils';
import { Edit3, History, Zap, AlertCircle } from 'lucide-react';

interface ActiveHeroCardProps {
  account: AccountData;
  onEditAlias: (acc: AccountData) => void;
  onViewHistory: (acc: AccountData) => void;
}

export const ActiveHeroCard: React.FC<ActiveHeroCardProps> = ({
  account,
  onEditAlias,
  onViewHistory,
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
    <div className="relative overflow-hidden rounded-xl border border-blue-200/90 bg-white p-3.5 shadow-xs transition-all hover:shadow-sm">
      {/* Top Row: Identity & Quick Actions */}
      <div className="flex flex-wrap items-center justify-between gap-2.5">
        {/* Left: Active Indicator, Name, Plan, Resets */}
        <div className="flex flex-wrap items-center gap-2 min-w-0">
          {/* Active status pulse dot & badge */}
          <div className="flex items-center gap-1.5 rounded-full bg-emerald-50 border border-emerald-200/80 px-2 py-0.5 shadow-2xs">
            <span className="relative flex h-1.5 w-1.5">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
              <span className="relative inline-flex rounded-full h-1.5 w-1.5 bg-emerald-500"></span>
            </span>
            <span className="text-[10px] font-bold uppercase tracking-wider text-emerald-700">
              {t('accounts.activeBadge')}
            </span>
          </div>

          {/* Account display name */}
          <span className="text-sm font-bold tracking-tight text-slate-900 truncate" title={account.display_name}>
            {account.display_name}
          </span>

          {/* Plan badge */}
          {account.plan && (
            <span className={`text-[10px] font-bold uppercase tracking-wider px-2 py-0.2 rounded-full border ${getPlanBadgeClass(account.plan)}`}>
              {account.plan}
            </span>
          )}

          {/* Reauth badge */}
          {account.credential_status === 'reauth_required' && (
            <span className="text-[10px] font-bold uppercase tracking-wider px-2 py-0.2 rounded-full border bg-rose-50 text-rose-700 border-rose-200">
              Reauth
            </span>
          )}

          {/* Available resets badge */}
          <div className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-amber-50 border border-amber-200/80 text-amber-800 text-[11px] font-semibold">
            <Zap className="w-3 h-3 text-amber-500 fill-amber-500/20" />
            <span>{account.reset_credits ?? 0}</span>
          </div>

          {/* Email subtitle if alias is used */}
          {account.alias && account.email && (
            <span className="text-[11px] text-slate-400 font-mono hidden md:inline truncate" title={account.email}>
              ({account.email})
            </span>
          )}
        </div>

        {/* Right: Compact Action Buttons */}
        <div className="flex items-center gap-1.5 shrink-0">
          <button
            onClick={() => onEditAlias(account)}
            className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium text-slate-600 bg-slate-50 hover:bg-slate-100 hover:text-slate-900 border border-slate-200/70 transition-all shadow-2xs"
            title={t('accounts.editAlias')}
          >
            <Edit3 className="w-3 h-3 text-slate-500" />
            <span>{t('accounts.editAlias')}</span>
          </button>

          <button
            onClick={() => onViewHistory(account)}
            className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium text-slate-600 bg-slate-50 hover:bg-slate-100 hover:text-slate-900 border border-slate-200/70 transition-all shadow-2xs"
            title={t('accounts.viewHistory')}
          >
            <History className="w-3 h-3 text-slate-500" />
            <span>{t('accounts.viewHistory')}</span>
          </button>
        </div>
      </div>

      {/* Bottom Row: Dual Linear Progress Bars */}
      <div className="mt-2.5 pt-2.5 border-t border-slate-100 grid grid-cols-1 sm:grid-cols-2 gap-2.5 sm:gap-6">
        {/* 5H Limit */}
        <div className="space-y-1">
          <div className="flex items-center justify-between text-xs">
            <span className="text-slate-500 font-medium flex items-center gap-1.5">
              <span>{t('accounts.primaryWindow')}</span>
              {account.primary.resets_at && (
                <span className="text-[10px] text-slate-400 font-normal">
                  ({formatRelativeTime(account.primary.resets_at)})
                </span>
              )}
            </span>
            <span className={`font-bold text-xs ${primaryColors.text}`}>
              {account.primary.remaining_percent !== null ? `${Math.round(account.primary.remaining_percent)}%` : '-'}
            </span>
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
        <div className="space-y-1">
          <div className="flex items-center justify-between text-xs">
            <span className="text-slate-500 font-medium flex items-center gap-1.5">
              <span>{t('accounts.secondaryWindow')}</span>
              {account.secondary.resets_at && (
                <span className="text-[10px] text-slate-400 font-normal">
                  ({formatRelativeTime(account.secondary.resets_at)})
                </span>
              )}
            </span>
            <span className={`font-bold text-xs ${secondaryColors.text}`}>
              {account.secondary.remaining_percent !== null ? `${Math.round(account.secondary.remaining_percent)}%` : '-'}
            </span>
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
        <div className="mt-2.5 flex items-start gap-1.5 text-xs text-rose-700 bg-rose-50 border border-rose-200 px-2.5 py-1.5 rounded-md">
          <AlertCircle className="w-3.5 h-3.5 shrink-0 mt-0.5 text-rose-500" />
          <span className="leading-snug">{account.last_error}</span>
        </div>
      )}
    </div>
  );
};
