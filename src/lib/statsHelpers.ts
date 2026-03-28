import type { DailyUsage, ModelUsage } from "./types";
import { getModelTotalTokens, getTotalTokens, toLocalDateStr } from "./format";

export type Period = "today" | "week" | "month" | "year" | "all";

export function filterByPeriod(daily: DailyUsage[], period: Period, year?: number): DailyUsage[] {
  const now = new Date();
  const todayStr = toLocalDateStr(now);

  switch (period) {
    case "today":
      return daily.filter((d) => d.date === todayStr);
    case "week": {
      const dow = now.getDay();
      const mondayOffset = dow === 0 ? 6 : dow - 1;
      const monday = new Date(now);
      monday.setDate(now.getDate() - mondayOffset);
      const mondayStr = toLocalDateStr(monday);
      return daily.filter((d) => d.date >= mondayStr && d.date <= todayStr);
    }
    case "month": {
      const prefix = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
      return daily.filter((d) => d.date.startsWith(prefix));
    }
    case "year": {
      const y = year ?? now.getFullYear();
      return daily.filter((d) => d.date.startsWith(`${y}-`));
    }
    case "all":
      return daily;
  }
}

export function computeTotalCost(daily: DailyUsage[]): number {
  return daily.reduce((sum, d) => sum + d.cost_usd, 0);
}

export function computeTotalTokens(daily: DailyUsage[]): number {
  return daily.reduce((sum, d) => sum + getTotalTokens(d.tokens), 0);
}

export function findBusiestDay(daily: DailyUsage[]): { date: string; tokens: number } {
  let best = { date: "", tokens: 0 };
  for (const d of daily) {
    const t = getTotalTokens(d.tokens);
    if (t > best.tokens) best = { date: d.date, tokens: t };
  }
  return best;
}

export function getMostUsedModel(modelUsage: Record<string, ModelUsage>): { name: string; totalTokens: number; cost: number } | null {
  let best: { name: string; totalTokens: number; cost: number } | null = null;
  for (const [name, u] of Object.entries(modelUsage)) {
    const total = getModelTotalTokens(u);
    if (!best || total > best.totalTokens) {
      best = { name, totalTokens: total, cost: u.cost_usd };
    }
  }
  return best;
}

export function computeCacheHitRate(modelUsage: Record<string, ModelUsage>): number {
  let totalInput = 0;
  let totalCacheRead = 0;
  for (const u of Object.values(modelUsage)) {
    totalInput += u.input_tokens;
    totalCacheRead += u.cache_read;
  }
  const denom = totalInput + totalCacheRead;
  return denom > 0 ? (totalCacheRead / denom) * 100 : 0;
}

export function computeCacheSavings(daily: DailyUsage[]): number {
  // Approximate savings: cache reads cost ~10% of input price
  // Savings = cache_read_tokens * 0.9 * avg_input_price_per_token
  // Use a rough avg input price of $3/MTok (Sonnet-weighted)
  const totalCacheRead = daily.reduce((sum, d) => sum + d.cache_read_tokens, 0);
  const avgInputPrice = 3 / 1_000_000; // $3 per million tokens
  return totalCacheRead * avgInputPrice * 0.9;
}

export interface StreakInfo {
  currentStreak: number;
  currentStart: string;
  currentEnd: string;
  longestStreak: number;
  longestStart: string;
  longestEnd: string;
}

export function computeStreaks(daily: DailyUsage[], year?: number): StreakInfo {
  const now = new Date();
  const todayStr = toLocalDateStr(now);

  const filtered = year != null
    ? daily.filter((d) => d.date.startsWith(`${year}-`))
    : daily;

  const activeDates = new Set(
    filtered.filter((d) => getTotalTokens(d.tokens) > 0).map((d) => d.date)
  );

  // Current streak (if not active today, start from yesterday)
  let currentStreak = 0;
  let currentStart = todayStr;
  const checkDate = new Date(now);
  if (!activeDates.has(todayStr)) {
    checkDate.setDate(checkDate.getDate() - 1);
  }
  while (true) {
    const ds = toLocalDateStr(checkDate);
    if (activeDates.has(ds)) {
      currentStreak++;
      currentStart = ds;
      checkDate.setDate(checkDate.getDate() - 1);
    } else {
      break;
    }
  }

  // Longest streak
  const sortedDates = Array.from(activeDates).sort();
  let longestStreak = 0;
  let longestStart = "";
  let longestEnd = "";
  let streak = 0;
  let streakStart = "";
  let prevDate = "";

  for (const ds of sortedDates) {
    if (prevDate) {
      const prev = new Date(prevDate + "T00:00:00");
      const curr = new Date(ds + "T00:00:00");
      const diff = (curr.getTime() - prev.getTime()) / 86400000;
      if (diff === 1) {
        streak++;
      } else {
        if (streak > longestStreak) {
          longestStreak = streak;
          longestStart = streakStart;
          longestEnd = prevDate;
        }
        streak = 1;
        streakStart = ds;
      }
    } else {
      streak = 1;
      streakStart = ds;
    }
    prevDate = ds;
  }
  if (streak > longestStreak) {
    longestStreak = streak;
    longestStart = streakStart;
    longestEnd = prevDate;
  }

  return {
    currentStreak,
    currentStart,
    currentEnd: todayStr,
    longestStreak,
    longestStart,
    longestEnd,
  };
}

export function shortenModelName(name: string): string {
  const match = name.match(/claude-(\w+)-(\d+)-(\d+)/);
  if (match) {
    return `${match[1].charAt(0).toUpperCase() + match[1].slice(1)} ${match[2]}.${match[3]}`;
  }
  // Codex models
  if (name.startsWith("gpt-")) return name;
  if (name === "codex-mini") return "Codex Mini";
  return name;
}
