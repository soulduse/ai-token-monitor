import type { AllStats } from "../lib/types";
import { formatTokens, formatCost } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  stats: AllStats;
}

export function AnalyticsSummary({ stats }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();

  const totalCost = stats.daily.reduce((s, d) => s + d.cost_usd, 0);
  const totalInput = stats.daily.reduce((s, d) => s + d.input_tokens, 0);
  const totalOutput = stats.daily.reduce((s, d) => s + d.output_tokens, 0);
  const totalCacheRead = stats.daily.reduce((s, d) => s + d.cache_read_tokens, 0);
  const totalCacheWrite = stats.daily.reduce((s, d) => s + d.cache_write_tokens, 0);
  const cacheHitRate = totalInput + totalCacheRead > 0
    ? Math.round((totalCacheRead / (totalInput + totalCacheRead)) * 100)
    : 0;

  const fmt = prefs.number_format;

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: "12px 16px",
      boxShadow: "var(--shadow-card)",
    }}>
      {/* Top row: cost + key metrics */}
      <div style={{
        display: "flex",
        alignItems: "baseline",
        gap: 12,
        flexWrap: "wrap",
      }}>
        <span style={{
          fontSize: 20,
          fontWeight: 800,
          color: "var(--accent-orange, var(--accent-purple))",
        }}>
          {formatCost(totalCost)}
        </span>
        <Metric label={t("analytics.summary.messages")} value={stats.total_messages.toLocaleString()} />
        <Metric label={t("analytics.summary.sessions")} value={stats.total_sessions.toLocaleString()} />
        <Metric label={t("analytics.summary.cacheHit")} value={`${cacheHitRate}%`} />
      </div>

      {/* Bottom row: token breakdown */}
      <div style={{
        display: "flex",
        gap: 12,
        marginTop: 6,
        flexWrap: "wrap",
      }}>
        <TokenStat label={t("analytics.summary.in")} value={formatTokens(totalInput, fmt)} color="var(--accent-purple)" />
        <TokenStat label={t("analytics.summary.out")} value={formatTokens(totalOutput, fmt)} color="var(--accent-pink)" />
        <TokenStat label={t("analytics.summary.cached")} value={formatTokens(totalCacheRead, fmt)} color="var(--accent-mint)" />
        <TokenStat label={t("analytics.summary.written")} value={formatTokens(totalCacheWrite, fmt)} color="var(--text-secondary)" />
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <span style={{ fontSize: 13, color: "var(--text-secondary)" }}>
      <span style={{ fontWeight: 700, color: "var(--text-primary)", marginRight: 3 }}>{value}</span>
      {label}
    </span>
  );
}

function TokenStat({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <span style={{ fontSize: 11, color: "var(--text-secondary)" }}>
      <span style={{ fontWeight: 700, color, marginRight: 2 }}>{value}</span>
      {label}
    </span>
  );
}
