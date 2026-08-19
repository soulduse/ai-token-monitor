import type { DailyUsage, ModelUsage } from "./types";
import { getTotalTokens, toLocalDateStr } from "./format";

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
    const total = u.input_tokens + u.output_tokens + u.cache_read;
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

/** Capitalize each hyphen-separated word, leaving pure-numeric segments alone. */
function titleCase(s: string): string {
  return s
    .split("-")
    .map((w) => (/^\d+$/.test(w) ? w : w.charAt(0).toUpperCase() + w.slice(1)))
    .join(" ");
}

/**
 * Human-readable label for a model id, e.g. `claude-opus-4-8` → "Opus 4.8".
 *
 * Ids reaching the frontend are normalized by the backend: lowercase, with dots
 * and underscores folded to hyphens and any vendor prefix dropped. So `gpt-5.6-sol`
 * arrives as `gpt-5-6-sol`, and the version separator has to be reconstructed
 * here rather than read off the id.
 *
 * Version digits are capped at 2 so date suffixes ("-20260320") never read as
 * versions.
 */
export function shortenModelName(name: string): string {
  const claude = name.match(
    /(opus|sonnet|haiku|fable|mythos)-(\d{1,2})(?:-(\d{1,2}))?(?!\d)/
  );
  if (claude) {
    const family = claude[1].charAt(0).toUpperCase() + claude[1].slice(1);
    return claude[3] ? `${family} ${claude[2]}.${claude[3]}` : `${family} ${claude[2]}`;
  }

  if (name === "codex-mini" || name === "codex-mini-latest") return "Codex Mini";
  if (name === "codex") return "Codex";

  if (name.startsWith("grok-code-fast") || name.startsWith("grok-build")) {
    return "Grok Code Fast";
  }

  // `gpt-4o` glues a letter to the version, so it has no `major.minor` to rebuild.
  const gpt4o = name.match(/^gpt-4o(?:-(.+))?$/);
  if (gpt4o) return `GPT-4o${gpt4o[1] ? " " + titleCase(gpt4o[1]) : ""}`;

  // OpenAI reasoning models: `o3`, `o4-mini`.
  const oSeries = name.match(/^(o\d)(?:-(.+))?$/);
  if (oSeries) return `${oSeries[1].toUpperCase()}${oSeries[2] ? " " + titleCase(oSeries[2]) : ""}`;

  // Families that write their version as `major-minor` and display it as
  // `major.minor`, with an optional tier suffix:
  //   gpt-5-6-sol → "GPT-5.6 Sol", grok-4-5 → "Grok 4.5", glm-4-6 → "GLM 4.6"
  const FAMILIES: Array<[RegExp, string]> = [
    [/^gpt-(\d{1,2})(?:-(\d{1,2}))?(?:-(.+))?$/, "GPT-"],
    [/^grok-(\d{1,2})(?:-(\d{1,2}))?(?:-(.+))?$/, "Grok "],
    [/^glm-(\d{1,2})(?:-(\d{1,2}))?(?:-(.+))?$/, "GLM "],
    [/^gemini-(\d{1,2})(?:-(\d{1,2}))?(?:-(.+))?$/, "Gemini "],
  ];
  for (const [pattern, prefix] of FAMILIES) {
    const m = name.match(pattern);
    if (m) {
      const version = m[2] ? `${m[1]}.${m[2]}` : m[1];
      return `${prefix}${version}${m[3] ? " " + titleCase(m[3]) : ""}`;
    }
  }

  // Unrecognized: title-case the hyphenated segments so it still reads as a name
  // rather than a raw id.
  return titleCase(name);
}
