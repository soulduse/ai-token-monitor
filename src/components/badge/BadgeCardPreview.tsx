import { forwardRef } from "react";
import type { BadgeData } from "../../hooks/useBadgeData";
import { formatTokens, formatCost } from "../../lib/format";
import { PROVIDER_LABELS, PERIOD_LABELS } from "../../lib/badgeSvgTemplate";

interface Props {
  data: BadgeData;
  theme: "light" | "dark";
}

const PROVIDER_GRADIENTS: Record<string, string> = {
  claude: "linear-gradient(135deg, #7C5CFC, #5A3DE6)",
  codex: "linear-gradient(135deg, #0ea5e9, #0284c7)",
  opencode: "linear-gradient(135deg, #d97706, #b45309)",
  grok: "linear-gradient(135deg, #64748b, #334155)",
  kimi: "linear-gradient(135deg, #1a73e8, #1557b0)",
  glm: "linear-gradient(135deg, #00b96b, #008f52)",
  gjc: "linear-gradient(135deg, #e11d48, #be123c)",
  kiro: "linear-gradient(135deg, #7C5CFC, #5A3DE6)",
};

function getRankDisplay(rank: number): string {
  if (rank === 1) return "🥇";
  if (rank === 2) return "🥈";
  if (rank === 3) return "🥉";
  return `#${rank}`;
}

export const BadgeCardPreview = forwardRef<HTMLDivElement, Props>(({ data, theme }, ref) => {
  const isDark = theme === "dark";
  const gradient = PROVIDER_GRADIENTS[data.provider];
  const rankDisplay = getRankDisplay(data.rank);

  const cardStyle: React.CSSProperties = {
    width: 360,
    height: 200,
    borderRadius: 16,
    padding: 24,
    display: "flex",
    flexDirection: "column",
    justifyContent: "space-between",
    fontFamily: "'Nunito', 'Verdana', sans-serif",
    position: "relative",
    overflow: "hidden",
    flexShrink: 0,
    ...(isDark
      ? { background: gradient, color: "#fff" }
      : {
          background: "#ffffff",
          color: "#1a1a1a",
          border: "1px solid #e5e7eb",
          boxShadow: "0 1px 3px rgba(0,0,0,0.08)",
        }),
  };

  const accentColor = isDark ? "rgba(255,255,255,0.6)" : "#9ca3af";
  const pillBg = isDark ? "rgba(255,255,255,0.15)" : "#f3f4f6";
  const pillColor = isDark ? "#fff" : "#374151";

  return (
    <div ref={ref} style={cardStyle}>
      {/* Top: Avatar + Nickname + Rank */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          {data.avatarUrl ? (
            <img
              src={data.avatarUrl}
              alt=""
              style={{ width: 36, height: 36, borderRadius: "50%", border: `2px solid ${isDark ? "rgba(255,255,255,0.3)" : "#e5e7eb"}` }}
              crossOrigin="anonymous"
            />
          ) : (
            <div style={{
              width: 36,
              height: 36,
              borderRadius: "50%",
              background: isDark ? "rgba(255,255,255,0.2)" : "#e5e7eb",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 16,
              fontWeight: 700,
            }}>
              {data.nickname.charAt(0).toUpperCase()}
            </div>
          )}
          <div style={{ fontSize: 16, fontWeight: 800 }}>{data.nickname}</div>
        </div>
        <div style={{ fontSize: data.rank <= 3 ? 28 : 22, fontWeight: 800 }}>
          {rankDisplay}
        </div>
      </div>

      {/* Center: Tokens + Cost */}
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
        <div>
          <div style={{ fontSize: 28, fontWeight: 800, letterSpacing: "-1px", lineHeight: 1 }}>
            {formatTokens(data.totalTokens)}
          </div>
          <div style={{ fontSize: 11, fontWeight: 600, color: accentColor, marginTop: 2 }}>
            tokens
          </div>
        </div>
        <div style={{ fontSize: 18, fontWeight: 700, color: accentColor }}>
          {formatCost(data.costUsd)}
        </div>
      </div>

      {/* Bottom: Provider + Period + Watermark */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div style={{ display: "flex", gap: 6 }}>
          <span style={{
            fontSize: 10,
            fontWeight: 700,
            padding: "3px 8px",
            borderRadius: 6,
            background: pillBg,
            color: pillColor,
          }}>
            {PROVIDER_LABELS[data.provider]}
          </span>
          <span style={{
            fontSize: 10,
            fontWeight: 700,
            padding: "3px 8px",
            borderRadius: 6,
            background: pillBg,
            color: pillColor,
          }}>
            {PERIOD_LABELS[data.period]}
          </span>
        </div>
        <div style={{ fontSize: 9, fontWeight: 600, color: accentColor, opacity: 0.7 }}>
          AI Token Monitor
        </div>
      </div>
    </div>
  );
});

BadgeCardPreview.displayName = "BadgeCardPreview";
