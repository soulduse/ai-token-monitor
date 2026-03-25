import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DailyUsage } from "../lib/types";
import { formatTokens, formatCost, getTotalTokens } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { InfoTooltip } from "./InfoTooltip";
import { useI18n } from "../i18n/I18nContext";

interface PricingRow {
  model: string;
  input: string;
  output: string;
  cache_read: string;
  cache_write: string;
}

interface PricingTable {
  version: string;
  last_updated: string;
  claude: PricingRow[];
  codex: PricingRow[];
}

interface Props {
  today: DailyUsage | null;
  weekAvg: number;
}

export function TodaySummary({ today, weekAvg }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();
  const [pricing, setPricing] = useState<PricingTable | null>(null);
  const totalTokens = today ? getTotalTokens(today.tokens) : 0;
  const cost = today?.cost_usd ?? 0;
  const messages = today?.messages ?? 0;
  const sessions = today?.sessions ?? 0;

  useEffect(() => {
    invoke<PricingTable>("get_pricing_table").then(setPricing).catch(console.error);
  }, []);

  const comparison = weekAvg > 0
    ? Math.round(((totalTokens - weekAvg) / weekAvg) * 100)
    : 0;

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
        marginBottom: 8,
      }}>
        {t("today.title")}
      </div>

      <div style={{ display: "flex", alignItems: "baseline", gap: 8, marginBottom: 4 }}>
        <span style={{
          fontSize: 28,
          fontWeight: 800,
          color: "var(--accent-purple)",
          letterSpacing: "-1px",
        }}>
          {formatTokens(totalTokens, prefs.number_format)}
        </span>
        <span style={{ fontSize: 13, color: "var(--text-secondary)", fontWeight: 600 }}>
          {t("today.tokens")}
        </span>
        {comparison !== 0 && (
          <span style={{
            fontSize: 11,
            fontWeight: 700,
            color: comparison > 0 ? "var(--accent-pink)" : "var(--accent-mint)",
            background: comparison > 0
              ? "rgba(255,143,164,0.1)"
              : "rgba(93,217,168,0.1)",
            padding: "2px 6px",
            borderRadius: 6,
          }}>
            {comparison > 0 ? "+" : ""}{comparison}% {t("today.vs7d")}
          </span>
        )}
      </div>

      <div style={{
        display: "flex",
        gap: 16,
        marginTop: 8,
      }}>
        <StatChip
          label={t("today.cost")}
          value={formatCost(cost)}
          color="var(--accent-orange)"
          tooltip={pricing && (
            <InfoTooltip wide>
              <div style={{ fontWeight: 700, marginBottom: 6 }}>{t("today.costTooltipTitle")}</div>
              {(prefs.include_claude ? [{ label: "Claude", rows: pricing.claude }] : [])
                .concat(prefs.include_codex ? [{ label: "Codex", rows: pricing.codex }] : [])
                .map(({ label, rows }) => (
                <div key={label}>
                  {prefs.include_claude && prefs.include_codex && (
                    <div style={{ fontWeight: 600, fontSize: 9, marginTop: 4, marginBottom: 2, opacity: 0.7 }}>{label}</div>
                  )}
                  <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 9 }}>
                    <thead>
                      <tr style={{ opacity: 0.7 }}>
                        <th style={{ textAlign: "left", paddingBottom: 2 }}>{t("today.costTooltipModel")}</th>
                        <th style={{ textAlign: "right", paddingBottom: 2 }}>{t("today.costTooltipIn")}</th>
                        <th style={{ textAlign: "right", paddingBottom: 2 }}>{t("today.costTooltipOut")}</th>
                        <th style={{ textAlign: "right", paddingBottom: 2 }}>{t("today.costTooltipCacheR")}</th>
                        <th style={{ textAlign: "right", paddingBottom: 2 }}>{t("today.costTooltipCacheW")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((r) => (
                        <tr key={r.model}>
                          <td>{r.model}</td>
                          <td style={{ textAlign: "right" }}>{r.input}</td>
                          <td style={{ textAlign: "right" }}>{r.output}</td>
                          <td style={{ textAlign: "right" }}>{r.cache_read}</td>
                          <td style={{ textAlign: "right" }}>{r.cache_write}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ))}
              <div style={{ marginTop: 6, opacity: 0.7, fontSize: 9 }}>
                {t("today.costTooltipNote")} (v{pricing.version}, {pricing.last_updated})
              </div>
            </InfoTooltip>
          )}
        />
        <StatChip label={t("today.messages")} value={String(messages)} color="var(--accent-purple)" />
        <StatChip label={t("today.sessions")} value={String(sessions)} color="var(--accent-mint)" />
      </div>
    </div>
  );
}

function StatChip({ label, value, color, tooltip }: { label: string; value: string; color: string; tooltip?: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
      <div style={{ display: "flex", alignItems: "center" }}>
        <span style={{ fontSize: 10, color: "var(--text-secondary)", fontWeight: 600 }}>
          {label}
        </span>
        {tooltip}
      </div>
      <span style={{ fontSize: 15, fontWeight: 700, color }}>
        {value}
      </span>
    </div>
  );
}
