import React from 'react';
import { getQuotaStatus, getStatusColors, formatRelativeTime, formatExactTime } from '../utils';

interface QuotaRingProps {
  label: string;
  remainingPercent: number | null;
  resetsAt: number | null;
  size?: number;
  strokeWidth?: number;
  sublabel?: string;
}

export const QuotaRing: React.FC<QuotaRingProps> = ({
  label,
  remainingPercent,
  resetsAt,
  size = 140,
  strokeWidth = 10,
  sublabel = "Remaining",
}) => {
  const status = getQuotaStatus(remainingPercent);
  const colors = getStatusColors(status);

  const center = size / 2;
  const radius = center - strokeWidth;
  const circumference = 2 * Math.PI * radius;

  const validPercent = remainingPercent !== null ? Math.max(0, Math.min(100, remainingPercent)) : 0;
  const strokeDashoffset = remainingPercent !== null
    ? circumference - (validPercent / 100) * circumference
    : circumference;

  const filterId = `glow-${label.replace(/[^a-zA-Z0-9]/g, '')}`;

  return (
    <div className="flex flex-col items-center select-none">
      {/* Ring Title */}
      <span className="text-xs font-semibold uppercase tracking-wider text-slate-500 mb-2">
        {label}
      </span>

      {/* SVG Circular Progress */}
      <div className="relative flex items-center justify-center" style={{ width: size, height: size }}>
        <svg
          width={size}
          height={size}
          className="transform -rotate-90"
        >
          <defs>
            <filter id={filterId} x="-20%" y="-20%" width="140%" height="140%">
              <feDropShadow dx="0" dy="0" stdDeviation="3" floodColor={colors.stroke} floodOpacity="0.3" />
            </filter>
          </defs>

          {/* Background Track */}
          <circle
            cx={center}
            cy={center}
            r={radius}
            fill="none"
            stroke="#e2e8f0"
            strokeWidth={strokeWidth}
          />

          {/* Active Arc */}
          {remainingPercent !== null ? (
            <circle
              cx={center}
              cy={center}
              r={radius}
              fill="none"
              stroke={colors.stroke}
              strokeWidth={strokeWidth}
              strokeDasharray={circumference}
              strokeDashoffset={strokeDashoffset}
              strokeLinecap="round"
              filter={`url(#${filterId})`}
              className="transition-all duration-700 ease-out"
            />
          ) : (
            <circle
              cx={center}
              cy={center}
              r={radius}
              fill="none"
              stroke="#cbd5e1"
              strokeWidth={strokeWidth}
              strokeDasharray="4 6"
            />
          )}
        </svg>

        {/* Center Content */}
        <div className="absolute inset-0 flex flex-col items-center justify-center text-center">
          <span className={`text-2xl font-bold tracking-tight ${colors.text}`}>
            {remainingPercent !== null ? `${Math.round(remainingPercent)}%` : "-"}
          </span>
          <span className="text-[10px] font-medium text-slate-400 uppercase tracking-wider">
            {sublabel}
          </span>
        </div>
      </div>

      {/* Reset Info */}
      <div className="mt-2.5 text-center">
        {resetsAt ? (
          <div className="flex flex-col items-center">
            <span className="text-xs font-medium text-slate-600">
              Resets {formatRelativeTime(resetsAt)}
            </span>
            <span className="text-[10px] text-slate-400">
              {formatExactTime(resetsAt)}
            </span>
          </div>
        ) : (
          <span className="text-xs text-slate-400">No active limit</span>
        )}
      </div>
    </div>
  );
};
