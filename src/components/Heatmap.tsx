import { useState, useMemo } from "react";
import type { DailyUsage } from "../lib/types";
import { getTotalTokens, toLocalDateStr } from "../lib/format";
import { HeatmapCell } from "./HeatmapCell";
import { Tooltip } from "./Tooltip";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  daily: DailyUsage[];
  weeks?: number;
}

const DEFAULT_WEEKS = 12;
const DAYS = 7;
const DAY_LABEL_KEYS = ["day.mon", "", "day.wed", "", "day.fri", "", "day.sun"];
const HEAT_COLORS = [
  "var(--heat-0)",
  "var(--heat-1)",
  "var(--heat-2)",
  "var(--heat-3)",
  "var(--heat-4)",
];

function getHeatLevel(value: number, thresholds: number[]): number {
  if (value === 0) return 0;
  for (let i = thresholds.length - 1; i >= 0; i--) {
    if (value >= thresholds[i]) return i + 1;
  }
  return 1;
}

export function Heatmap({ daily, weeks: WEEKS = DEFAULT_WEEKS }: Props) {
  const t = useI18n();
  const [tooltip, setTooltip] = useState<{
    date: string;
    tokens: number;
    cost: number;
    x: number;
    y: number;
  } | null>(null);

  const { grid, monthLabels, thresholds } = useMemo(() => {
    // Build date map
    const dateMap = new Map<string, DailyUsage>();
    for (const d of daily) {
      dateMap.set(d.date, d);
    }

    // Calculate grid dates (last 12 weeks ending today)
    const today = new Date();
    const todayDow = today.getDay(); // 0=Sun
    // Adjust to Monday-based: Mon=0, Sun=6
    const mondayDow = todayDow === 0 ? 6 : todayDow - 1;
    const endOfWeek = new Date(today);
    endOfWeek.setDate(today.getDate() + (6 - mondayDow)); // Go to Sunday

    const startDate = new Date(endOfWeek);
    startDate.setDate(endOfWeek.getDate() - WEEKS * 7 + 1);

    const cells: { date: string; tokens: number; cost: number }[] = [];
    const values: number[] = [];
    const months = new Map<number, string>();

    for (let i = 0; i < WEEKS * DAYS; i++) {
      const d = new Date(startDate);
      d.setDate(startDate.getDate() + i);
      const dateStr = d.toISOString().slice(0, 10);
      const usage = dateMap.get(dateStr);
      const tokens = usage ? getTotalTokens(usage.tokens) : 0;
      const cost = usage?.cost_usd ?? 0;
      cells.push({ date: dateStr, tokens, cost });
      if (tokens > 0) values.push(tokens);

      // Month labels (at column level)
      const weekIdx = Math.floor(i / DAYS);
      const dayIdx = i % DAYS;
      if (dayIdx === 0) {
        const month = d.toLocaleDateString("en", { month: "short" });
        if (!months.has(weekIdx) || d.getDate() <= 7) {
          if (d.getDate() <= 7) {
            months.set(weekIdx, month);
          }
        }
      }
    }

    // Calculate quantile thresholds
    values.sort((a, b) => a - b);
    const quantiles = [0.25, 0.5, 0.75, 0.9];
    const thresholds = quantiles.map(
      (q) => values[Math.floor(q * values.length)] || 0
    );

    // Build grid: grid[row][col] where row=day(0=Mon), col=week
    type Cell = { date: string; tokens: number; cost: number };
    const grid: Cell[][] = Array.from({ length: DAYS }, () => []);
    for (let col = 0; col < WEEKS; col++) {
      for (let row = 0; row < DAYS; row++) {
        grid[row].push(cells[col * DAYS + row]);
      }
    }

    // Month labels array
    const monthLabels: { col: number; label: string }[] = [];
    for (const [col, label] of months.entries()) {
      monthLabels.push({ col, label });
    }

    return { grid, monthLabels, thresholds };
  }, [daily, WEEKS]);

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 16,
      boxShadow: "var(--shadow-card)",
    }}>
      <div style={{
        fontSize: 12,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
        marginBottom: 10,
      }}>
        {t("activity.weeks", { weeks: WEEKS })}
      </div>

      {/* Month labels */}
      <div style={{
        display: "flex",
        marginLeft: 28,
        marginBottom: 4,
        gap: 0,
      }}>
        {Array.from({ length: WEEKS }).map((_, col) => {
          const label = monthLabels.find((m) => m.col === col);
          return (
            <div key={col} style={{
              width: 18,
              fontSize: 10,
              color: "var(--text-secondary)",
              fontWeight: 600,
            }}>
              {label?.label ?? ""}
            </div>
          );
        })}
      </div>

      {/* Grid */}
      <div style={{ display: "flex", gap: 0 }}>
        {/* Day labels */}
        <div style={{
          display: "flex",
          flexDirection: "column",
          gap: 4,
          marginRight: 4,
          paddingTop: 0,
        }}>
          {DAY_LABEL_KEYS.map((key, i) => (
            <div key={i} style={{
              height: 14,
              fontSize: 10,
              color: "var(--text-secondary)",
              fontWeight: 600,
              display: "flex",
              alignItems: "center",
              width: 24,
              justifyContent: "flex-end",
            }}>
              {key ? t(key) : ""}
            </div>
          ))}
        </div>

        {/* Cells */}
        <div style={{
          display: "flex",
          flexDirection: "column",
          gap: 4,
        }}>
          {(() => {
            const todayStr = toLocalDateStr(new Date());
            return grid.map((row, rowIdx) => (
            <div key={rowIdx} style={{ display: "flex", gap: 4 }}>
              {row.map((cell, colIdx) => {
                const level = getHeatLevel(cell.tokens, thresholds);
                return (
                  <HeatmapCell
                    key={`${rowIdx}-${colIdx}`}
                    color={HEAT_COLORS[level]}
                    isToday={cell.date === todayStr}
                    onMouseEnter={(e) => {
                      const rect = e.currentTarget.getBoundingClientRect();
                      setTooltip({
                        date: cell.date,
                        tokens: cell.tokens,
                        cost: cell.cost,
                        x: rect.left + rect.width / 2,
                        y: rect.top,
                      });
                    }}
                    onMouseLeave={() => setTooltip(null)}
                  />
                );
              })}
            </div>
          ));
          })()}
        </div>
      </div>

      {/* Legend */}
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: 4,
        marginTop: 10,
        justifyContent: "flex-end",
      }}>
        <span style={{ fontSize: 10, color: "var(--text-secondary)", marginRight: 4 }}>{t("activity.less")}</span>
        {HEAT_COLORS.map((color, i) => (
          <div key={i} style={{
            width: 10,
            height: 10,
            borderRadius: 2,
            background: color,
          }} />
        ))}
        <span style={{ fontSize: 10, color: "var(--text-secondary)", marginLeft: 4 }}>{t("activity.more")}</span>
      </div>

      {tooltip && (
        <Tooltip
          date={tooltip.date}
          tokens={tooltip.tokens}
          cost={tooltip.cost}
          x={tooltip.x}
          y={tooltip.y}
          visible
        />
      )}
    </div>
  );
}
