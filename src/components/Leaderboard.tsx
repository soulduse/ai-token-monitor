import { useState, useEffect, useMemo } from "react";
import { useAuth } from "../hooks/useAuth";
import { useLeaderboardSync } from "../hooks/useLeaderboardSync";
import { useTokenStats } from "../hooks/useTokenStats";
import type { LeaderboardProvider } from "../lib/types";
import type { User } from "@supabase/supabase-js";
import { useSettings } from "../contexts/SettingsContext";
import { LeaderboardRow } from "./LeaderboardRow";
import { useI18n } from "../i18n/I18nContext";

export function Leaderboard() {
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

  return <LeaderboardContent user={user} />;
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

function LeaderboardContent({ user }: { user: User }) {
  const t = useI18n();
  const { prefs } = useSettings();
  const [provider, setProvider] = useState<LeaderboardProvider>("claude");

  // Determine available provider tabs
  const availableProviders: LeaderboardProvider[] = [];
  if (prefs.include_claude) availableProviders.push("claude");
  if (prefs.include_codex) availableProviders.push("codex");
  if (prefs.include_opencode) availableProviders.push("opencode");
  // Default to claude if nothing enabled
  if (availableProviders.length === 0) availableProviders.push("claude");

  // Ensure selected provider is valid
  const activeProvider = availableProviders.includes(provider) ? provider : availableProviders[0];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {/* Provider tabs — only show if multiple providers */}
      {availableProviders.length > 1 && (
        <div style={{
          display: "flex",
          background: "var(--heat-0)",
          borderRadius: 6,
          padding: 2,
          alignSelf: "center",
        }}>
          {availableProviders.map((p) => (
            <button
              key={p}
              onClick={() => setProvider(p)}
              style={{
                fontSize: 11,
                fontWeight: 600,
                padding: "4px 14px",
                borderRadius: 4,
                border: "none",
                cursor: "pointer",
                background: activeProvider === p ? "var(--accent-purple)" : "transparent",
                color: activeProvider === p ? "#fff" : "var(--text-secondary)",
                transition: "all 0.15s ease",
              }}
            >
              {t(`sources.${p}`)}
            </button>
          ))}
        </div>
      )}

      <ProviderLeaderboard
        provider={activeProvider}
        user={user}
      />
    </div>
  );
}

const PAGE_SIZE = 20;

function ProviderLeaderboard({
  provider,
  user,
}: {
  provider: LeaderboardProvider;
  user: User;
}) {
  const t = useI18n();
  const { stats } = useTokenStats(provider);
  const { leaderboard, loading, period, setPeriod } = useLeaderboardSync({
    stats,
    user,
    optedIn: true,
    provider,
  });

  const [page, setPage] = useState(0);
  const totalPages = Math.max(1, Math.ceil(leaderboard.length / PAGE_SIZE));
  const myRank = leaderboard.findIndex((e) => e.user_id === user.id) + 1;

  // Reset to page 0 when period or provider changes
  useEffect(() => { setPage(0); }, [period, provider]);

  // Clamp page if data shrinks
  useEffect(() => {
    if (page >= totalPages) setPage(Math.max(0, totalPages - 1));
  }, [totalPages, page]);

  const pageEntries = useMemo(
    () => leaderboard.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE),
    [leaderboard, page],
  );

  const goToMyPage = () => {
    if (myRank > 0) setPage(Math.floor((myRank - 1) / PAGE_SIZE));
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {/* Period toggle */}
      <div style={{
        display: "flex",
        background: "var(--heat-0)",
        borderRadius: 6,
        padding: 2,
        alignSelf: "center",
      }}>
        {(["today", "week", "month"] as const).map((p) => (
          <button
            key={p}
            onClick={() => setPeriod(p)}
            style={{
              fontSize: 11,
              fontWeight: 600,
              padding: "4px 12px",
              borderRadius: 4,
              border: "none",
              cursor: "pointer",
              background: period === p ? "var(--accent-purple)" : "transparent",
              color: period === p ? "#fff" : "var(--text-secondary)",
              transition: "all 0.15s ease",
            }}
          >
            {{ today: t("leaderboard.today"), week: t("leaderboard.thisWeek"), month: t("leaderboard.thisMonth") }[p]}
          </button>
        ))}
      </div>

      {/* My rank card */}
      {myRank > 0 && (
        <div
          onClick={goToMyPage}
          style={{
            background: "linear-gradient(135deg, rgba(124,92,252,0.08), rgba(255,143,164,0.08))",
            borderRadius: "var(--radius-lg)",
            padding: "12px 16px",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 6,
            border: "1px solid rgba(124, 92, 252, 0.1)",
            cursor: "pointer",
          }}
        >
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
          pageEntries.map((entry, i) => (
            <LeaderboardRow
              key={entry.user_id}
              entry={entry}
              rank={page * PAGE_SIZE + i + 1}
              isMe={entry.user_id === user.id}
            />
          ))
        )}
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <Pagination page={page} totalPages={totalPages} onPageChange={setPage} />
      )}
    </div>
  );
}

function Pagination({
  page,
  totalPages,
  onPageChange,
}: {
  page: number;
  totalPages: number;
  onPageChange: (p: number) => void;
}) {
  const pages = useMemo(() => {
    const items: (number | "...")[] = [];
    if (totalPages <= 7) {
      for (let i = 0; i < totalPages; i++) items.push(i);
    } else {
      items.push(0);
      if (page > 2) items.push("...");
      for (let i = Math.max(1, page - 1); i <= Math.min(totalPages - 2, page + 1); i++) {
        items.push(i);
      }
      if (page < totalPages - 3) items.push("...");
      items.push(totalPages - 1);
    }
    return items;
  }, [page, totalPages]);

  const btnBase: React.CSSProperties = {
    fontSize: 11,
    fontWeight: 600,
    border: "none",
    borderRadius: 4,
    cursor: "pointer",
    background: "transparent",
    color: "var(--text-secondary)",
    padding: "4px 8px",
    minWidth: 28,
    transition: "all 0.15s ease",
  };

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      gap: 2,
    }}>
      <button
        onClick={() => onPageChange(Math.max(0, page - 1))}
        disabled={page === 0}
        style={{ ...btnBase, opacity: page === 0 ? 0.3 : 1, cursor: page === 0 ? "default" : "pointer" }}
      >
        ‹
      </button>
      {pages.map((p, i) =>
        p === "..." ? (
          <span key={`dots-${i}`} style={{ ...btnBase, cursor: "default" }}>…</span>
        ) : (
          <button
            key={p}
            onClick={() => onPageChange(p)}
            style={{
              ...btnBase,
              background: p === page ? "var(--accent-purple)" : "transparent",
              color: p === page ? "#fff" : "var(--text-secondary)",
            }}
          >
            {p + 1}
          </button>
        ),
      )}
      <button
        onClick={() => onPageChange(Math.min(totalPages - 1, page + 1))}
        disabled={page === totalPages - 1}
        style={{ ...btnBase, opacity: page === totalPages - 1 ? 0.3 : 1, cursor: page === totalPages - 1 ? "default" : "pointer" }}
      >
        ›
      </button>
    </div>
  );
}
