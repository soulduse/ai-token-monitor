import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useOAuthUsage } from "../hooks/useOAuthUsage";
import { useTokenStats } from "../hooks/useTokenStats";
import { useToday } from "../hooks/useToday";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";
import type { AllStats, RateLimitWindow } from "../lib/types";
import { formatCost, formatTokens, getTotalTokens } from "../lib/format";

const REFRESH_COOLDOWN_SECONDS = 30;

function getBarColor(percent: number): string {
  if (percent >= 90) return "#ef4444";
  if (percent >= 80) return "#f97316";
  if (percent >= 50) return "#eab308";
  return "#22c55e";
}

function formatResetTime(resetsAt: string, t: (key: string, params?: Record<string, string>) => string): string {
  const reset = new Date(resetsAt);
  const now = new Date();
  const diffMs = reset.getTime() - now.getTime();
  if (diffMs <= 0) return t("usageAlert.resetsNow");
  const totalMin = Math.floor(diffMs / 60000);
  const d = Math.floor(totalMin / 1440);
  const h = Math.floor((totalMin % 1440) / 60);
  const m = totalMin % 60;
  const parts: string[] = [];
  if (d > 0) parts.push(`${d}d`);
  if (h > 0) parts.push(`${h}h`);
  if (m > 0 || parts.length === 0) parts.push(`${m}m`);
  return t("usageAlert.resetsIn", { time: parts.join(" ") });
}

function formatUnixResetTime(resetsAt: number, t: (key: string, params?: Record<string, string>) => string): string {
  return formatResetTime(new Date(resetsAt * 1000).toISOString(), t);
}

function formatCodexWindowLabel(
  window: RateLimitWindow,
  fallback: string,
  t: (key: string, params?: Record<string, string>) => string,
): string {
  if (window.window_minutes === 300) return t("usageAlert.session");
  if (window.window_minutes === 10_080) return t("usageAlert.weekly");
  if (window.window_minutes >= 1_440 && window.window_minutes % 1_440 === 0) {
    return `${window.window_minutes / 1_440}d`;
  }
  if (window.window_minutes >= 60 && window.window_minutes % 60 === 0) {
    return `${window.window_minutes / 60}h`;
  }
  return fallback;
}

const SEGMENT_COUNT = 10;

interface CodexUsageSummary {
  tokens: number;
  cost: number;
  messages: number;
  sessions: number;
}

function emptySummary(): CodexUsageSummary {
  return { tokens: 0, cost: 0, messages: 0, sessions: 0 };
}

function summarizeCodexStats(
  stats: AllStats | null,
  todayStr: string,
  days: number,
): CodexUsageSummary {
  if (!stats) return emptySummary();

  const todayTime = new Date(`${todayStr}T00:00:00`).getTime();
  return stats.daily.reduce((summary, day) => {
    const dayTime = new Date(`${day.date}T00:00:00`).getTime();
    const diffDays = Math.floor((todayTime - dayTime) / 86_400_000);
    if (diffDays < 0 || diffDays >= days) return summary;

    summary.tokens += getTotalTokens(day.tokens);
    summary.cost += day.cost_usd;
    summary.messages += day.messages;
    summary.sessions += day.sessions;
    return summary;
  }, emptySummary());
}

function UsageRow({
  label,
  utilization,
  subtitle,
}: {
  label: string;
  utilization: number;
  subtitle: string;
}) {
  const pct = Math.min(utilization, 100);
  const color = getBarColor(utilization);
  const filledSegments = Math.round((pct / 100) * SEGMENT_COUNT);

  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 4,
      }}>
        <span style={{ fontSize: 10, fontWeight: 600, color: "var(--text-primary)" }}>
          {label}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
            {subtitle}
          </span>
          <span style={{ fontSize: 11, fontWeight: 700, color }}>
            {utilization.toFixed(1)}%
          </span>
        </div>
      </div>
      <div style={{
        display: "flex",
        gap: 3,
        width: "100%",
        height: 10,
        padding: 2,
        background: "rgba(0,0,0,0.3)",
        borderRadius: 3,
        border: "1px solid rgba(255,255,255,0.08)",
      }}>
        {Array.from({ length: SEGMENT_COUNT }, (_, i) => (
          <div
            key={i}
            style={{
              flex: 1,
              height: "100%",
              borderRadius: 1,
              background: i < filledSegments ? color : "rgba(255,255,255,0.06)",
              boxShadow: i < filledSegments ? `0 0 4px ${color}40` : "none",
              transition: "background 0.3s ease",
            }}
          />
        ))}
      </div>
    </div>
  );
}

