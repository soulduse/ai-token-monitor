import { createClient } from "https://esm.sh/@supabase/supabase-js@2";

const PROVIDER_COLORS: Record<string, string> = {
  claude: "#7C5CFC",
  codex: "#0ea5e9",
  opencode: "#d97706",
  kimi: "#1a73e8",
  glm: "#00b96b",
};

const PROVIDER_LABELS: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  opencode: "OpenCode",
  kimi: "Kimi",
  glm: "GLM",
};

const PERIOD_LABELS: Record<string, string> = {
  today: "Today",
  week: "This Week",
  month: "This Month",
};

const VALID_PROVIDERS = new Set(["claude", "codex", "opencode", "kimi", "glm"]);
const VALID_PERIODS = new Set(["today", "week", "month"]);
const VALID_STYLES = new Set(["flat", "flat-square", "card"]);

// Verdana 11px character width table (shields.io standard)
const CHAR_WIDTHS: Record<string, number> = {
  " ": 3.58, "!": 3.94, "#": 7.83, $: 6.27, "%": 8.72, "&": 7.37,
  "(": 3.94, ")": 3.94, "*": 6.27, "+": 7.83, ",": 3.58, "-": 3.94,
  ".": 3.58, "/": 4.47, "0": 6.27, "1": 6.27, "2": 6.27, "3": 6.27,
  "4": 6.27, "5": 6.27, "6": 6.27, "7": 6.27, "8": 6.27, "9": 6.27,
  ":": 3.58, A: 6.84, B: 6.84, C: 6.54, D: 7.58, E: 6.11, F: 5.63,
  G: 7.48, H: 7.48, I: 4.06, J: 4.47, K: 6.84, L: 5.63, M: 8.72,
  N: 7.48, O: 7.78, P: 6.11, Q: 7.78, R: 6.84, S: 6.27, T: 5.73,
  U: 7.48, V: 6.84, W: 9.77, X: 6.11, Y: 5.73, Z: 6.27,
  a: 5.73, b: 6.27, c: 4.92, d: 6.27, e: 5.73, f: 3.51, g: 6.27,
  h: 6.32, i: 2.82, j: 3.51, k: 5.73, l: 2.82, m: 9.24, n: 6.32,
  o: 6.12, p: 6.27, q: 6.27, r: 4.47, s: 4.92, t: 3.94, u: 6.32,
  v: 5.73, w: 8.18, x: 5.73, y: 5.73, z: 5.06, "|": 4.06,
};

function measureText(text: string): number {
  let width = 0;
  for (const char of text) {
    width += CHAR_WIDTHS[char] ?? 6.2;
  }
  return width;
}

function escapeXml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

function formatTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

function formatCost(usd: number): string {
  if (usd >= 100) return `$${usd.toFixed(0)}`;
  if (usd >= 1) return `$${usd.toFixed(2)}`;
  return `$${usd.toFixed(4)}`;
}

function isValidUUID(str: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(str);
}

function getDateRange(period: string): { dateFrom: string; dateTo: string } {
  const now = new Date();
  const y = now.getFullYear();
  const m = now.getMonth();
  const d = now.getDate();
  const pad = (n: number) => String(n).padStart(2, "0");
  const today = `${y}-${pad(m + 1)}-${pad(d)}`;

  if (period === "today") {
    return { dateFrom: today, dateTo: today };
  }
  if (period === "week") {
    const day = now.getDay();
    const diff = day === 0 ? 6 : day - 1; // Monday start
    const monday = new Date(y, m, d - diff);
    return {
      dateFrom: `${monday.getFullYear()}-${pad(monday.getMonth() + 1)}-${pad(monday.getDate())}`,
      dateTo: today,
    };
  }
  // month
  return { dateFrom: `${y}-${pad(m + 1)}-01`, dateTo: today };
}

