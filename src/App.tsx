import { useState, useMemo, useEffect } from "react";
import { useCombinedStats } from "./hooks/useCombinedStats";
import { useToday } from "./hooks/useToday";
import { useDateNav } from "./hooks/useDateNav";
import { useUnreadChat } from "./hooks/useUnreadChat";
import { useChatNotification } from "./hooks/useChatNotification";
import { formatTokens, getTotalTokens } from "./lib/format";
import { SettingsProvider, useSettings } from "./contexts/SettingsContext";
import { AuthProvider, useAuth } from "./contexts/AuthContext";
import { MiniProfileProvider } from "./contexts/MiniProfileContext";
import { I18nProvider, useI18n } from "./i18n/I18nContext";
import { PopoverShell } from "./components/PopoverShell";
import { Header } from "./components/Header";
import { TabBar } from "./components/TabBar";
import type { TabType } from "./components/TabBar";
import { KiroBreakdown } from "./components/KiroBreakdown";
import { TodaySummary } from "./components/TodaySummary";
import { DailyChart } from "./components/DailyChart";
import { Heatmap } from "./components/Heatmap";
import { ModelBreakdown } from "./components/ModelBreakdown";
import { PeriodTotals } from "./components/PeriodTotals";
import { CacheEfficiency } from "./components/CacheEfficiency";
import { Leaderboard } from "./components/Leaderboard";
import { LeaderboardUploader } from "./components/LeaderboardUploader";
import { ServerHistoryRestorer } from "./components/ServerHistoryRestorer";
import { ChatRoom } from "./components/ChatRoom";
import { ActivityGraph } from "./components/ActivityGraph";
import { SupportBanner } from "./components/SupportBanner";
import { SourceSelector } from "./components/SourceSelector";
import { SalaryComparator } from "./components/SalaryComparator";
import { UsageAlertBar } from "./components/UsageAlertBar";
import { MiniProfile } from "./components/MiniProfile";
import { OAuthFallbackModal } from "./components/OAuthFallbackModal";
import { StarPromptToast } from "./components/StarPromptToast";
import { OnboardingOverlay } from "./components/OnboardingOverlay";
import { OnboardingProvider } from "./contexts/OnboardingContext";
import { AnalyticsSubTabs } from "./components/AnalyticsSubTabs";
import type { AnalyticsSubTab } from "./components/AnalyticsSubTabs";
import { ProjectBreakdown } from "./components/ProjectBreakdown";
import { ToolUsage } from "./components/ToolUsage";
import { ShellCommands } from "./components/ShellCommands";
import { AnalyticsSummary } from "./components/AnalyticsSummary";
import { ActivityBreakdown } from "./components/ActivityBreakdown";
import { useUpdater } from "./hooks/useUpdater";
import { setChatChannelUser, activateChatChannel } from "./realtime/chatChannel";

function AnalyticsEmptyState({ message }: { message: string }) {
  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: "32px 16px",
      boxShadow: "var(--shadow-card)",
      textAlign: "center",
      color: "var(--text-secondary)",
      fontSize: 12,
    }}>
      {message}
    </div>
  );
}

