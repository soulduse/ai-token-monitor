import type { AllStats } from "../lib/types";
import { InfoTooltip } from "./InfoTooltip";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  stats: AllStats;
}

export function CacheEfficiency({ stats }: Props) {
  const t = useI18n();
  let totalInput = 0;
  let totalCacheRead = 0;
  let totalCacheWrite = 0;

  for (const usage of Object.values(stats.model_usage)) {
    totalInput += usage.input_tokens;
    totalCacheRead += usage.cache_read;
    totalCacheWrite += usage.cache_write;
  }

  const hitRate = totalInput + totalCacheRead > 0
    ? (totalCacheRead / (totalInput + totalCacheRead)) * 100
    : 0;

  const R = 36;
  const STROKE = 8;
  const C = 2 * Math.PI * R;
  const filled = (hitRate / 100) * C;

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 16,
      boxShadow: "var(--shadow-card)",
    }}>
      <div style={{
        fontSize: 11,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
        marginBottom: 12,
      }}>
        {t("cache.title")}
        <InfoTooltip>
          <div style={{ fontWeight: 700, marginBottom: 4 }}>{t("cache.tooltipTitle")}</div>
          <div>{t("cache.tooltipDesc")}</div>
          <div style={{ marginTop: 6 }}>
            <strong>{t("cache.tooltipGood")}</strong>
          </div>
          <div style={{ marginTop: 4, opacity: 0.7 }}>
            {t("cache.tooltipFormula")}
          </div>
        </InfoTooltip>
      </div>

      <div style={{
        display: "flex",
        alignItems: "center",
        gap: 20,
      }}>
        {/* Donut */}
        <div style={{ position: "relative", width: 88, height: 88, flexShrink: 0 }}>
          <svg viewBox="0 0 88 88" width="88" height="88">
            <circle cx="44" cy="44" r={R} fill="none" stroke="var(--heat-0)" strokeWidth={STROKE} />
            <circle
              cx="44" cy="44" r={R} fill="none"
              stroke="var(--accent-mint)" strokeWidth={STROKE}
              strokeDasharray={`${filled} ${C - filled}`}
              strokeDashoffset={C / 4} strokeLinecap="round"
              style={{ transition: "stroke-dasharray 0.4s ease" }}
            />
          </svg>
          <div style={{
            position: "absolute", inset: 0,
            display: "flex", alignItems: "center", justifyContent: "center", flexDirection: "column",
          }}>
            <span style={{ fontSize: 18, fontWeight: 800, color: "var(--accent-mint)", lineHeight: 1 }}>
              {hitRate.toFixed(1)}%
            </span>
            <span style={{ fontSize: 9, color: "var(--text-secondary)", fontWeight: 600 }}>
              {t("cache.cached")}
            </span>
          </div>
        </div>

        {/* Stats */}
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <CacheStat label={t("cache.read")} value={totalCacheRead} color="var(--accent-mint)" />
          <CacheStat label={t("cache.write")} value={totalCacheWrite} color="var(--accent-orange)" />
          <CacheStat label={t("cache.input")} value={totalInput} color="var(--accent-purple)" />
        </div>
      </div>
    </div>
  );
}

function CacheStat({ label, value, color }: { label: string; value: number; color: string }) {
  const formatted = value >= 1_000_000
    ? `${(value / 1_000_000).toFixed(1)}M`
    : value >= 1_000
      ? `${(value / 1_000).toFixed(1)}K`
      : String(value);

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <div style={{ width: 6, height: 6, borderRadius: 2, background: color, flexShrink: 0 }} />
      <span style={{ fontSize: 10, color: "var(--text-secondary)", fontWeight: 600, width: 72 }}>
        {label}
      </span>
      <span style={{ fontSize: 11, fontWeight: 700, color: "var(--text-primary)" }}>
        {formatted}
      </span>
    </div>
  );
}