function RefreshButton({
  refreshing,
  cooldown,
  onRefresh,
}: {
  refreshing: boolean;
  cooldown: number;
  onRefresh: () => void;
}) {
  const t = useI18n();
  const disabled = refreshing || cooldown > 0;

  return (
    <button
      onClick={onRefresh}
      disabled={disabled}
      title={
        refreshing
          ? t("usageAlert.refreshing")
          : cooldown > 0
          ? `${t("usageAlert.refresh")} (${cooldown}s)`
          : t("usageAlert.refresh")
      }
      aria-label={t("usageAlert.refresh")}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: 18,
        height: 18,
        padding: 0,
        background: "transparent",
        border: "none",
        borderRadius: 3,
        color: "var(--text-muted)",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.4 : 0.8,
        transition: "opacity 0.2s ease, color 0.2s ease",
      }}
      onMouseEnter={(e) => {
        if (!disabled) {
          e.currentTarget.style.color = "var(--text-primary)";
        }
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.color = "var(--text-muted)";
      }}
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{
          animation: refreshing ? "miniProfileSpin 0.8s linear infinite" : "none",
        }}
      >
        <path d="M3 12a9 9 0 0 1 15-6.7L21 8" />
        <path d="M21 3v5h-5" />
        <path d="M21 12a9 9 0 0 1-15 6.7L3 16" />
        <path d="M3 21v-5h5" />
      </svg>
    </button>
  );
}

function ProviderHeader({
  label,
  stale,
  refreshButton,
}: {
  label: string;
  stale?: boolean;
  refreshButton?: ReactNode;
}) {
  const t = useI18n();

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      marginBottom: 8,
    }}>
      <span style={{
        fontSize: 11,
        fontWeight: 700,
        color: "var(--text-primary)",
      }}>
        {label}
      </span>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        {stale && (
          <span style={{
            fontSize: 9,
            fontWeight: 600,
            color: "var(--text-muted)",
          }}>
            {t("usageAlert.stale")}
          </span>
        )}
        {refreshButton}
      </div>
    </div>
  );
}

function CodexUsageRow({
  label,
  summary,
  maxTokens,
}: {
  label: string;
  summary: CodexUsageSummary;
  maxTokens: number;
}) {
  const { prefs } = useSettings();
  const t = useI18n();
  const pct = maxTokens > 0 ? Math.min((summary.tokens / maxTokens) * 100, 100) : 0;
  const filledSegments = Math.round((pct / 100) * SEGMENT_COUNT);

  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 4,
      }}>
        <span style={{ fontSize: 10, fontWeight: 600, color: "var(--text-primary)" }}>
          {label}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
            {formatCost(summary.cost)}
          </span>
          <span style={{ fontSize: 11, fontWeight: 700, color: "var(--accent-purple)" }}>
            {formatTokens(summary.tokens, prefs.number_format)}
          </span>
        </div>
      </div>
      <div style={{
        display: "flex",
        gap: 3,
        width: "100%",
        height: 10,
        padding: 2,
        background: "rgba(0,0,0,0.3)",
        borderRadius: 3,
        border: "1px solid rgba(255,255,255,0.08)",
      }}>
        {Array.from({ length: SEGMENT_COUNT }, (_, i) => (
          <div
            key={i}
            style={{
              flex: 1,
              height: "100%",
              borderRadius: 1,
              background: i < filledSegments ? "var(--accent-purple)" : "rgba(255,255,255,0.06)",
              boxShadow: i < filledSegments ? "0 0 4px rgba(88,166,255,0.25)" : "none",
              transition: "background 0.3s ease",
            }}
          />
        ))}
      </div>
      <div style={{
        marginTop: 3,
        fontSize: 9,
        color: "var(--text-muted)",
        display: "flex",
        justifyContent: "space-between",
      }}>
        <span>{summary.messages.toLocaleString()} {t("analytics.summary.messages")}</span>
        <span>{summary.sessions.toLocaleString()} {t("analytics.summary.sessions")}</span>
      </div>
    </div>
  );
}

function CodexRateLimitRows({
  primary,
  secondary,
}: {
  primary?: RateLimitWindow | null;
  secondary?: RateLimitWindow | null;
}) {
  const t = useI18n();

  return (
    <>
      {primary && (
        <UsageRow
          label={formatCodexWindowLabel(primary, t("usageAlert.session"), t)}
          utilization={primary.used_percent}
          subtitle={formatUnixResetTime(primary.resets_at, t)}
        />
      )}
      {secondary && (
        <UsageRow
          label={formatCodexWindowLabel(secondary, t("usageAlert.weekly"), t)}
          utilization={secondary.used_percent}
          subtitle={formatUnixResetTime(secondary.resets_at, t)}
        />
      )}
    </>
  );
}

