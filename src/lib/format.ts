export function formatTokens(n: number, format: "compact" | "full" = "compact"): string {
  if (format === "full") {
    return n.toLocaleString("en-US");
  }
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

export function formatCost(usd: number): string {
  if (usd >= 100) return `$${usd.toFixed(0)}`;
  if (usd >= 1) return `$${usd.toFixed(2)}`;
  return `$${usd.toFixed(4)}`;
}

// Map app locale codes to BCP 47 tags
const LOCALE_MAP: Record<string, string> = {
  "zh-CN": "zh-Hans",
  "zh-TW": "zh-Hant",
};

function toBcp47(locale?: string): string | undefined {
  return locale ? (LOCALE_MAP[locale] ?? locale) : undefined;
}

export function formatDate(dateStr: string, locale?: string): string {
  const date = new Date(dateStr + "T00:00:00");
  return date.toLocaleDateString(toBcp47(locale), {
    month: "short",
    day: "numeric",
  });
}

/** Date label for day navigation — includes the weekday (e.g. "Sat, Jul 18") */
export function formatNavDate(dateStr: string, locale?: string): string {
  const date = new Date(dateStr + "T00:00:00");
  return date.toLocaleDateString(toBcp47(locale), {
    month: "short",
    day: "numeric",
    weekday: "short",
  });
}

export function getTotalTokens(tokens: Record<string, number>): number {
  return Object.values(tokens).reduce((sum, v) => sum + v, 0);
}

/** Cache-only tokens for a day (cache_read + cache_write) — used for "(cached)" UI labels */
export function getDayCacheTokens(day: { cache_read_tokens: number; cache_write_tokens: number }): number {
  return day.cache_read_tokens + day.cache_write_tokens;
}

/** Format a Date to local YYYY-MM-DD string without UTC conversion */
export function toLocalDateStr(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