function AppContent() {
  // Escape → hide-window handling lives in main.tsx (module level) so it
  // survives React crashes; modal handlers still preempt it via capture-phase
  // preventDefault.
  const { prefs } = useSettings();
  const { stats, error, loading, cursorStats, cursorError } = useCombinedStats({
    includeClaude: prefs.include_claude,
    includeCodex: prefs.include_codex,
    includeOpencode: prefs.include_opencode,
    includeKimi: prefs.include_kimi,
    includeGlm: prefs.include_glm,
    includeGjc: prefs.include_gjc,
    includeGrok: prefs.include_grok,
    includeKiro: prefs.include_kiro,
    includeCursor: prefs.include_cursor,
    cursorProfiles: prefs.cursor_profiles,
  });
  const t = useI18n();
  const { user, profile } = useAuth();
  const updater = useUpdater();
  const [activeTab, setActiveTab] = useState<TabType>("overview");
  const [analyticsSubTab, setAnalyticsSubTab] = useState<AnalyticsSubTab>("usage");
  const [chatActivated, setChatActivated] = useState(false);
  const [leaderboardActivated, setLeaderboardActivated] = useState(false);
  const todayStr = useToday();
  const { unreadCount } = useUnreadChat(activeTab === "chat", user?.id ?? null);

  useChatNotification({
    isChatActive: activeTab === "chat",
    currentNickname: profile?.nickname ?? null,
    currentUserId: user?.id ?? null,
  });

  useEffect(() => {
    if (activeTab === "chat") setChatActivated(true);
    if (activeTab === "leaderboard") setLeaderboardActivated(true);
  }, [activeTab]);

  // Turning the Kiro source off removes its sub-tab button, which would leave
  // `analyticsSubTab` pointing at a tab the user can no longer see or leave.
  useEffect(() => {
    if (!prefs.include_kiro) {
      setAnalyticsSubTab((tab) => (tab === "kiro" ? "usage" : tab));
    }
  }, [prefs.include_kiro]);

  // Drive the unified chat realtime channel. Activation is gated only by
  // login state; RLS on chat_messages/chat_reactions enforces the actual
  // access policy on the server. This replaces three independent Realtime
  // channels that each reconnected on every visibilitychange, fixing IO
  // Budget exhaustion on Supabase Free/Nano.
  useEffect(() => {
    setChatChannelUser(user?.id ?? null);
  }, [user?.id]);
  useEffect(() => {
    activateChatChannel(!!user);
  }, [user]);

  // Earliest local date with data — the lower bound for day navigation.
  const minDataDate = useMemo(() => {
    if (!stats || stats.daily.length === 0) return null;
    return stats.daily.reduce((min, d) => (d.date < min ? d.date : min), stats.daily[0].date);
  }, [stats]);

  // Per-day token totals shared by every date-navigation calendar.
  const dailyTokens = useMemo(() => {
    const map: Record<string, number> = {};
    for (const d of stats?.daily ?? []) map[d.date] = getTotalTokens(d.tokens);
    return map;
  }, [stats]);

  const dateNav = useDateNav(todayStr, minDataDate);
  const selectedDate = dateNav.date;

  const cursorRollup = useMemo(() => {
    if (!cursorStats || cursorStats.daily.length === 0) return null;
    return {
      total: cursorStats.daily.reduce((sum, day) => sum + getTotalTokens(day.tokens), 0),
      lastDate: cursorStats.daily[cursorStats.daily.length - 1].date,
    };
  }, [cursorStats]);
  const { today, weekAvg } = useMemo(() => {
    if (!stats) return { today: null, weekAvg: 0 };

    const today = stats.daily.find((d) => d.date === selectedDate) ?? null;

    // 7 days preceding the selected date, so the "vs 7d" chip stays meaningful
    // while browsing past days.
    const last7 = stats.daily
      .filter((d) => {
        const diff = (new Date(selectedDate).getTime() - new Date(d.date).getTime()) / 86400000;
        return diff >= 1 && diff <= 7;
      })
      .map((d) => getTotalTokens(d.tokens));

    const weekAvg = last7.length > 0
      ? last7.reduce((a, b) => a + b, 0) / last7.length
      : 0;

    return { today, weekAvg };
  }, [stats, selectedDate]);

  if (loading && !stats) {
    return (
      <PopoverShell>
        <Header updater={updater} />
        <SourceSelector />
        <div style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flex: 1,
          color: "var(--text-secondary)",
          fontSize: 13,
          fontWeight: 600,
        }}>
          {t("app.loading")}
        </div>
      </PopoverShell>
    );
  }

  if (error || !stats) {
    return (
      <PopoverShell>
        <Header updater={updater} />
        <SourceSelector />
        <div style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          flex: 1,
          gap: 8,
          color: "var(--text-secondary)",
          fontSize: 12,
          fontWeight: 600,
          textAlign: "center",
          padding: 20,
        }}>
          <div style={{ fontSize: 24 }}>
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="12" cy="12" r="10"/>
              <path d="M12 8v4M12 16h.01"/>
            </svg>
          </div>
          <div>{t("app.error.title")}</div>
          <div style={{ fontSize: 10, color: "var(--text-secondary)" }}>
            {t("app.error.description")}
          </div>
        </div>
      </PopoverShell>
    );
  }

  return (
    <PopoverShell>
      <Header stats={stats} updater={updater} />
      <SourceSelector />
      {prefs.include_cursor && (
        <div style={{
          marginTop: -2,
          padding: "6px 8px",
          borderRadius: 7,
          border: "1px solid rgba(245, 158, 11, 0.22)",
          background: "rgba(245, 158, 11, 0.08)",
          color: "var(--text-secondary)",
          fontSize: 9.5,
          lineHeight: 1.4,
        }}>
          {cursorStats?.warnings?.length
            ? t("cursor.publicPartial")
            : cursorRollup
              ? t("cursor.publicSummary", {
                profiles: prefs.cursor_profiles.length,
                tokens: formatTokens(cursorRollup.total, prefs.number_format),
                date: cursorRollup.lastDate,
              })
              : cursorError
                ? t("cursor.publicError")
                : t("cursor.publicNotice")}
        </div>
      )}
      <TabBar activeTab={activeTab} onChange={setActiveTab} chatBadge={unreadCount} />

      {/* Keep mounted tabs alive to avoid remount/recalculation on switch */}
      <div style={{ display: activeTab === "overview" ? "contents" : "none" }}>
        <TodaySummary today={today} weekAvg={weekAvg} nav={dateNav} dailyTokens={dailyTokens} />
        <UsageAlertBar />
        <SalaryComparator stats={stats} />
        <DailyChart daily={stats.daily} days={7} />
        <PeriodTotals daily={stats.daily} />
        <Heatmap daily={stats.daily} weeks={8} />
      </div>

      <div style={{ display: activeTab === "analytics" ? "contents" : "none" }}>
        <AnalyticsSubTabs active={analyticsSubTab} onChange={setAnalyticsSubTab} showKiro={prefs.include_kiro} />

        {analyticsSubTab === "usage" && (
          <>
            <AnalyticsSummary stats={stats} />
            <ActivityGraph daily={stats.daily} />
            <DailyChart daily={stats.daily} days={30} />
            <PeriodTotals daily={stats.daily} />
            {stats.analytics && stats.analytics.activity_breakdown.length > 0 && (
              <ActivityBreakdown data={stats.analytics.activity_breakdown} />
            )}
            <ModelBreakdown modelUsage={stats.model_usage} />
            <CacheEfficiency stats={stats} />
          </>
        )}

        {analyticsSubTab === "projects" && (
          stats.analytics && stats.analytics.project_usage.length > 0
            ? <ProjectBreakdown data={stats.analytics.project_usage} />
            : <AnalyticsEmptyState message={t("analytics.empty.projects")} />
        )}

        {analyticsSubTab === "tools" && (
          stats.analytics && stats.analytics.tool_usage.length > 0
            ? <>
                <ToolUsage data={stats.analytics.tool_usage} />
                <ShellCommands
                  commands={stats.analytics.shell_commands}
                  mcp={stats.analytics.mcp_usage}
                />
              </>
            : <AnalyticsEmptyState message={t("analytics.empty.tools")} />
        )}

        {analyticsSubTab === "kiro" && prefs.include_kiro && <KiroBreakdown />}
      </div>

      {/* Leaderboard: defers mount (and its network requests) until first visit,
          then stays mounted behind a CSS toggle so the hooks' 30-min caches and
          poll timers survive tab switches instead of refetching on every entry. */}
      {leaderboardActivated && (
        <div style={{ display: activeTab === "leaderboard" ? "contents" : "none" }}>
          <Leaderboard minDate={minDataDate} dailyTokens={dailyTokens} />
        </div>
      )}

      {/* Chat: always mounted but hidden via CSS; defers fetch until first visit */}
      <div style={{
        flex: 1,
        minHeight: 0,
        display: activeTab === "chat" ? "flex" : "none",
        flexDirection: "column" as const,
      }}>
        <ChatRoom activated={chatActivated} visible={activeTab === "chat"} />
      </div>

      <SupportBanner />
      <MiniProfile localDaily={stats.daily} currentUserId={user?.id ?? null} />
      {/* Headless: keeps all enabled providers' snapshot history in sync
          regardless of whether the Leaderboard tab is open. */}
      <LeaderboardUploader />

      {/* Pulls the signed-in user's daily history from the server so a fresh
          install shows past usage; clears it on sign-out. */}
      <ServerHistoryRestorer />
      <OAuthFallbackModal />
      {/* Occasional GitHub star ask; useStarPrompt gates how rarely it shows. */}
      <StarPromptToast daily={stats.daily} />

      {/* First-run feature tour; auto-shows only its opt-in welcome card. */}
      <OnboardingOverlay activeTab={activeTab} onRequestTab={setActiveTab} />
    </PopoverShell>
  );
}

function I18nBridge({ children }: { children: React.ReactNode }) {
  const { prefs } = useSettings();
  return (
    <I18nProvider locale={prefs.language}>
      {children}
    </I18nProvider>
  );
}

function App() {
  return (
    <SettingsProvider>
      <I18nBridge>
        <AuthProvider>
          <MiniProfileProvider>
            <OnboardingProvider>
              <AppContent />
            </OnboardingProvider>
          </MiniProfileProvider>
        </AuthProvider>
      </I18nBridge>
    </SettingsProvider>
  );
}

export default App;
