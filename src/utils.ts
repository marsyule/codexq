/**
 * Format Unix epoch seconds into relative time (e.g., "in 2h 15m", "30m ago")
 */
export function formatRelativeTime(epochSeconds: number | null): string {
  if (!epochSeconds) return "N/A";
  const now = Math.floor(Date.now() / 1000);
  const diff = epochSeconds - now;

  if (diff <= 0) {
    const past = Math.abs(diff);
    if (past < 60) return "just now";
    if (past < 3600) return `${Math.floor(past / 60)}m ago`;
    if (past < 86400) return `${Math.floor(past / 3600)}h ago`;
    return `${Math.floor(past / 86400)}d ago`;
  }

  if (diff < 60) return "in <1m";
  if (diff < 3600) return `in ${Math.floor(diff / 60)}m`;
  const hours = Math.floor(diff / 3600);
  const mins = Math.floor((diff % 3600) / 60);
  if (diff < 86400) {
    return mins > 0 ? `in ${hours}h ${mins}m` : `in ${hours}h`;
  }
  const days = Math.floor(diff / 86400);
  const remHours = Math.floor((diff % 86400) / 3600);
  return remHours > 0 ? `in ${days}d ${remHours}h` : `in ${days}d`;
}

/**
 * Format Unix epoch seconds into localized date and time string
 */
export function formatExactTime(epochSeconds: number | null): string {
  if (!epochSeconds) return "-";
  try {
    const d = new Date(epochSeconds * 1000);
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return String(epochSeconds);
  }
}

/**
 * Format ISO date string (e.g., "2026-09-09T05:23:30+00:00") into readable local string
 */
export function formatIsoTime(isoStr: string | null): string {
  if (!isoStr) return "-";
  try {
    const d = new Date(isoStr);
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return isoStr;
  }
}

export type QuotaStatus = "healthy" | "warning" | "exhausted" | "unknown";

export function getQuotaStatus(remainingPercent: number | null): QuotaStatus {
  if (remainingPercent === null || remainingPercent === undefined) return "unknown";
  if (remainingPercent <= 0) return "exhausted";
  if (remainingPercent <= 20) return "warning";
  return "healthy";
}

export function getStatusColors(status: QuotaStatus) {
  switch (status) {
    case "healthy":
      return {
        stroke: "#10b981", // emerald-500
        glow: "rgba(16, 185, 129, 0.25)",
        text: "text-emerald-600",
        badgeBg: "bg-emerald-50 border-emerald-200 text-emerald-700",
      };
    case "warning":
      return {
        stroke: "#f59e0b", // amber-500
        glow: "rgba(245, 158, 11, 0.25)",
        text: "text-amber-600",
        badgeBg: "bg-amber-50 border-amber-200 text-amber-700",
      };
    case "exhausted":
      return {
        stroke: "#f43f5e", // rose-500
        glow: "rgba(244, 63, 94, 0.25)",
        text: "text-rose-600",
        badgeBg: "bg-rose-50 border-rose-200 text-rose-700",
      };
    default:
      return {
        stroke: "#94a3b8", // slate-400
        glow: "rgba(148, 163, 184, 0.15)",
        text: "text-slate-500",
        badgeBg: "bg-slate-100 border-slate-200 text-slate-600",
      };
  }
}
