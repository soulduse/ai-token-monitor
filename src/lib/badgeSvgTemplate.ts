import type { LeaderboardProvider } from "./types";
import { formatTokens, formatCost } from "./format";

export interface BadgeSvgData {
  nickname: string;
  rank: number;
  totalTokens: number;
  costUsd: number;
  provider: LeaderboardProvider;
  period: "today" | "week" | "month";
}

export const PROVIDER_COLORS: Record<LeaderboardProvider, string> = {
  claude: "#7C5CFC",
  codex: "#0ea5e9",
  opencode: "#d97706",
  kimi: "#1a73e8",
  glm: "#00b96b",
};

export const PROVIDER_LABELS: Record<LeaderboardProvider, string> = {
  claude: "Claude",
  codex: "Codex",
  opencode: "OpenCode",
  kimi: "Kimi",
  glm: "GLM",
};

export const PERIOD_LABELS: Record<string, string> = {
  today: "Today",
  week: "This Week",
  month: "This Month",
};

// Verdana 11px character width table (shields.io standard)
// Approximate widths for common ASCII characters
const CHAR_WIDTHS: Record<string, number> = {
  " ": 3.58, "!": 3.94, '"': 5.06, "#": 7.83, $: 6.27, "%": 8.72, "&": 7.37,
  "'": 2.82, "(": 3.94, ")": 3.94, "*": 6.27, "+": 7.83, ",": 3.58, "-": 3.94,
  ".": 3.58, "/": 4.47, "0": 6.27, "1": 6.27, "2": 6.27, "3": 6.27, "4": 6.27,
  "5": 6.27, "6": 6.27, "7": 6.27, "8": 6.27, "9": 6.27, ":": 3.58, ";": 3.58,
  "<": 7.83, "=": 7.83, ">": 7.83, "?": 5.52, "@": 9.77, A: 6.84, B: 6.84,
  C: 6.54, D: 7.58, E: 6.11, F: 5.63, G: 7.48, H: 7.48, I: 4.06, J: 4.47,
  K: 6.84, L: 5.63, M: 8.72, N: 7.48, O: 7.78, P: 6.11, Q: 7.78, R: 6.84,
  S: 6.27, T: 5.73, U: 7.48, V: 6.84, W: 9.77, X: 6.11, Y: 5.73, Z: 6.27,
  a: 5.73, b: 6.27, c: 4.92, d: 6.27, e: 5.73, f: 3.51, g: 6.27, h: 6.32,
  i: 2.82, j: 3.51, k: 5.73, l: 2.82, m: 9.24, n: 6.32, o: 6.12, p: 6.27,
  q: 6.27, r: 4.47, s: 4.92, t: 3.94, u: 6.32, v: 5.73, w: 8.18, x: 5.73,
  y: 5.73, z: 5.06, "|": 4.06,
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

export function generateFlatBadgeSvg(
  data: BadgeSvgData,
  style: "flat" | "flat-square" = "flat",
): string {
  const label = `AI Token Monitor · ${PROVIDER_LABELS[data.provider]}`;
  const value = `#${data.rank} · ${formatTokens(data.totalTokens)} tokens`;
  const rx = style === "flat" ? 3 : 0;
  const pad = 18; // horizontal padding per segment

  const labelWidth = Math.ceil(measureText(label) + pad);
  const valueWidth = Math.ceil(measureText(value) + pad);
  const totalWidth = labelWidth + valueWidth;

  const labelX = Math.round(labelWidth / 2);
  const valueX = Math.round(labelWidth + valueWidth / 2);
  const color = PROVIDER_COLORS[data.provider];

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${totalWidth}" height="20" viewBox="0 0 ${totalWidth} 20">
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

export function generateCardBadgeSvg(
  data: BadgeSvgData,
  theme: "light" | "dark" = "dark",
): string {
  const color = PROVIDER_COLORS[data.provider];
  const providerLabel = PROVIDER_LABELS[data.provider];
  const periodLabel = PERIOD_LABELS[data.period] ?? data.period;
  const tokensStr = formatTokens(data.totalTokens);
  const costStr = formatCost(data.costUsd);
  const rankStr = data.rank <= 3
    ? ["🥇", "🥈", "🥉"][data.rank - 1]
    : `#${data.rank}`;

  if (theme === "dark") {
    return `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="140" viewBox="0 0 400 140">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${color}"/>
      <stop offset="100%" stop-color="${adjustColor(color, -30)}"/>
    </linearGradient>
  </defs>
  <rect width="400" height="140" rx="12" fill="url(#bg)"/>
  <g fill="#fff" font-family="Verdana,Geneva,DejaVu Sans,sans-serif">
    <!-- Nickname + Rank -->
    <text x="24" y="36" font-size="16" font-weight="bold">${escapeXml(data.nickname)}</text>
    <text x="376" y="36" font-size="20" font-weight="bold" text-anchor="end">${escapeXml(rankStr)}</text>
    <!-- Divider -->
    <line x1="24" y1="50" x2="376" y2="50" stroke="rgba(255,255,255,0.2)" stroke-width="1"/>
    <!-- Tokens + Cost -->
    <text x="24" y="80" font-size="24" font-weight="bold">${escapeXml(tokensStr)} tokens</text>
    <text x="376" y="80" font-size="18" font-weight="bold" text-anchor="end" opacity="0.9">${escapeXml(costStr)}</text>
    <!-- Provider + Period -->
    <text x="24" y="112" font-size="11" opacity="0.7">${escapeXml(providerLabel)}  ·  ${escapeXml(periodLabel)}</text>
    <!-- Watermark -->
    <text x="376" y="130" font-size="9" text-anchor="end" opacity="0.4">AI Token Monitor</text>
  </g>
</svg>`;
  }

  // Light theme
  return `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="140" viewBox="0 0 400 140">
  <rect width="400" height="140" rx="12" fill="#fff" stroke="#e5e7eb" stroke-width="1"/>
  <rect x="0" y="0" width="6" height="140" rx="3" fill="${color}"/>
  <g font-family="Verdana,Geneva,DejaVu Sans,sans-serif">
    <!-- Nickname + Rank -->
    <text x="24" y="36" font-size="16" font-weight="bold" fill="#1a1a1a">${escapeXml(data.nickname)}</text>
    <text x="376" y="36" font-size="20" font-weight="bold" text-anchor="end" fill="${color}">${escapeXml(rankStr)}</text>
    <!-- Divider -->
    <line x1="24" y1="50" x2="376" y2="50" stroke="#e5e7eb" stroke-width="1"/>
    <!-- Tokens + Cost -->
    <text x="24" y="80" font-size="24" font-weight="bold" fill="#1a1a1a">${escapeXml(tokensStr)} tokens</text>
    <text x="376" y="80" font-size="18" font-weight="bold" text-anchor="end" fill="#6b7280">${escapeXml(costStr)}</text>
    <!-- Provider + Period -->
    <text x="24" y="112" font-size="11" fill="#9ca3af">${escapeXml(providerLabel)}  ·  ${escapeXml(periodLabel)}</text>
    <!-- Watermark -->
    <text x="376" y="130" font-size="9" text-anchor="end" fill="#d1d5db">AI Token Monitor</text>
  </g>
</svg>`;
}

function adjustColor(hex: string, amount: number): string {
  const num = parseInt(hex.replace("#", ""), 16);
  const r = Math.max(0, Math.min(255, ((num >> 16) & 0xff) + amount));
  const g = Math.max(0, Math.min(255, ((num >> 8) & 0xff) + amount));
  const b = Math.max(0, Math.min(255, (num & 0xff) + amount));
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, "0")}`;
}
