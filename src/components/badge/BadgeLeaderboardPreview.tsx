import { forwardRef } from "react";
import type { LeaderboardEntry } from "../../hooks/useLeaderboardSync";
import type { LeaderboardView } from "../../lib/types";
import { formatTokens, formatCost } from "../../lib/format";
import { PROVIDER_LABELS, PERIOD_LABELS } from "../../lib/badgeSvgTemplate";

interface Props {
  leaderboard: LeaderboardEntry[];
  userId: string;
  provider: LeaderboardView;
  period: "today" | "week" | "month";
  theme: "light" | "dark";
}

const MEDALS = ["", "\ud83e\udd47", "\ud83e\udd48", "\ud83e\udd49"];
const MAX_ROWS = 10;

export const BadgeLeaderboardPreview = forwardRef<HTMLDivElement, Props>(
  ({ leaderboard, userId, provider, period, theme }, ref) => {
    const isDark = theme === "dark";
    const myIndex = leaderboard.findIndex((e) => e.user_id === userId);

    // Show top MAX_ROWS, but if user is outside that range, include them with a separator
    let entries: { entry: LeaderboardEntry; rank: number; isSeparator?: boolean }[] = [];
    const topN = leaderboard.slice(0, MAX_ROWS);
    entries = topN.map((entry, i) => ({ entry, rank: i + 1 }));

    if (myIndex >= MAX_ROWS) {
      entries.push({ entry: leaderboard[myIndex], rank: myIndex + 1, isSeparator: true });
    }

    const bg = isDark ? "#1a1a2e" : "#ffffff";
    const textPrimary = isDark ? "#ffffff" : "#1a1a1a";
    const textSecondary = isDark ? "rgba(255,255,255,0.5)" : "#9ca3af";
    const rowHoverBg = isDark ? "rgba(124,92,252,0.12)" : "rgba(124,92,252,0.06)";
    const borderColor = isDark ? "rgba(255,255,255,0.08)" : "#f3f4f6";
    const accentColor = "#7C5CFC";

    return (
      <div
        ref={ref}
        style={{
          width: 360,
          borderRadius: 16,
          padding: "16px 0",
          background: bg,
          fontFamily: "'Nunito', 'Verdana', sans-serif",
          overflow: "hidden",
          flexShrink: 0,
          border: isDark ? "none" : "1px solid #e5e7eb",
          boxShadow: isDark ? "none" : "0 1px 3px rgba(0,0,0,0.08)",
        }}
      >
        {/* Header */}
        <div style={{
          padding: "0 16px 12px",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          borderBottom: `1px solid ${borderColor}`,
        }}>
          <div>
            <div style={{ fontSize: 14, fontWeight: 800, color: textPrimary }}>
              🏆 Leaderboard
            </div>
            <div style={{ fontSize: 10, fontWeight: 600, color: textSecondary, marginTop: 2 }}>
              {PROVIDER_LABELS[provider]} · {PERIOD_LABELS[period]}
            </div>
          </div>
          <div style={{ fontSize: 9, fontWeight: 600, color: textSecondary }}>
            AI Token Monitor
          </div>
        </div>

        {/* Rows */}
        <div style={{ padding: "8px 0" }}>
          {entries.map(({ entry, rank, isSeparator }) => {
            const isMe = entry.user_id === userId;
            return (
              <div key={entry.user_id}>
                {isSeparator && (
                  <div style={{
                    textAlign: "center",
                    padding: "4px 0",
                    fontSize: 10,
                    color: textSecondary,
                    letterSpacing: 2,
                  }}>
                    ···
                  </div>
                )}
                <div style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "5px 16px",
                  background: isMe ? rowHoverBg : "transparent",
                  borderLeft: isMe ? `3px solid ${accentColor}` : "3px solid transparent",
                }}>
                  {/* Rank */}
                  <div style={{
                    width: 22,
                    textAlign: "center",
                    fontSize: rank <= 3 ? 14 : 11,
                    fontWeight: 800,
                    color: rank <= 3 ? undefined : textSecondary,
                    flexShrink: 0,
                  }}>
                    {rank <= 3 ? MEDALS[rank] : rank}
                  </div>

                  {/* Avatar */}
                  {entry.avatar_url ? (
                    <img
                      src={entry.avatar_url}
                      alt=""
                      crossOrigin="anonymous"
                      style={{ width: 22, height: 22, borderRadius: "50%", flexShrink: 0 }}
                    />
                  ) : (
                    <div style={{
                      width: 22,
                      height: 22,
                      borderRadius: "50%",
                      background: isDark ? "rgba(255,255,255,0.1)" : "#e5e7eb",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      fontSize: 10,
                      fontWeight: 700,
                      color: accentColor,
                      flexShrink: 0,
                    }}>
                      {entry.nickname.charAt(0).toUpperCase()}
                    </div>
                  )}

                  {/* Name */}
                  <div style={{
                    flex: 1,
                    fontSize: 11,
                    fontWeight: isMe ? 800 : 600,
                    color: isMe ? accentColor : textPrimary,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}>
                    {entry.nickname}
                  </div>

                  {/* Tokens + Cost */}
                  <div style={{ textAlign: "right", flexShrink: 0 }}>
                    <div style={{
                      fontSize: 11,
                      fontWeight: 800,
                      color: isMe ? accentColor : textPrimary,
                    }}>
                      {formatTokens(entry.total_tokens)}
                    </div>
                    <div style={{ fontSize: 9, fontWeight: 600, color: textSecondary }}>
                      {formatCost(entry.cost_usd)}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    );
  },
);

BadgeLeaderboardPreview.displayName = "BadgeLeaderboardPreview";
