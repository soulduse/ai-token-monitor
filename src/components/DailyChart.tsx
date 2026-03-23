import { useState, useMemo } from "react";
import type { DailyUsage } from "../lib/types";
import { formatTokens, formatCost, getTotalTokens, formatDate, toLocalDateStr } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  daily: DailyUsage[];
  days?: number;
}

export function DailyChart({ daily, days = 7 }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();
  const [mode, setMode] = useState<"tokens" | "cost">("tokens");
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

  const dailyMap = useMemo(() => {
    const map = new Map<string, DailyUsage>();
    for (const d of daily) map.set(d.date, d);
    return map;
  }, [daily]);

  const chartData = useMemo(() => {
    const today = new Date();
    const result: { date: string; tokens: number; cost: number }[] = [];

    for (let i = days - 1; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(today.getDate() - i);
      const dateStr = toLocalDateStr(d);
      const usage = dailyMap.get(dateStr);
      result.push({
        date: dateStr,
        tokens: usage ? getTotalTokens(usage.tokens) : 0,
        cost: usage?.cost_usd ?? 0,
      });
    }
    return result;
  }, [dailyMap, days]);

  const values = chartData.map((d) => (mode === "tokens" ? d.tokens : d.cost));
  const maxVal = Math.max(...values, 1);
  const avg = values.reduce((a, b) => a + b, 0) / values.length;

  const W = 368;
  const H = 100;
  const yAxisWidth = 36;
  const chartW = W - yAxisWidth;
  const barGap = 4;
  const barWidth = Math.max(4, (chartW - barGap * (days + 1)) / days);
  const avgY = H - (avg / maxVal) * (H - 10);

  // Y-axis ticks: 0, mid, max
  const yTicks = [0, Math.round(maxVal / 2), Math.round(maxVal)];
  const formatYTick = (v: number) => {
    if (mode === "cost") {
      return v >= 1 ? `$${Math.round(v)}` : `$${v.toFixed(1)}`;
    }
    if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
    if (v >= 1_000) return `${(v / 1_000).toFixed(0)}K`;
    return `${v}`;
  };

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 16,
      boxShadow: "var(--shadow-card)",
    }}>
      <div style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        marginBottom: 10,
      }}>
        <div style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-secondary)",
          textTransform: "uppercase",
          letterSpacing: "0.5px",
        }}>
          {t("daily.title", { days })}
        </div>
        <div style={{
          display: "flex",
          background: "var(--heat-0)",
          borderRadius: 6,
          padding: 2,
        }}>
          {(["tokens", "cost"] as const).map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              style={{
                fontSize: 10,
                fontWeight: 600,
                padding: "2px 8px",
                borderRadius: 4,
                border: "none",
                cursor: "pointer",
                background: mode === m ? "var(--accent-purple)" : "transparent",
                color: mode === m ? "#fff" : "var(--text-secondary)",
                transition: "all 0.15s ease",
              }}
            >
              {m === "tokens" ? t("daily.tokens") : t("daily.cost")}
            </button>
          ))}
        </div>
      </div>

      <div style={{ position: "relative" }}>
        <svg
          viewBox={`0 0 ${W} ${H}`}
          width="100%"
          height={H}
          style={{ display: "block" }}
        >
          <defs>
            <linearGradient id="barGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="var(--accent-purple)" />
              <stop offset="100%" stopColor="var(--accent-pink)" />
            </linearGradient>
          </defs>

          {/* Y-axis labels */}
          {yTicks.map((tick, i) => {
            const tickY = maxVal > 0 ? H - (tick / maxVal) * (H - 10) : H;
            return (
              <text
                key={i}
                x={yAxisWidth - 4}
                y={tick === 0 ? tickY - 2 : tickY + 3}
                textAnchor="end"
                fontSize={8}
                fontWeight={600}
                fill="var(--text-secondary)"
                opacity={0.9}
              >
                {formatYTick(tick)}
              </text>
            );
          })}

          {/* Average line */}
          <line
            x1={yAxisWidth}
            y1={avgY}
            x2={W}
            y2={avgY}
            stroke="var(--text-secondary)"
            strokeWidth={0.5}
            strokeDasharray="4 3"
            opacity={0.7}
          />

          {/* Bars */}
          {values.map((val, i) => {
            const barH = maxVal > 0 ? (val / maxVal) * (H - 10) : 0;
            const x = yAxisWidth + barGap + i * (barWidth + barGap);
            const y = H - barH;
            const isHovered = hoveredIdx === i;

            return (
              <rect
                key={i}
                x={x}
                y={y}
                width={barWidth}
                height={barH}
                rx={Math.min(barWidth / 2, 3)}
                fill="url(#barGrad)"
                opacity={isHovered ? 1 : 0.8}
                onMouseEnter={() => setHoveredIdx(i)}
                onMouseLeave={() => setHoveredIdx(null)}
                style={{ cursor: "pointer", transition: "opacity 0.15s ease" }}
              />
            );
          })}
        </svg>

        {/* Tooltip */}
        {hoveredIdx !== null && (
          <div style={{
            position: "absolute",
            top: -4,
            left: `${((hoveredIdx + 0.5) / days) * 100}%`,
            transform: "translateX(-50%) translateY(-100%)",
            background: "var(--text-primary)",
            color: "var(--bg-primary)",
            padding: "4px 8px",
            borderRadius: 6,
            fontSize: 10,
            fontWeight: 600,
            whiteSpace: "nowrap",
            pointerEvents: "none",
            zIndex: 10,
            boxShadow: "0 2px 8px rgba(0,0,0,0.15)",
          }}>
            <div>{formatDate(chartData[hoveredIdx].date)}</div>
            <div>
              {mode === "tokens"
                ? formatTokens(chartData[hoveredIdx].tokens, prefs.number_format)
                : formatCost(chartData[hoveredIdx].cost)}
            </div>
          </div>
        )}
      </div>

      {/* Day labels */}
      <div style={{
        display: "flex",
        justifyContent: "space-between",
        marginTop: 4,
        paddingLeft: yAxisWidth + barGap,
        paddingRight: barGap,
      }}>
        {chartData.map((d, i) => {
          if (days > 14 && i % Math.ceil(days / 7) !== 0) return <span key={i} />;
          const dayLabel = new Date(d.date + "T00:00:00").toLocaleDateString("en", { weekday: "short" });
          return (
            <span key={i} style={{
              fontSize: 8,
              color: "var(--text-secondary)",
              fontWeight: 600,
              textAlign: "center",
              flex: days <= 14 ? 1 : undefined,
            }}>
              {days <= 7 ? dayLabel : formatDate(d.date)}
            </span>
          );
        })}
      </div>
    </div>
  );
}