function adjustColor(hex: string, amount: number): string {
  const num = parseInt(hex.replace("#", ""), 16);
  const r = Math.max(0, Math.min(255, ((num >> 16) & 0xff) + amount));
  const g = Math.max(0, Math.min(255, ((num >> 8) & 0xff) + amount));
  const b = Math.max(0, Math.min(255, (num & 0xff) + amount));
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, "0")}`;
}

function generateErrorBadge(message: string, style: string): string {
  const label = "AI Token Monitor";
  const rx = style === "flat-square" ? 0 : 3;
  const labelWidth = Math.round(measureText(label) + 12);
  const valueWidth = Math.round(measureText(message) + 12);
  const totalWidth = labelWidth + valueWidth;
  const labelX = labelWidth / 2;
  const valueX = labelWidth + valueWidth / 2;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${totalWidth}" height="20">
  <clipPath id="r"><rect width="${totalWidth}" height="20" rx="${rx}" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="${labelWidth}" height="20" fill="#555"/>
    <rect x="${labelWidth}" width="${valueWidth}" height="20" fill="#9f9f9f"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="${labelX}" y="15" fill="#010101" fill-opacity=".3">${escapeXml(label)}</text>
    <text x="${labelX}" y="14">${escapeXml(label)}</text>
    <text x="${valueX}" y="15" fill="#010101" fill-opacity=".3">${escapeXml(message)}</text>
    <text x="${valueX}" y="14">${escapeXml(message)}</text>
  </g>
</svg>`;
}

function generateFlatBadge(
  data: { nickname: string; rank: number; totalTokens: number; provider: string },
  style: string,
): string {
  const label = `AI Token Monitor | ${PROVIDER_LABELS[data.provider] ?? data.provider}`;
  const value = `#${data.rank} | ${formatTokens(data.totalTokens)} tokens`;
  const rx = style === "flat-square" ? 0 : 3;
  const color = PROVIDER_COLORS[data.provider] ?? "#555";
  const labelWidth = Math.round(measureText(label) + 12);
  const valueWidth = Math.round(measureText(value) + 12);
  const totalWidth = labelWidth + valueWidth;
  const labelX = labelWidth / 2;
  const valueX = labelWidth + valueWidth / 2;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${totalWidth}" height="20">
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r"><rect width="${totalWidth}" height="20" rx="${rx}" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="${labelWidth}" height="20" fill="#555"/>
    <rect x="${labelWidth}" width="${valueWidth}" height="20" fill="${color}"/>
    <rect width="${totalWidth}" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="${labelX}" y="15" fill="#010101" fill-opacity=".3">${escapeXml(label)}</text>
    <text x="${labelX}" y="14">${escapeXml(label)}</text>
    <text x="${valueX}" y="15" fill="#010101" fill-opacity=".3">${escapeXml(value)}</text>
    <text x="${valueX}" y="14">${escapeXml(value)}</text>
  </g>
</svg>`;
}

function truncateNickname(name: string, maxWidth: number): string {
  let width = 0;
  for (let i = 0; i < name.length; i++) {
    width += CHAR_WIDTHS[name[i]] ?? 6.2;
    if (width > maxWidth) {
      return name.slice(0, i) + "...";
    }
  }
  return name;
}

function generateCardBadge(
  data: { nickname: string; rank: number; totalTokens: number; costUsd: number; provider: string; period: string },
): string {
  const color = PROVIDER_COLORS[data.provider] ?? "#555";
  const providerLabel = PROVIDER_LABELS[data.provider] ?? data.provider;
  const periodLabel = PERIOD_LABELS[data.period] ?? data.period;
  const tokensStr = formatTokens(data.totalTokens);
  const costStr = formatCost(data.costUsd);
  const rankStr = data.rank <= 3 ? ["#1", "#2", "#3"][data.rank - 1] : `#${data.rank}`;
  // Truncate nickname to fit within card width (max ~250px for font-size 16)
  const nickname = truncateNickname(data.nickname, 220);

  return `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="140" viewBox="0 0 400 140">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${color}"/>
      <stop offset="100%" stop-color="${adjustColor(color, -30)}"/>
    </linearGradient>
  </defs>
  <rect width="400" height="140" rx="12" fill="url(#bg)"/>
  <g fill="#fff" font-family="Verdana,Geneva,DejaVu Sans,sans-serif">
    <text x="24" y="36" font-size="16" font-weight="bold">${escapeXml(nickname)}</text>
    <text x="376" y="36" font-size="20" font-weight="bold" text-anchor="end">${escapeXml(rankStr)}</text>
    <line x1="24" y1="50" x2="376" y2="50" stroke="rgba(255,255,255,0.2)" stroke-width="1"/>
    <text x="24" y="80" font-size="24" font-weight="bold">${escapeXml(tokensStr)} tokens</text>
    <text x="376" y="80" font-size="18" font-weight="bold" text-anchor="end" opacity="0.9">${escapeXml(costStr)}</text>
    <text x="24" y="112" font-size="11" opacity="0.7">${escapeXml(providerLabel)}  &#183;  ${escapeXml(periodLabel)}</text>
    <text x="376" y="130" font-size="9" text-anchor="end" opacity="0.4">AI Token Monitor</text>
  </g>
</svg>`;
}

