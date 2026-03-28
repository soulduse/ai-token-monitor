export function formatTokens(n: number, format: "compact" | "full" = "compact"): string {
  if (format === "full") {
    return n.toLocaleString("en-US");
  }
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

export function formatCost(usd: number): string {
  if (usd >= 100) return `$${usd.toFixed(0)}`;
  if (usd >= 1) return `$${usd.toFixed(2)}`;
  return `$${usd.toFixed(4)}`;
}

export function formatDate(dateStr: string, locale?: string): string {
  const date = new Date(dateStr + "T00:00:00");
  // Map app locale codes to BCP 47 tags
  const localeMap: Record<string, string> = {
    "zh-CN": "zh-Hans",
    "zh-TW": "zh-Hant",
  };
  const bcp47 = locale ? (localeMap[locale] ?? locale) : undefined;
  return date.toLocaleDateString(bcp47, {
    month: "short",
    day: "numeric",
  });
}

export function getTotalTokens(tokens: Record<string, number>): number {
  return Object.values(tokens).reduce((sum, v) => sum + v, 0);
}

export function getModelTotalTokens(usage: {
  input_tokens: number;
  output_tokens: number;
  cache_read: number;
  cache_write: number;
}): number {
  return usage.input_tokens + usage.output_tokens + usage.cache_read + usage.cache_write;
}

/** Format a Date to local YYYY-MM-DD string without UTC conversion */
export function toLocalDateStr(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
