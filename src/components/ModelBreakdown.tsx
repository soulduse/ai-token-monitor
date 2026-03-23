import type { ModelUsage } from "../lib/types";
import { formatTokens, formatCost } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  modelUsage: Record<string, ModelUsage>;
}

function shortModelName(name: string): string {
  // Extract family and version: "claude-opus-4-6" → "Opus 4.6"
  const match = name.match(/(opus|sonnet|haiku)-(\d+)-(\d+)/);
  if (match) {
    const family = match[1].charAt(0).toUpperCase() + match[1].slice(1);
    return `${family} ${match[2]}.${match[3]}`;
  }
  if (name.includes("opus")) return "Opus";
  if (name.includes("sonnet")) return "Sonnet";
  if (name.includes("haiku")) return "Haiku";
  return name.split("-").slice(0, 2).join(" ");
}

export function ModelBreakdown({ modelUsage }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();
  const models = Object.entries(modelUsage).sort(
    ([, a], [, b]) =>
      b.input_tokens + b.output_tokens - (a.input_tokens + a.output_tokens)
  );

  if (models.length === 0) return null;

  const maxTotal = Math.max(
    ...models.map(([, m]) => m.input_tokens + m.output_tokens + m.cache_read)
  );

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
        marginBottom: 10,
      }}>
        {t("model.title")}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        {models.map(([model, usage]) => {
          const total = usage.input_tokens + usage.output_tokens + usage.cache_read;
          const inputPct = (usage.input_tokens / total) * 100;
          const outputPct = (usage.output_tokens / total) * 100;
          const cachePct = (usage.cache_read / total) * 100;
          const barWidth = (total / maxTotal) * 100;

          return (
            <div key={model}>
              <div style={{
                display: "flex",
                justifyContent: "space-between",
                marginBottom: 4,
              }}>
                <span style={{ fontSize: 12, fontWeight: 700, color: "var(--text-primary)" }}>
                  {shortModelName(model)}
                  <span style={{ fontSize: 9, fontWeight: 500, color: "var(--text-secondary)", marginLeft: 4 }}>
                    {formatCost(usage.cost_usd)}
                  </span>
                </span>
                <span style={{ fontSize: 11, fontWeight: 600, color: "var(--text-secondary)" }}>
                  {formatTokens(total, prefs.number_format)}
                </span>
              </div>
              <div style={{
                height: 8,
                borderRadius: 4,
                background: "var(--heat-0)",
                overflow: "hidden",
                width: `${barWidth}%`,
              }}>
                <div style={{
                  display: "flex",
                  height: "100%",
                }}>
                  <div style={{
                    width: `${inputPct}%`,
                    background: "var(--accent-purple)",
                    borderRadius: "4px 0 0 4px",
                  }} />
                  <div style={{
                    width: `${outputPct}%`,
                    background: "var(--accent-pink)",
                  }} />
                  <div style={{
                    width: `${cachePct}%`,
                    background: "var(--accent-mint)",
                    borderRadius: "0 4px 4px 0",
                  }} />
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* Legend */}
      <div style={{
        display: "flex",
        gap: 12,
        marginTop: 10,
      }}>
        <LegendItem color="var(--accent-purple)" label={t("model.input")} />
        <LegendItem color="var(--accent-pink)" label={t("model.output")} />
        <LegendItem color="var(--accent-mint)" label={t("model.cache")} />
      </div>
    </div>
  );
}

function LegendItem({ color, label }: { color: string; label: string }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
      <div style={{ width: 8, height: 8, borderRadius: 2, background: color }} />
      <span style={{ fontSize: 9, color: "var(--text-secondary)", fontWeight: 600 }}>{label}</span>
    </div>
  );
}
