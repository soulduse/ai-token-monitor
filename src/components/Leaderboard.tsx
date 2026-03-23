import { useAuth } from "../hooks/useAuth";
import { useLeaderboardSync } from "../hooks/useLeaderboardSync";
import type { AllStats } from "../lib/types";
import type { User } from "@supabase/supabase-js";
import { useSettings } from "../contexts/SettingsContext";
import { LeaderboardRow } from "./LeaderboardRow";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  stats: AllStats;
}

export function Leaderboard({ stats }: Props) {
  const { user, loading: authLoading, signIn, available } = useAuth();
  const { prefs } = useSettings();
  const t = useI18n();

  if (!available) {
    return (
      <div style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-lg)",
        padding: 24,
        boxShadow: "var(--shadow-card)",
        textAlign: "center",
      }}>
        <div style={{ fontSize: 32, marginBottom: 8 }}>🏆</div>
        <div style={{ fontSize: 14, fontWeight: 700, color: "var(--text-primary)", marginBottom: 4 }}>
          {t("leaderboard.comingSoon")}
        </div>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", fontWeight: 600 }}>
          {t("leaderboard.notConfigured")}
        </div>
      </div>
    );
  }

  if (!user || !prefs.leaderboard_opted_in) {
    return <LeaderboardCTA
      onSignIn={signIn}
      loading={authLoading}
      hasUser={!!user}
    />;
  }

  return <LeaderboardContent stats={stats} user={user} />;
}

function LeaderboardCTA({
  onSignIn,
  loading,
  hasUser,
}: {
  onSignIn: () => void;
  loading: boolean;
  hasUser: boolean;
}) {
  const { updatePrefs } = useSettings();
  const t = useI18n();

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 24,
      boxShadow: "var(--shadow-card)",
      textAlign: "center",
      display: "flex",
      flexDirection: "column",
      alignItems: "center",
      gap: 12,
    }}>
      <div style={{ fontSize: 40 }}>🏆</div>
      <div style={{
        fontSize: 16,
        fontWeight: 800,
        color: "var(--text-primary)",
      }}>
        {t("leaderboard.join")}
      </div>
      <div style={{
        fontSize: 12,
        color: "var(--text-secondary)",
        fontWeight: 600,
        maxWidth: 260,
        lineHeight: 1.5,
      }}>
        {t("leaderboard.description")}
      </div>

      {hasUser ? (
        <button
          onClick={() => updatePrefs({ leaderboard_opted_in: true })}
          style={{
            padding: "10px 24px",
            fontSize: 13,
            fontWeight: 700,
            border: "none",
            borderRadius: "var(--radius-sm)",
            cursor: "pointer",
            background: "linear-gradient(135deg, var(--accent-purple), var(--accent-pink))",
            color: "#fff",
          }}
        >
          {t("leaderboard.enable")}
        </button>
      ) : (
        <button
          onClick={onSignIn}
          disabled={loading}
          style={{
            padding: "10px 24px",
            fontSize: 13,
            fontWeight: 700,
            border: "none",
            borderRadius: "var(--radius-sm)",
            cursor: loading ? "wait" : "pointer",
            background: "#24292e",
            color: "#fff",
            display: "flex",
            alignItems: "center",
            gap: 8,
            opacity: loading ? 0.7 : 1,
          }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/>
          </svg>
          {t("leaderboard.signIn")}
        </button>
      )}
    </div>
  );
}

function LeaderboardContent({ stats, user }: { stats: AllStats; user: User }) {
  const t = useI18n();
  const { leaderboard, loading, period, setPeriod } = useLeaderboardSync({
    stats,
    user,
    optedIn: true,
  });

  const myRank = leaderboard.findIndex((e) => e.user_id === user.id) + 1;

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      gap: 14,
    }}>
      {/* Period toggle */}
      <div style={{
        display: "flex",
        background: "var(--heat-0)",
        borderRadius: 6,
        padding: 2,
        alignSelf: "center",
      }}>
        {(["today", "week"] as const).map((p) => (
          <button
            key={p}
            onClick={() => setPeriod(p)}
            style={{
              fontSize: 11,
              fontWeight: 600,
              padding: "4px 14px",
              borderRadius: 4,
              border: "none",
              cursor: "pointer",
              background: period === p ? "var(--accent-purple)" : "transparent",
              color: period === p ? "#fff" : "var(--text-secondary)",
              transition: "all 0.15s ease",
            }}
          >
            {p === "today" ? t("leaderboard.today") : t("leaderboard.thisWeek")}
          </button>
        ))}
      </div>

      {/* My rank card */}
      {myRank > 0 && (
        <div style={{
          background: "linear-gradient(135deg, rgba(124,92,252,0.08), rgba(255,143,164,0.08))",
          borderRadius: "var(--radius-lg)",
          padding: "12px 16px",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 6,
          border: "1px solid rgba(124, 92, 252, 0.1)",
        }}>
          <span style={{ fontSize: 11, color: "var(--text-secondary)", fontWeight: 600 }}>
            {t("leaderboard.yourRank")}
          </span>
          <span style={{ fontSize: 20, fontWeight: 800, color: "var(--accent-purple)" }}>
            #{myRank}
          </span>
          <span style={{ fontSize: 11, color: "var(--text-secondary)", fontWeight: 600 }}>
            {t("leaderboard.of")} {leaderboard.length}
          </span>
        </div>
      )}

      {/* Leaderboard list */}
      <div style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-lg)",
        padding: 8,
        boxShadow: "var(--shadow-card)",
      }}>
        {loading && leaderboard.length === 0 ? (
          <div style={{
            padding: 20,
            textAlign: "center",
            color: "var(--text-secondary)",
            fontSize: 12,
            fontWeight: 600,
          }}>
            {t("leaderboard.loading")}
          </div>
        ) : leaderboard.length === 0 ? (
          <div style={{
            padding: 20,
            textAlign: "center",
            color: "var(--text-secondary)",
            fontSize: 12,
            fontWeight: 600,
          }}>
            {t("leaderboard.noData")}
          </div>
        ) : (
          leaderboard.map((entry, i) => (
            <LeaderboardRow
              key={entry.user_id}
              entry={entry}
              rank={i + 1}
              isMe={entry.user_id === user.id}
            />
          ))
        )}
      </div>
    </div>
  );
}