function ClaudeTrackingPrompt({
  enabling,
  onEnable,
}: {
  enabling: boolean;
  onEnable: () => Promise<void>;
}) {
  const t = useI18n();

  return (
    <div>
      <ProviderHeader label={t("usageAlert.claude")} />
      <div style={{
        fontSize: 10,
        color: "var(--text-secondary)",
        marginBottom: 10,
        lineHeight: 1.4,
      }}>
        {t("usageTracking.description")}
      </div>
      <button
        onClick={onEnable}
        disabled={enabling}
        style={{
          width: "100%",
          padding: "6px 0",
          fontSize: 11,
          fontWeight: 600,
          color: "var(--text-primary)",
          background: "var(--bg-hover)",
          border: "1px solid var(--border-secondary)",
          borderRadius: "var(--radius-md)",
          cursor: enabling ? "default" : "pointer",
          opacity: enabling ? 0.6 : 1,
          transition: "opacity 0.2s ease",
        }}
      >
        {enabling ? t("usageTracking.enabling") : t("usageTracking.enable")}
      </button>
    </div>
  );
}

export function UsageAlertBar() {
  const { prefs, refreshPrefs } = useSettings();
  const { usage, refreshing, refresh } = useOAuthUsage();
  const { stats: codexStats } = useTokenStats("codex");
  const todayStr = useToday();
  const t = useI18n();
  const [enabling, setEnabling] = useState(false);
  const [cooldown, setCooldown] = useState(0);
  const cooldownTimerRef = useRef<number | null>(null);
  const showClaude = prefs.include_claude;
  const showCodex = prefs.include_codex;
  const enableClaudeTracking = async () => {
    setEnabling(true);
    try {
      await invoke("enable_usage_tracking");
      await refreshPrefs();
    } catch {
      // silently ignore
    } finally {
      setEnabling(false);
    }
  };

  useEffect(() => {
    return () => {
      if (cooldownTimerRef.current !== null) {
        window.clearInterval(cooldownTimerRef.current);
      }
    };
  }, []);

  const handleRefresh = async () => {
    if (refreshing || cooldown > 0) return;
    setCooldown(REFRESH_COOLDOWN_SECONDS);
    if (cooldownTimerRef.current !== null) {
      window.clearInterval(cooldownTimerRef.current);
    }
    cooldownTimerRef.current = window.setInterval(() => {
      setCooldown((prev) => {
        if (prev <= 1) {
          if (cooldownTimerRef.current !== null) {
            window.clearInterval(cooldownTimerRef.current);
            cooldownTimerRef.current = null;
          }
          return 0;
        }
        return prev - 1;
      });
    }, 1000);
    await refresh();
  };

  if (!showClaude && !showCodex) return null;

  const codexToday = summarizeCodexStats(codexStats, todayStr, 1);
  const codexWeek = summarizeCodexStats(codexStats, todayStr, 7);
  const codexMaxTokens = Math.max(codexToday.tokens, codexWeek.tokens, 1);
  const codexRateLimits = codexStats?.rate_limits ?? null;
  const hasCodexRateLimits = !!(codexRateLimits?.primary || codexRateLimits?.secondary);
  const hasCodexSummary = codexWeek.tokens > 0 || codexWeek.cost > 0 || codexWeek.messages > 0;
  // Codex usage must only surface when the source is actually enabled in the
  // selector. Gate on showCodex so disabling Codex hides its gauges even when
  // cached stats / rate limits still have data.
  const hasCodexData = showCodex && (hasCodexRateLimits || hasCodexSummary);

  // Claude-only, tracking never enabled: show the standalone enable card. When
  // Codex is also on, the same enable affordance is rendered inline further
  // down via showClaudePrompt → ClaudeTrackingPrompt, so this branch is
  // deliberately gated on !showCodex to avoid a duplicate prompt.
  if (showClaude && !prefs.usage_tracking_enabled && !showCodex) {
    return (
      <div style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-lg)",
        padding: "12px 16px",
      }}>
        <div style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-primary)",
          marginBottom: 4,
        }}>
          {t("usageTracking.title")}
        </div>
        <div style={{
          fontSize: 10,
          color: "var(--text-secondary)",
          marginBottom: 10,
          lineHeight: 1.4,
        }}>
          {t("usageTracking.description")}
        </div>
        <button
          onClick={enableClaudeTracking}
          disabled={enabling}
          style={{
            width: "100%",
            padding: "6px 0",
            fontSize: 11,
            fontWeight: 600,
            color: "var(--text-primary)",
            background: "var(--bg-hover)",
            border: "1px solid var(--border-secondary)",
            borderRadius: "var(--radius-md)",
            cursor: enabling ? "default" : "pointer",
            opacity: enabling ? 0.6 : 1,
            transition: "opacity 0.2s ease",
          }}
        >
          {enabling ? t("usageTracking.enabling") : t("usageTracking.enable")}
        </button>
      </div>
    );
  }

  if (!showClaude && showCodex && !hasCodexData) return null;

  const { five_hour, seven_day, extra_usage, is_stale } = usage ?? {};

  const hasClaudeData = showClaude && (!!five_hour || !!seven_day || !!extra_usage);
  const showClaudePrompt = showClaude && !prefs.usage_tracking_enabled;
  // Tracking is enabled (often auto-migrated for existing users) but no usage
  // payload is cached yet — credentials missing, first poll pending, or the
  // OAuth fetch failed. Surface an explicit "unavailable" state instead of
  // silently dropping the whole card, which otherwise looks like Claude usage
  // vanished when the user merely toggled Codex off.
  const showClaudeUnavailable =
    showClaude && prefs.usage_tracking_enabled && !hasClaudeData;
  if (!hasClaudeData && !showClaudePrompt && !showClaudeUnavailable && !hasCodexData) return null;

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: "12px 16px",
    }}>
      {/* Header */}
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 8,
      }}>
        <span style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-primary)",
        }}>
          {t("usageAlert.title")}
        </span>
      </div>

      {hasClaudeData && (
        <div>
          <ProviderHeader
            label={t("usageAlert.claude")}
            stale={is_stale}
            refreshButton={(
              <RefreshButton
                refreshing={refreshing}
                cooldown={cooldown}
                onRefresh={handleRefresh}
              />
            )}
          />
          {five_hour && (
            <UsageRow
              label={t("usageAlert.session")}
              utilization={five_hour.utilization}
              subtitle={formatResetTime(five_hour.resets_at, t)}
            />
          )}
          {seven_day && (
            <UsageRow
              label={t("usageAlert.weekly")}
              utilization={seven_day.utilization}
              subtitle={formatResetTime(seven_day.resets_at, t)}
            />
          )}
          {extra_usage && extra_usage.is_enabled && (
            <UsageRow
              label={t("usageAlert.extraUsage")}
              utilization={extra_usage.utilization}
              subtitle={`$${extra_usage.used_credits.toFixed(2)} / $${extra_usage.monthly_limit.toFixed(2)}`}
            />
          )}
        </div>
      )}

      {showClaudePrompt && (
        <ClaudeTrackingPrompt
          enabling={enabling}
          onEnable={enableClaudeTracking}
        />
      )}

      {showClaudeUnavailable && (
        <div>
          <ProviderHeader
            label={t("usageAlert.claude")}
            refreshButton={(
              <RefreshButton
                refreshing={refreshing}
                cooldown={cooldown}
                onRefresh={handleRefresh}
              />
            )}
          />
          <div style={{
            fontSize: 10,
            color: "var(--text-secondary)",
            lineHeight: 1.4,
          }}>
            {t("usageAlert.claudeUnavailable")}
          </div>
        </div>
      )}

      {(hasClaudeData || showClaudePrompt || showClaudeUnavailable) && hasCodexData && (
        <div style={{
          height: 1,
          background: "rgba(255,255,255,0.08)",
          margin: "12px 0",
        }} />
      )}

      {hasCodexData && (
        <div>
          <ProviderHeader label={t("usageAlert.codex")} />
          {hasCodexRateLimits ? (
            <CodexRateLimitRows
              primary={codexRateLimits?.primary}
              secondary={codexRateLimits?.secondary}
            />
          ) : (
            <>
              <CodexUsageRow
                label={t("usageAlert.today")}
                summary={codexToday}
                maxTokens={codexMaxTokens}
              />
              <CodexUsageRow
                label={t("usageAlert.last7Days")}
                summary={codexWeek}
                maxTokens={codexMaxTokens}
              />
            </>
          )}
        </div>
      )}
    </div>
  );
}
