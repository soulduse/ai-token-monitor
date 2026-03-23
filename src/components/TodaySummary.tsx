import type { DailyUsage } from "../lib/types";
import { formatTokens, formatCost, getTotalTokens } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { InfoTooltip } from "./InfoTooltip";

interface Props {
  today: DailyUsage | null;
  weekAvg: number;
}

export function TodaySummary({ today, weekAvg }: Props) {
  const { prefs } = useSettings();
  const totalTokens = today ? getTotalTokens(today.tokens) : 0;
  const cost = today?.cost_usd ?? 0;
  const messages = today?.messages ?? 0;
  const sessions = today?.sessions ?? 0;

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
        Today
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
          tokens
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
            {comparison > 0 ? "+" : ""}{comparison}% vs 7d avg
          </span>
        )}
      </div>

      <div style={{
        display: "flex",
        gap: 16,
        marginTop: 8,
      }}>
        <StatChip
          label="Cost"
          value={formatCost(cost)}
          color="var(--accent-orange)"
          tooltip={
            <InfoTooltip>
              <div style={{ fontWeight: 700, marginBottom: 6 }}>Estimated API cost ($/MTok)</div>
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 9 }}>
                <thead>
                  <tr style={{ opacity: 0.7 }}>
                    <th style={{ textAlign: "left", paddingBottom: 2 }}>Model</th>
                    <th style={{ textAlign: "right", paddingBottom: 2 }}>In</th>
                    <th style={{ textAlign: "right", paddingBottom: 2 }}>Out</th>
                    <th style={{ textAlign: "right", paddingBottom: 2 }}>Cache R</th>
                    <th style={{ textAlign: "right", paddingBottom: 2 }}>Cache W</th>
                  </tr>
                </thead>
                <tbody>
                  <tr><td>Opus</td><td style={{ textAlign: "right" }}>$5</td><td style={{ textAlign: "right" }}>$25</td><td style={{ textAlign: "right" }}>$0.50</td><td style={{ textAlign: "right" }}>$6.25</td></tr>
                  <tr><td>Sonnet</td><td style={{ textAlign: "right" }}>$3</td><td style={{ textAlign: "right" }}>$15</td><td style={{ textAlign: "right" }}>$0.30</td><td style={{ textAlign: "right" }}>$3.75</td></tr>
                  <tr><td>Haiku</td><td style={{ textAlign: "right" }}>$1</td><td style={{ textAlign: "right" }}>$5</td><td style={{ textAlign: "right" }}>$0.10</td><td style={{ textAlign: "right" }}>$1.25</td></tr>
                </tbody>
              </table>
              <div style={{ marginTop: 6, opacity: 0.7, fontSize: 9 }}>
                Pro/Max plan users pay subscription, not per-token.
              </div>
            </InfoTooltip>
          }
        />
        <StatChip label="Messages" value={String(messages)} color="var(--accent-purple)" />
        <StatChip label="Sessions" value={String(sessions)} color="var(--accent-mint)" />
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