function svgResponse(svg: string, status = 200): Response {
  return new Response(svg, {
    status,
    headers: {
      "Content-Type": "image/svg+xml",
      "Cache-Control": "public, max-age=3600, s-maxage=3600",
      "Access-Control-Allow-Origin": "*",
    },
  });
}

Deno.serve(async (req) => {
  // CORS preflight
  if (req.method === "OPTIONS") {
    return new Response(null, {
      headers: {
        "Access-Control-Allow-Origin": "*",
        "Access-Control-Allow-Methods": "GET, OPTIONS",
        "Access-Control-Allow-Headers": "Content-Type",
      },
    });
  }

  if (req.method !== "GET") {
    return svgResponse(generateErrorBadge("Method Not Allowed", "flat"), 405);
  }

  const url = new URL(req.url);
  const pathParts = url.pathname.split("/").filter(Boolean);
  const userId = pathParts[pathParts.length - 1];

  const provider = url.searchParams.get("provider") ?? "claude";
  const period = url.searchParams.get("period") ?? "month";
  const style = url.searchParams.get("style") ?? "flat";

  if (!userId || !isValidUUID(userId)) {
    return svgResponse(generateErrorBadge("Invalid User", style));
  }
  if (!VALID_PROVIDERS.has(provider)) {
    return svgResponse(generateErrorBadge("Invalid Provider", style));
  }
  if (!VALID_PERIODS.has(period)) {
    return svgResponse(generateErrorBadge("Invalid Period", style));
  }
  if (!VALID_STYLES.has(style)) {
    return svgResponse(generateErrorBadge("Invalid Style", style));
  }

  const supabase = createClient(
    Deno.env.get("SUPABASE_URL")!,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!,
  );

  // Check profile
  const { data: profile, error: profileError } = await supabase
    .from("profiles")
    .select("nickname, leaderboard_hidden")
    .eq("id", userId)
    .single();

  if (profileError || !profile) {
    return svgResponse(generateErrorBadge("User Not Found", style));
  }

  if (profile.leaderboard_hidden) {
    return svgResponse(generateErrorBadge("Private", style));
  }

  const { dateFrom, dateTo } = getDateRange(period);

  // Get user's aggregated stats
  const { data: snapshots } = await supabase
    .from("daily_snapshots")
    .select("total_tokens, cost_usd, messages")
    .eq("user_id", userId)
    .eq("provider", provider)
    .gte("date", dateFrom)
    .lte("date", dateTo);

  const totalTokens = snapshots?.reduce((s, r) => s + Number(r.total_tokens), 0) ?? 0;
  const costUsd = snapshots?.reduce((s, r) => s + Number(r.cost_usd), 0) ?? 0;

  if (totalTokens === 0) {
    return svgResponse(generateErrorBadge("No Data", style));
  }

  // Calculate rank efficiently
  const { data: rankResult } = await supabase.rpc("get_badge_rank", {
    p_user_id: userId,
    p_provider: provider,
    p_date_from: dateFrom,
    p_date_to: dateTo,
  }).single();

  // Fallback: if RPC doesn't exist yet, use a simple approach
  let rank = rankResult?.rank ?? 0;
  if (!rank) {
    // Fallback: get full leaderboard and find position
    const { data: lb } = await supabase.rpc("get_leaderboard_entries", {
      p_provider: provider,
      p_date_from: dateFrom,
      p_date_to: dateTo,
    });
    if (lb) {
      const idx = lb.findIndex((e: { user_id: string }) => e.user_id === userId);
      rank = idx >= 0 ? idx + 1 : 0;
    }
  }

  if (!rank) {
    return svgResponse(generateErrorBadge("Unranked", style));
  }

  const badgeData = {
    nickname: profile.nickname,
    rank,
    totalTokens,
    costUsd,
    provider,
    period,
  };

  if (style === "card") {
    return svgResponse(generateCardBadge(badgeData));
  }

  return svgResponse(generateFlatBadge(badgeData, style));
});
