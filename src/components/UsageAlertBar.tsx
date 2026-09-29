import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useOAuthUsage } from "../hooks/useOAuthUsage";
import { useGrokUsage } from "../hooks/useGrokUsage";
import { useTeamAIUsage } from "../hooks/useTeamAIUsage";
import { useTokenStats } from "../hooks/useTokenStats";
import { useToday } from "../hooks/useToday";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";
import type {
  AllStats,
  RateLimitWindow,
  TeamAIAccountStatus,
  TeamAIClaudeAccount,
  TeamAICodexAccount,
  TeamAIUsage,
  TeamAIWindow,
} from "../lib/types";
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

// ── TeamAI multi-account table ───────────────────────────────────────────────
// Mirrors the TeamAI terminal dashboard: one row per pooled account, each quota
// window drawn as a filled bar with "65% 1d12h" printed across it.

type Translate = (key: string, params?: Record<string, string>) => string;

const TEAMAI_GAUGE_WIDTH = 72;
const TEAMAI_GAP = 4;
const TEAMAI_DDAY_WIDTH = 30;
// The three Claude gauge columns; Codex rows split the same span evenly so
// both sections line up.
const TEAMAI_GAUGE_SPAN = TEAMAI_GAUGE_WIDTH * 3 + TEAMAI_GAP * 2;

function teamaiGrid(gaugeCount: number): string {
  const count = Math.max(gaugeCount, 1);
  const width = (TEAMAI_GAUGE_SPAN - TEAMAI_GAP * (count - 1)) / count;
  return `minmax(0, 1fr) repeat(${count}, ${width}px) ${TEAMAI_DDAY_WIDTH}px`;
}

