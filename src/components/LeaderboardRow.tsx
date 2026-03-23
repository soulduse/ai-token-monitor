import { openUrl } from "@tauri-apps/plugin-opener";
import type { LeaderboardEntry } from "../hooks/useLeaderboardSync";
import { formatTokens, formatCost } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  entry: LeaderboardEntry;
  rank: number;
  isMe: boolean;
}

const MEDALS = ["", "\ud83e\udd47", "\ud83e\udd48", "\ud83e\udd49"];

export function LeaderboardRow({ entry, rank, isMe }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      gap: 10,
      padding: "8px 10px",
      borderRadius: "var(--radius-sm)",
      background: isMe ? "rgba(124, 92, 252, 0.08)" : "transparent",
      border: isMe ? "1px solid rgba(124, 92, 252, 0.15)" : "1px solid transparent",
      transition: "background 0.15s ease",
    }}>
      {/* Rank */}
      <div style={{
        width: 24,
        textAlign: "center",
        fontSize: rank <= 3 ? 16 : 12,
        fontWeight: 800,
        color: rank <= 3 ? undefined : "var(--text-secondary)",
        flexShrink: 0,
      }}>
        {rank <= 3 ? MEDALS[rank] : rank}
      </div>

      {/* Avatar + Name (clickable → GitHub profile) */}
      <div
        onClick={() => openUrl(`https://github.com/${entry.nickname}`)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          flex: 1,
          minWidth: 0,
          cursor: "pointer",
        }}
      >
        {entry.avatar_url ? (
          <img
            src={entry.avatar_url}
            alt=""
            style={{
              width: 28,
              height: 28,
              borderRadius: 14,
              flexShrink: 0,
            }}
          />
        ) : (
          <div style={{
            width: 28,
            height: 28,
            borderRadius: 14,
            background: "var(--heat-1)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontSize: 12,
            fontWeight: 700,
            color: "var(--accent-purple)",
            flexShrink: 0,
          }}>
            {entry.nickname.charAt(0).toUpperCase()}
          </div>
        )}

        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{
            fontSize: 12,
            fontWeight: 700,
            color: isMe ? "var(--accent-purple)" : "var(--text-primary)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}>
            {entry.nickname}
            {isMe && (
              <span style={{
                fontSize: 9,
                fontWeight: 600,
                color: "var(--accent-purple)",
                marginLeft: 4,
                opacity: 0.7,
              }}>
                {t("leaderboard.you")}
              </span>
            )}
            {/* GitHub link icon */}
            <svg
              width="10" height="10" viewBox="0 0 24 24"
              fill="none" stroke="var(--text-muted)" strokeWidth="2"
              strokeLinecap="round" strokeLinejoin="round"
              style={{ marginLeft: 4, verticalAlign: "middle", opacity: 0.5 }}
            >
              <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
              <polyline points="15 3 21 3 21 9"/>
              <line x1="10" y1="14" x2="21" y2="3"/>
            </svg>
          </div>
          <div style={{
            fontSize: 10,
            color: "var(--text-secondary)",
            fontWeight: 600,
          }}>
            {entry.messages} {t("leaderboard.msgs")}
          </div>
        </div>
      </div>

      {/* Tokens + Cost */}
      <div style={{ textAlign: "right", flexShrink: 0 }}>
        <div style={{
          fontSize: 13,
          fontWeight: 800,
          color: isMe ? "var(--accent-purple)" : "var(--text-primary)",
        }}>
          {formatTokens(entry.total_tokens, prefs.number_format)}
        </div>
        <div style={{
          fontSize: 10,
          color: "var(--text-secondary)",
          fontWeight: 600,
        }}>
          {formatCost(entry.cost_usd)}
        </div>
      </div>
    </div>
  );
}
