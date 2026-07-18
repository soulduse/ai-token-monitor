import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useOAuthUsage } from "../hooks/useOAuthUsage";
import { useTokenStats } from "../hooks/useTokenStats";
import { useToday } from "../hooks/useToday";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";
import type { AccountSnapshot, AllStats, OAuthUsage, RateLimitWindow } from "../lib/types";
import { formatCost, formatTokens, getTotalTokens } from "../lib/format";

const REFRESH_COOLDOWN_SECONDS = 30;

function getBarColor(percent: number): string {
  if (percent >= 90) return "#ef4444";
  if (percent >= 80) return "#f97316";
  if (percent >= 50) return "#eab308";
  return "#22c55e";
}

function formatResetTime(resetsAt: string | null | undefined, t: (key: string, params?: Record<string, string>) => string): string {
  // The API omits resets_at (null) for windows with no scheduled reset. Bail
  // before the diff math so we render a clean blank instead of "NaNd NaNh".
  if (!resetsAt) return "";
  const reset = new Date(resetsAt);
  if (Number.isNaN(reset.getTime())) return "";
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

// "5m" / "3h" / "2d" age of an inactive account's snapshot, so the split view
// is honest about how old each non-active account's numbers are.
function formatSnapshotAge(updatedAt: string, t: (key: string, params?: Record<string, string>) => string): string {
  const then = new Date(updatedAt).getTime();
  if (Number.isNaN(then)) return "";
  const diffMin = Math.floor((Date.now() - then) / 60000);
  if (diffMin < 1) return t("usageAlert.justNow");
  const time =
    diffMin < 60
      ? `${diffMin}m`
      : diffMin < 1440
      ? `${Math.floor(diffMin / 60)}h`
      : `${Math.floor(diffMin / 1440)}d`;
  return t("usageAlert.lastChecked", { time });
}

// Display label for an account. Email wins over display name — two accounts
// owned by the same person often share a display name but never an email.
function accountLabel(snapshot: AccountSnapshot): string {
  return snapshot.email || snapshot.display_name || `${snapshot.account_uuid.slice(0, 8)}…`;
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

// The Claude limit gauges (session / weekly / per-model / extra usage) for one
// usage payload. Shared between the default single-account card and each
// section of the split-account view.
function ClaudeUsageWindows({ usage }: { usage: OAuthUsage }) {
  const t = useI18n();
  // Per-model weekly windows (e.g. Fable). The backend already filters these to
  // the active, model-scoped limits, so we render whatever it sends. This makes
  // newly introduced or removed model limits appear/disappear on their own —
  // Fable, for instance, may be temporary and will simply stop rendering.
  const modelWindows = usage.seven_day_models ?? [];

  return (
    <>
      {usage.five_hour && (
        <UsageRow
          label={t("usageAlert.session")}
          utilization={usage.five_hour.utilization}
          subtitle={formatResetTime(usage.five_hour.resets_at, t)}
        />
      )}
      {usage.seven_day && (
        <UsageRow
          label={t("usageAlert.weekly")}
          utilization={usage.seven_day.utilization}
          subtitle={formatResetTime(usage.seven_day.resets_at, t)}
        />
      )}
      {modelWindows.map((m) => (
        <UsageRow
          key={m.model}
          label={t("usageAlert.weeklyModel", { model: m.model })}
          utilization={m.utilization}
          subtitle={formatResetTime(m.resets_at, t)}
        />
      ))}
      {usage.extra_usage && usage.extra_usage.is_enabled && (
        <UsageRow
          label={t("usageAlert.extraUsage")}
          utilization={usage.extra_usage.utilization}
          subtitle={`$${usage.extra_usage.used_credits.toFixed(2)} / $${usage.extra_usage.monthly_limit.toFixed(2)}`}
        />
      )}
    </>
  );
}

// Toggles the split-account view. Only rendered once two or more accounts have
// been observed, so single-account users never see it.
function AccountViewToggle({
  active,
  onToggle,
}: {
  active: boolean;
  onToggle: () => void;
}) {
  const t = useI18n();
  const title = active ? t("usageAlert.accountViewOff") : t("usageAlert.accountView");

  return (
    <button
      onClick={onToggle}
      title={title}
      aria-label={title}
      aria-pressed={active}
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
        color: active ? "var(--accent-purple)" : "var(--text-muted)",
        cursor: "pointer",
        opacity: active ? 1 : 0.8,
        transition: "opacity 0.2s ease, color 0.2s ease",
      }}
      onMouseEnter={(e) => {
        if (!active) e.currentTarget.style.color = "var(--text-primary)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.color = active ? "var(--accent-purple)" : "var(--text-muted)";
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
      >
        <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
        <circle cx="9" cy="7" r="4" />
        <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
        <path d="M16 3.13a4 4 0 0 1 0 7.75" />
      </svg>
    </button>
  );
}

// One account's block in the split view. Active account shows a badge; the
// others show how old their last snapshot is plus a remove affordance (an
// account that comes back gets re-recorded on its next fetch anyway).
function AccountUsageSection({
  snapshot,
  isActive,
  onRemove,
}: {
  snapshot: AccountSnapshot;
  isActive: boolean;
  onRemove: (accountUuid: string) => void;
}) {
  const t = useI18n();

  return (
    <div style={{ marginBottom: 4 }}>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 6,
      }}>
        <span
          title={snapshot.account_uuid}
          style={{
            fontSize: 10,
            fontWeight: 600,
            color: "var(--text-secondary)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {accountLabel(snapshot)}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 6, flexShrink: 0 }}>
          {isActive ? (
            <span style={{
              fontSize: 9,
              fontWeight: 700,
              color: "#22c55e",
            }}>
              {t("usageAlert.activeAccount")}
            </span>
          ) : (
            <>
              <span style={{ fontSize: 9, fontWeight: 600, color: "var(--text-muted)" }}>
                {formatSnapshotAge(snapshot.updated_at, t)}
              </span>
              <button
                onClick={() => onRemove(snapshot.account_uuid)}
                title={t("usageAlert.removeAccount")}
                aria-label={t("usageAlert.removeAccount")}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  width: 14,
                  height: 14,
                  padding: 0,
                  background: "transparent",
                  border: "none",
                  borderRadius: 3,
                  color: "var(--text-muted)",
                  cursor: "pointer",
                  opacity: 0.6,
                  transition: "opacity 0.2s ease, color 0.2s ease",
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.color = "#ef4444";
                  e.currentTarget.style.opacity = "1";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.color = "var(--text-muted)";
                  e.currentTarget.style.opacity = "0.6";
                }}
              >
                <svg
                  width="10"
                  height="10"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.5"
                  strokeLinecap="round"
                >
                  <path d="M18 6 6 18" />
                  <path d="m6 6 12 12" />
                </svg>
              </button>
            </>
          )}
        </div>
      </div>
      <ClaudeUsageWindows usage={snapshot.usage} />
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
  rateLimitRemaining,
  refreshButton,
}: {
  label: string;
  stale?: boolean;
  rateLimitRemaining?: number | null;
  refreshButton?: ReactNode;
}) {
  const t = useI18n();
  // When inside a 429 back-off window, refresh genuinely can't hit the API yet.
  // Say so explicitly instead of leaving the bare "stale" badge, which makes the
  // refresh button look broken.
  const throttled = rateLimitRemaining != null && rateLimitRemaining > 0;

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
        {throttled ? (
          <span
            title={t("usageAlert.rateLimitedTooltip")}
            style={{
              fontSize: 9,
              fontWeight: 600,
              color: "var(--text-muted)",
            }}
          >
            {t("usageAlert.rateLimited", { seconds: Math.ceil(rateLimitRemaining!) })}
          </span>
        ) : stale && (
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
  const { prefs, refreshPrefs, updatePrefs } = useSettings();
  const {
    usage,
    status: oauthStatus,
    refreshing,
    rateLimitRemaining,
    refresh,
    accounts,
    removeAccount,
  } = useOAuthUsage();
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

  const { five_hour, seven_day, seven_day_models, extra_usage, is_stale } = usage ?? {};

  const modelWindows = seven_day_models ?? [];

  const hasClaudeData =
    showClaude && (!!five_hour || !!seven_day || modelWindows.length > 0 || !!extra_usage);
  // Split-account view: opt-in, and only meaningful once at least two accounts
  // have been observed. Snapshots are attributed at fetch time, so each block
  // is guaranteed to show numbers that belonged to that account — the default
  // (combined) view below stays exactly as before.
  const claudeAccounts = accounts?.accounts ?? [];
  const canSplitAccounts =
    showClaude && prefs.usage_tracking_enabled && claudeAccounts.length >= 2;
  const splitAccounts = canSplitAccounts && prefs.account_breakdown_enabled;
  const showClaudePrompt = showClaude && !prefs.usage_tracking_enabled;
  // Only surface the "unavailable" message when the backend reports that OAuth
  // credentials exist but no usage is cached yet (first poll pending or a failed
  // fetch). The "no_credentials" status — the normal state for Codex-only users
  // who never signed into Claude Code — stays hidden so we don't show a
  // permanent false error. Until the status resolves, render nothing.
  const showClaudeUnavailable =
    showClaude &&
    prefs.usage_tracking_enabled &&
    !hasClaudeData &&
    !splitAccounts &&
    oauthStatus === "unavailable";
  if (!hasClaudeData && !showClaudePrompt && !showClaudeUnavailable && !hasCodexData && !splitAccounts) {
    return null;
  }

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

      {(hasClaudeData || splitAccounts) && (
        <div>
          <ProviderHeader
            label={t("usageAlert.claude")}
            stale={is_stale}
            rateLimitRemaining={rateLimitRemaining}
            refreshButton={(
              <>
                {canSplitAccounts && (
                  <AccountViewToggle
                    active={splitAccounts}
                    onToggle={() =>
                      updatePrefs({ account_breakdown_enabled: !prefs.account_breakdown_enabled })
                    }
                  />
                )}
                <RefreshButton
                  refreshing={refreshing}
                  cooldown={cooldown}
                  onRefresh={handleRefresh}
                />
              </>
            )}
          />
          {splitAccounts ? (
            claudeAccounts.map((snapshot) => (
              <AccountUsageSection
                key={snapshot.account_uuid}
                snapshot={snapshot}
                isActive={snapshot.account_uuid === accounts?.active_account_uuid}
                onRemove={removeAccount}
              />
            ))
          ) : (
            usage && <ClaudeUsageWindows usage={usage} />
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

      {(hasClaudeData || splitAccounts || showClaudePrompt || showClaudeUnavailable) && hasCodexData && (
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