// "35m" / "3h25m" / "6d8h" — TeamAI's compact time-left format.
function formatCompactRemaining(resetsAt: number | null): string {
  if (!resetsAt) return "";
  const mins = Math.ceil((resetsAt - Date.now()) / 60_000);
  if (mins <= 0) return "";
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h${mins % 60 ? `${mins % 60}m` : ""}`;
  return `${Math.floor(hours / 24)}d${hours % 24 ? `${hours % 24}h` : ""}`;
}

function formatAge(at: number): string {
  const mins = Math.max(0, Math.floor((Date.now() - at) / 60_000));
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  return hours < 24 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
}

// "1w" / "5h" — a Codex window named by its own length.
function formatSpan(minutes: number | null): string {
  if (!minutes) return "";
  if (minutes % 10_080 === 0) return `${minutes / 10_080}w`;
  if (minutes % 1_440 === 0) return `${minutes / 1_440}d`;
  if (minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

// Every account is on the same mail domain more often than not, so show the
// local part — unless two accounts would then read the same.
function makeShortLabel(labels: string[]): (label: string) => string {
  const local = (label: string) => label.split("@")[0] || label;
  const counts = new Map<string, number>();
  labels.forEach((label) => counts.set(local(label), (counts.get(local(label)) ?? 0) + 1));
  return (label) => ((counts.get(local(label)) ?? 0) > 1 ? label : local(label));
}

const TEAMAI_STATUS_COLOR: Record<TeamAIAccountStatus, string | null> = {
  active: null,
  cooldown: "#eab308",
  error: "#ef4444",
  disabled: "var(--text-muted)",
  inactive: "#ef4444",
};

const TEAMAI_STATUS_KEY: Record<TeamAIAccountStatus, string> = {
  active: "usageAlert.teamaiStatusActive",
  cooldown: "usageAlert.teamaiStatusCooldown",
  error: "usageAlert.teamaiStatusError",
  disabled: "usageAlert.teamaiStatusDisabled",
  inactive: "usageAlert.teamaiStatusInactive",
};

function formatDday(days: number): string {
  return days <= 0 ? "D-DAY" : `D-${days}`;
}

function ddayColor(days: number): string {
  if (days <= 3) return "#ef4444";
  if (days <= 7) return "#eab308";
  return "#22c55e";
}

function TeamAIGauge({ window, t }: { window: TeamAIWindow | null; t: Translate }) {
  const pct = window ? Math.min(Math.max(window.utilization, 0), 100) : 0;
  const remaining = window ? formatCompactRemaining(window.resets_at) : "";
  const percent = window ? `${Math.round(pct)}%` : "–";
  const color = getBarColor(pct);

  return (
    <div
      title={window?.resets_at ? formatResetTime(new Date(window.resets_at).toISOString(), t) : undefined}
      style={{
        position: "relative",
        height: 20,
        borderRadius: 4,
        overflow: "hidden",
        background: "rgba(0,0,0,0.3)",
        border: "1px solid rgba(255,255,255,0.08)",
        boxSizing: "border-box",
      }}
    >
      {window && (
        <div style={{
          position: "absolute",
          top: 0,
          bottom: 0,
          left: 0,
          width: `${pct}%`,
          background: color,
          transition: "width 0.3s ease",
        }} />
      )}
      <span style={{
        position: "relative",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 3,
        height: "100%",
        fontSize: 11,
        fontWeight: 700,
        color: window ? "#fff" : "var(--text-muted)",
        textShadow: window ? "0 0 3px rgba(0,0,0,0.7)" : "none",
        fontVariantNumeric: "tabular-nums",
        whiteSpace: "nowrap",
      }}>
        {percent}
        {remaining && (
          <span style={{ fontSize: 9.5, fontWeight: 500, opacity: 0.85 }}>{remaining}</span>
        )}
      </span>
    </div>
  );
}

function TeamAIColumnHeader({
  title,
  columns,
  grid,
}: {
  title: string;
  columns: string[];
  grid: string;
}) {
  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: grid,
      gap: TEAMAI_GAP,
      alignItems: "center",
      marginBottom: 6,
      fontSize: 10,
      fontWeight: 600,
      color: "var(--text-muted)",
    }}>
      <span style={{ color: "var(--text-secondary)" }}>{title}</span>
      {columns.map((column, i) => (
        <span key={i} style={{ textAlign: "center" }}>{column}</span>
      ))}
      <span />
    </div>
  );
}

function TeamAIAccountRow({
  label,
  tooltip,
  status,
  grid,
  children,
  trailing,
}: {
  label: string;
  tooltip: string;
  status: TeamAIAccountStatus;
  grid: string;
  children: ReactNode;
  trailing?: ReactNode;
}) {
  const statusColor = TEAMAI_STATUS_COLOR[status];

  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: grid,
      gap: TEAMAI_GAP,
      alignItems: "center",
      marginBottom: 5,
      opacity: status === "disabled" ? 0.45 : 1,
    }}>
      <span
        title={tooltip}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          minWidth: 0,
          fontSize: 11.5,
          fontWeight: 600,
          color: "var(--text-primary)",
        }}
      >
        {statusColor && (
          <span style={{
            flexShrink: 0,
            width: 6,
            height: 6,
            borderRadius: "50%",
            background: statusColor,
          }} />
        )}
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {label}
        </span>
      </span>
      {children}
      <span style={{
        fontSize: 10,
        fontWeight: 700,
        textAlign: "right",
        fontVariantNumeric: "tabular-nums",
      }}>
        {trailing}
      </span>
    </div>
  );
}

function teamaiTooltip(
  account: { label: string; plan: string | null; status: TeamAIAccountStatus },
  t: Translate,
  renewalDays?: number | null,
): string {
  const lines = [account.label, [account.plan, t(TEAMAI_STATUS_KEY[account.status])].filter(Boolean).join(" · ")];
  if (renewalDays != null) lines.push(t("usageAlert.teamaiRenewal", { dday: formatDday(renewalDays) }));
  return lines.join("\n");
}

function TeamAIClaudeRows({
  accounts,
  modelLabel,
  t,
}: {
  accounts: TeamAIClaudeAccount[];
  modelLabel: string | null;
  t: Translate;
}) {
  const grid = teamaiGrid(3);
  const shortLabel = makeShortLabel(accounts.map((a) => a.label));

  return (
    <div>
      <TeamAIColumnHeader
        title={`${t("usageAlert.claude")} (${accounts.length})`}
        columns={["5h", "7d", modelLabel ?? "7d model"]}
        grid={grid}
      />
      {accounts.map((account) => (
        <TeamAIAccountRow
          key={account.id}
          label={shortLabel(account.label)}
          tooltip={teamaiTooltip(account, t, account.renewal_days)}
          status={account.status}
          grid={grid}
          trailing={account.renewal_days != null && (
            <span style={{ color: ddayColor(account.renewal_days) }}>
              {formatDday(account.renewal_days)}
            </span>
          )}
        >
          <TeamAIGauge window={account.five_hour} t={t} />
          <TeamAIGauge window={account.seven_day} t={t} />
          <TeamAIGauge window={account.seven_day_model} t={t} />
        </TeamAIAccountRow>
      ))}
    </div>
  );
}

function TeamAICodexRows({
  accounts,
  t,
}: {
  accounts: TeamAICodexAccount[];
  t: Translate;
}) {
  // Titles follow the windows actually in play, so the header never names a
  // gauge no row draws; a span shared by every account is printed once.
  const gaugeCount = Math.max(1, ...accounts.map((a) => a.windows.length));
  const grid = teamaiGrid(gaugeCount);
  const shortLabel = makeShortLabel(accounts.map((a) => a.label));
  const columns = Array.from({ length: gaugeCount }, (_, i) => {
    const spans = new Set(accounts.map((a) => formatSpan(a.windows[i]?.minutes ?? null)).filter(Boolean));
    return spans.size === 1 ? [...spans][0] : "";
  });

  return (
    <div>
      <TeamAIColumnHeader
        title={`${t("usageAlert.codex")} (${accounts.length})`}
        columns={columns}
        grid={grid}
      />
      {accounts.map((account) => (
        <TeamAIAccountRow
          key={account.id}
          label={shortLabel(account.label)}
          tooltip={teamaiTooltip(account, t)}
          status={account.status}
          grid={grid}
        >
          {Array.from({ length: gaugeCount }, (_, i) => (
            <TeamAIGauge key={i} window={account.windows[i] ?? null} t={t} />
          ))}
        </TeamAIAccountRow>
      ))}
    </div>
  );
}

function TeamAIUsageSection({
  usage,
  showClaude,
  showCodex,
  onRefresh,
}: {
  usage: TeamAIUsage;
  showClaude: boolean;
  showCodex: boolean;
  onRefresh: () => void;
}) {
  const t = useI18n();
  // A stopped TeamAI no longer measures anything: say how old the numbers are
  // instead of presenting them as live.
  const status = usage.running ? (
    <span style={{ fontSize: 10, fontWeight: 600, color: "#22c55e" }}>
      ● {t("usageAlert.teamaiRunning")}
    </span>
  ) : (
    <span style={{ fontSize: 10, fontWeight: 600, color: "var(--text-muted)" }}>
      {usage.updated_at
        ? t("usageAlert.teamaiStoppedAgo", { time: formatAge(usage.updated_at) })
        : t("usageAlert.teamaiStopped")}
    </span>
  );

  return (
    <div>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 8,
      }}>
        <span style={{ fontSize: 11, fontWeight: 700, color: "var(--text-primary)" }}>
          TeamAI
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {status}
          <RefreshButton refreshing={false} cooldown={0} onRefresh={onRefresh} />
        </div>
      </div>
      {showClaude && (
        <TeamAIClaudeRows accounts={usage.claude} modelLabel={usage.model_label} t={t} />
      )}
      {showClaude && showCodex && <div style={{ height: 10 }} />}
      {showCodex && <TeamAICodexRows accounts={usage.codex} t={t} />}
    </div>
  );
}

function SectionDivider() {
  return (
    <div style={{
      height: 1,
      background: "rgba(255,255,255,0.08)",
      margin: "12px 0",
    }} />
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
  const { usage, status: oauthStatus, refreshing, rateLimitRemaining, refresh } = useOAuthUsage();
  const { stats: codexStats } = useTokenStats("codex");
  const showGrok = prefs.include_grok;
  const { credits: grokCredits } = useGrokUsage(showGrok);
  const { usage: teamai, refresh: refreshTeamAI } = useTeamAIUsage(prefs.include_teamai);
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

  const hasGrokCredits = showGrok && (grokCredits?.credit_usage_percent != null || !!grokCredits?.subscription_tier);

  if (!showClaude && !showCodex && !showGrok) return null;

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
  // With TeamAI in front of Claude Code / Codex, the single-account gauges only
  // describe whichever account the local credentials or the last JSONL entry
  // happen to belong to. When TeamAI data is present its per-account table
  // replaces that provider's block — still gated on the source toggle.
  const teamaiClaude = showClaude && (teamai?.claude.length ?? 0) > 0;
  const teamaiCodex = showCodex && (teamai?.codex.length ?? 0) > 0;
  const showCodexBlock = hasCodexData && !teamaiCodex;

  // Claude-only, tracking never enabled: show the standalone enable card. When
  // Codex is also on, the same enable affordance is rendered inline further
  // down via showClaudePrompt → ClaudeTrackingPrompt, so this branch is
  // deliberately gated on !showCodex to avoid a duplicate prompt.
  if (showClaude && !prefs.usage_tracking_enabled && !showCodex && !showGrok && !teamaiClaude) {
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

  if (!showClaude && !hasCodexData && !hasGrokCredits && !teamaiCodex) return null;

  const { five_hour, seven_day, seven_day_models, extra_usage, is_stale } = usage ?? {};

  // Per-model weekly windows (e.g. Fable). The backend already filters these to
  // the active, model-scoped limits, so we render whatever it sends. This makes
  // newly introduced or removed model limits appear/disappear on their own —
  // Fable, for instance, may be temporary and will simply stop rendering.
  const modelWindows = seven_day_models ?? [];

  const hasClaudeData =
    showClaude && !teamaiClaude && (!!five_hour || !!seven_day || modelWindows.length > 0 || !!extra_usage);
  const showClaudePrompt = showClaude && !teamaiClaude && !prefs.usage_tracking_enabled;
  // Only surface the "unavailable" message when the backend reports that OAuth
  // credentials exist but no usage is cached yet (first poll pending or a failed
  // fetch). The "no_credentials" status — the normal state for Codex-only users
  // who never signed into Claude Code — stays hidden so we don't show a
  // permanent false error. Until the status resolves, render nothing.
  const showClaudeUnavailable =
    showClaude &&
    !teamaiClaude &&
    prefs.usage_tracking_enabled &&
    !hasClaudeData &&
    oauthStatus === "unavailable";
  const showTeamAI = teamaiClaude || teamaiCodex;
  if (!hasClaudeData && !showClaudePrompt && !showClaudeUnavailable && !showTeamAI && !showCodexBlock && !hasGrokCredits) return null;

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
            rateLimitRemaining={rateLimitRemaining}
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
          {modelWindows.map((m) => (
            <UsageRow
              key={m.model}
              label={t("usageAlert.weeklyModel", { model: m.model })}
              utilization={m.utilization}
              subtitle={formatResetTime(m.resets_at, t)}
            />
          ))}
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

      {(hasClaudeData || showClaudePrompt || showClaudeUnavailable) && showTeamAI && <SectionDivider />}

      {showTeamAI && teamai && (
        <TeamAIUsageSection
          usage={teamai}
          showClaude={teamaiClaude}
          showCodex={teamaiCodex}
          onRefresh={refreshTeamAI}
        />
      )}

      {(hasClaudeData || showClaudePrompt || showClaudeUnavailable || showTeamAI) && showCodexBlock && <SectionDivider />}

      {showCodexBlock && (
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

      {(hasClaudeData || showClaudePrompt || showClaudeUnavailable || showTeamAI || showCodexBlock) && hasGrokCredits && <SectionDivider />}

      {hasGrokCredits && grokCredits && (
        <div>
          <ProviderHeader label={t("usageAlert.grok")} />
          {grokCredits.credit_usage_percent != null && (
            <UsageRow
              label={grokCredits.subscription_tier || t("usageAlert.weekly")}
              utilization={grokCredits.credit_usage_percent}
              subtitle={formatResetTime(grokCredits.period_end, t)}
            />
          )}
          {grokCredits.on_demand_cap > 0 && (
            <UsageRow
              label={t("usageAlert.grokOnDemand")}
              utilization={Math.min((grokCredits.on_demand_used / grokCredits.on_demand_cap) * 100, 100)}
              subtitle={`${grokCredits.on_demand_used.toFixed(1)} / ${grokCredits.on_demand_cap.toFixed(1)}`}
            />
          )}
        </div>
      )}
    </div>
  );
}
