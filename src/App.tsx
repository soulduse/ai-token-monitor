import { useState, useMemo } from "react";
import { useTokenStats } from "./hooks/useTokenStats";
import { useToday } from "./hooks/useToday";
import { getTotalTokens } from "./lib/format";
import { SettingsProvider, useSettings } from "./contexts/SettingsContext";
import { AuthProvider } from "./contexts/AuthContext";
import { I18nProvider, useI18n } from "./i18n/I18nContext";
import { PopoverShell } from "./components/PopoverShell";
import { Header } from "./components/Header";
import { TabBar } from "./components/TabBar";
import type { TabType } from "./components/TabBar";
import { TodaySummary } from "./components/TodaySummary";
import { DailyChart } from "./components/DailyChart";
import { Heatmap } from "./components/Heatmap";
import { ModelBreakdown } from "./components/ModelBreakdown";
import { PeriodTotals } from "./components/PeriodTotals";
import { CacheEfficiency } from "./components/CacheEfficiency";
import { Leaderboard } from "./components/Leaderboard";
import { ActivityGraph } from "./components/ActivityGraph";
import { SupportBanner } from "./components/SupportBanner";

function AppContent() {
  const { stats, error, loading } = useTokenStats();
  const t = useI18n();
  const [activeTab, setActiveTab] = useState<TabType>("overview");
  const todayStr = useToday();

  const { today, weekAvg } = useMemo(() => {
    if (!stats) return { today: null, weekAvg: 0 };

    const today = stats.daily.find((d) => d.date === todayStr) ?? null;

    const last7 = stats.daily
      .filter((d) => {
        const diff = (new Date(todayStr).getTime() - new Date(d.date).getTime()) / 86400000;
        return diff >= 1 && diff <= 7;
      })
      .map((d) => getTotalTokens(d.tokens));

    const weekAvg = last7.length > 0
      ? last7.reduce((a, b) => a + b, 0) / last7.length
      : 0;

    return { today, weekAvg };
  }, [stats, todayStr]);

  if (loading) {
    return (
      <PopoverShell>
        <Header />
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
        <Header />
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
      <Header stats={stats} />
      <TabBar activeTab={activeTab} onChange={setActiveTab} />

      {/* Keep mounted tabs alive to avoid remount/recalculation on switch */}
      <div style={{ display: activeTab === "overview" ? "contents" : "none" }}>
        <TodaySummary today={today} weekAvg={weekAvg} />
        <DailyChart daily={stats.daily} days={7} />
        <PeriodTotals daily={stats.daily} />
        <Heatmap daily={stats.daily} weeks={8} />
      </div>

      <div style={{ display: activeTab === "analytics" ? "contents" : "none" }}>
        <ActivityGraph daily={stats.daily} />
        <DailyChart daily={stats.daily} days={30} />
        <PeriodTotals daily={stats.daily} />
        <ModelBreakdown modelUsage={stats.model_usage} />
        <CacheEfficiency stats={stats} />
      </div>

      {/* Leaderboard lazy-loads (network requests), keep conditional */}
      {activeTab === "leaderboard" && (
        <Leaderboard stats={stats} />
      )}

      <SupportBanner />
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
          <AppContent />
        </AuthProvider>
      </I18nBridge>
    </SettingsProvider>
  );
}

export default App;
