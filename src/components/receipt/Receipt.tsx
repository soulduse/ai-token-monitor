import { useMemo, forwardRef } from "react";
import type { AllStats } from "../../lib/types";
import { formatTokens, formatCost, getModelTotalTokens } from "../../lib/format";
import {
  filterByPeriod,
  computeTotalCost,
  computeTotalTokens,
  computeStreaks,
  computeCacheSavings,
  shortenModelName,
} from "../../lib/statsHelpers";
import type { Period } from "../../lib/statsHelpers";
import { useI18n } from "../../i18n/I18nContext";

interface Props {
  stats: AllStats;
  period: Period;
  appVersion: string;
}

const MONO = "'Courier New', Courier, monospace";

// Fixed receipt colors (always light, like real paper)
const C = {
  bg: "#fefdf8",
  text: "#1a1a1a",
  textSec: "#666",
  accent: "#7C5CFC",
  mint: "#059669",
  sep: "#999",
  barcode: "#1a1a1a",
};

export const Receipt = forwardRef<HTMLDivElement, Props>(
  ({ stats, period, appVersion }, ref) => {
    const t = useI18n();

    const data = useMemo(() => {
      const filtered = filterByPeriod(stats.daily, period);
      const totalCost = computeTotalCost(filtered);
      const totalTokens = computeTotalTokens(filtered);
      const cacheSavings = computeCacheSavings(filtered);
      const messages = filtered.reduce((s, d) => s + d.messages, 0);
      const sessions = filtered.reduce((s, d) => s + d.sessions, 0);
      const toolCalls = filtered.reduce((s, d) => s + d.tool_calls, 0);
      const streaks = computeStreaks(stats.daily);

      // Aggregate model usage from filtered daily data
      const modelMap = new Map<string, { tokens: number; cost: number }>();
      for (const day of filtered) {
        for (const [model, tokens] of Object.entries(day.tokens)) {
          const existing = modelMap.get(model) ?? { tokens: 0, cost: 0 };
          existing.tokens += tokens;
          modelMap.set(model, existing);
        }
      }
      // Calculate cost per model from overall model_usage ratios
      for (const [model, info] of modelMap) {
        const overallUsage = stats.model_usage[model];
        if (overallUsage) {
          const overallTotal = getModelTotalTokens(overallUsage);
          if (overallTotal > 0) {
            info.cost = (info.tokens / overallTotal) * overallUsage.cost_usd;
          }
        }
      }

      const models = Array.from(modelMap.entries())
        .map(([name, info]) => ({ name: shortenModelName(name), tokens: info.tokens, cost: info.cost }))
        .filter((m) => m.tokens > 0)
        .sort((a, b) => b.cost - a.cost);

      return { totalCost, totalTokens, cacheSavings, messages, sessions, toolCalls, models, streaks };
    }, [stats, period]);

    const now = new Date();
    const dayNames = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const dateStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")} (${dayNames[now.getDay()]})`;

    return (
      <div
        ref={ref}
        style={{
          width: 320,
          padding: "24px 20px",
          fontFamily: MONO,
          fontSize: 12,
          lineHeight: 1.6,
          color: C.text,
          background: C.bg,
          borderRadius: 12,
          whiteSpace: "pre",
          position: "relative",
          overflow: "hidden",
        }}
      >
        {/* Header */}
        <div style={{ textAlign: "center", fontWeight: 700, fontSize: 14, color: C.text }}>
          AI TOKEN MONITOR
        </div>
        <div style={{ textAlign: "center", fontSize: 10, color: C.textSec }}>
          {dateStr}
        </div>

        <Separator double />

        {/* Line items */}
        {data.models.map((m) => (
          <div key={m.name} style={{ display: "flex", justifyContent: "space-between", color: C.text }}>
            <span>{m.name}</span>
            <span>
              {formatTokens(m.tokens, "compact").padStart(7)}{" "}
              {formatCost(m.cost).padStart(7)}
            </span>
          </div>
        ))}
        {data.models.length === 0 && (
          <div style={{ textAlign: "center", color: C.textSec }}>
            {t("receipt.noData")}
          </div>
        )}

        <Separator />

        {/* Subtotal */}
        <div style={{ display: "flex", justifyContent: "space-between", color: C.text }}>
          <span>{t("receipt.subtotal")}</span>
          <span style={{ fontWeight: 700 }}>{formatCost(data.totalCost + data.cacheSavings)}</span>
        </div>
        {data.cacheSavings > 0 && (
          <div style={{ display: "flex", justifyContent: "space-between", color: C.mint }}>
            <span>{t("receipt.cacheDiscount")} ~</span>
            <span>-{formatCost(data.cacheSavings)}</span>
          </div>
        )}

        <Separator double />

        {/* Total */}
        <div style={{
          display: "flex",
          justifyContent: "space-between",
          fontWeight: 700,
          fontSize: 14,
          color: C.text,
        }}>
          <span>{t("receipt.total")}</span>
          <span>{formatCost(data.totalCost)}</span>
        </div>

        <Separator double />

        {/* Stats */}
        <div style={{ fontSize: 11, color: C.textSec }}>
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span>{t("receipt.messages")}</span>
            <span>{data.messages.toLocaleString()}</span>
          </div>
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span>{t("receipt.sessions")}</span>
            <span>{data.sessions.toLocaleString()}</span>
          </div>
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span>{t("receipt.toolCalls")}</span>
            <span>{data.toolCalls.toLocaleString()}</span>
          </div>
        </div>

        {/* Streak */}
        {data.streaks.currentStreak > 0 && (
          <>
            <div style={{ height: 8 }} />
            <div style={{
              textAlign: "center",
              fontWeight: 700,
              fontSize: 12,
              color: C.accent,
            }}>
              ★ {data.streaks.currentStreak} DAY STREAK ★
            </div>
          </>
        )}

        <Separator />

        {/* Footer */}
        <div style={{
          textAlign: "center",
          fontWeight: 700,
          fontSize: 11,
          color: C.textSec,
        }}>
          {t("receipt.thankYou")}
        </div>

        <Separator />

        {/* Barcode decoration */}
        <div style={{
          display: "flex",
          justifyContent: "center",
          gap: 1.5,
          height: 28,
          margin: "4px 0",
        }}>
          {Array.from({ length: 40 }, (_, i) => (
            <div
              key={i}
              style={{
                width: i % 3 === 0 ? 2.5 : 1,
                height: "100%",
                background: C.barcode,
                opacity: 0.3,
              }}
            />
          ))}
        </div>

        <div style={{
          textAlign: "center",
          fontSize: 9,
          color: C.textSec,
        }}>
          v{appVersion}
        </div>
      </div>
    );
  }
);

function Separator({ double }: { double?: boolean }) {
  return (
    <div style={{
      borderBottom: double
        ? `2px double ${C.sep}`
        : `1px dashed ${C.sep}`,
      opacity: 0.3,
      margin: "6px 0",
    }} />
  );
}
