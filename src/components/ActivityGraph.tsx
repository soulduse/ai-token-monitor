import { useState, useMemo, useRef } from "react";
import type { DailyUsage } from "../lib/types";
import { getTotalTokens, toLocalDateStr, formatTokens, formatCost } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { Heatmap3D } from "./Heatmap3D";
import { ActivityStats } from "./ActivityStats";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  daily: DailyUsage[];
}

export interface CellData {
  date: string;
  tokens: number;
  cost: number;
}

const DAY_LABEL_KEYS = ["day.mon", "", "day.wed", "", "day.fri", "", ""];
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

export function ActivityGraph({ daily }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();
  const [viewMode, setViewMode] = useState<"2d" | "3d">("2d");
  const currentYear = new Date().getFullYear();
  const [selectedYear, setSelectedYear] = useState(currentYear);
  const [tooltip, setTooltip] = useState<{
    date: string;
    tokens: number;
    cost: number;
    x: number;
    y: number;
  } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const availableYears = useMemo(() => {
    const years = new Set<number>();
    for (const d of daily) {
      years.add(parseInt(d.date.slice(0, 4)));
    }
    years.add(currentYear);
    return Array.from(years).sort();
  }, [daily, currentYear]);

  const { grid, totalWeeks, monthLabels, thresholds } = useMemo(() => {
    const dateMap = new Map<string, DailyUsage>();
    for (const d of daily) dateMap.set(d.date, d);

    // First Monday on or before Jan 1
    const jan1 = new Date(selectedYear, 0, 1);
    const jan1Dow = jan1.getDay();
    const mondayOffset = jan1Dow === 0 ? -6 : 1 - jan1Dow;
    const startDate = new Date(jan1);
    startDate.setDate(jan1.getDate() + mondayOffset);

    // Last Sunday on or after Dec 31
    const dec31 = new Date(selectedYear, 11, 31);
    const dec31Dow = dec31.getDay();
    const sundayOffset = dec31Dow === 0 ? 0 : 7 - dec31Dow;
    const endDate = new Date(dec31);
    endDate.setDate(dec31.getDate() + sundayOffset);

    const totalDays = Math.round((endDate.getTime() - startDate.getTime()) / 86400000) + 1;
    const totalWeeks = Math.ceil(totalDays / 7);

    const cells: CellData[] = [];
    const values: number[] = [];
    const months = new Map<number, string>();

    for (let i = 0; i < totalWeeks * 7; i++) {
      const d = new Date(startDate);
      d.setDate(startDate.getDate() + i);
      const dateStr = toLocalDateStr(d);
      const usage = dateMap.get(dateStr);
      const tokens = usage ? getTotalTokens(usage.tokens) : 0;
      const cost = usage?.cost_usd ?? 0;
      cells.push({ date: dateStr, tokens, cost });
      if (tokens > 0) values.push(tokens);

      const dayIdx = i % 7;
      const weekIdx = Math.floor(i / 7);
      if (dayIdx === 0 && d.getDate() <= 7) {
        months.set(weekIdx, d.toLocaleDateString("en", { month: "short" }));
      }
    }

    values.sort((a, b) => a - b);
    const quantiles = [0.25, 0.5, 0.75, 0.9];
    const thresholds = quantiles.map(
      (q) => values[Math.floor(q * values.length)] || 0
    );

    const grid: CellData[][] = Array.from({ length: 7 }, () => []);
    for (let col = 0; col < totalWeeks; col++) {
      for (let row = 0; row < 7; row++) {
        grid[row].push(cells[col * 7 + row]);
      }
    }

    const monthLabels: { col: number; label: string }[] = [];
    for (const [col, label] of months.entries()) {
      monthLabels.push({ col, label });
    }

    return { grid, totalWeeks, monthLabels, thresholds };
  }, [daily, selectedYear]);

  const todayStr = toLocalDateStr(new Date());
  const canGoPrev = selectedYear > availableYears[0];
  const canGoNext = selectedYear < currentYear;

  const cellSize = 5;
  const cellGap = 1;
  const labelW = 20;

  return (
    <div
      ref={containerRef}
      style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-lg)",
        padding: 16,
        boxShadow: "var(--shadow-card)",
        position: "relative",
      }}
    >
      {/* Header */}
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 10,
      }}>
        <div style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-secondary)",
          textTransform: "uppercase",
          letterSpacing: "0.5px",
        }}>
          {t("activity.title")}
        </div>

        {/* 2D / 3D toggle */}
        <div style={{
          display: "flex",
          background: "var(--heat-0)",
          borderRadius: 6,
          padding: 2,
        }}>
          {(["2d", "3d"] as const).map((m) => (
            <button
              key={m}
              onClick={() => setViewMode(m)}
              style={{
                fontSize: 10,
                fontWeight: 600,
                padding: "2px 8px",
                borderRadius: 4,
                border: "none",
                cursor: "pointer",
                background: viewMode === m ? "var(--accent-purple)" : "transparent",
                color: viewMode === m ? "#fff" : "var(--text-secondary)",
                transition: "all 0.15s ease",
                textTransform: "uppercase",
              }}
            >
              {m}
            </button>
          ))}
        </div>

        {/* Year navigation */}
        <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
          <button
            onClick={() => canGoPrev && setSelectedYear(selectedYear - 1)}
            disabled={!canGoPrev}
            style={{
              background: "none",
              border: "none",
              cursor: canGoPrev ? "pointer" : "default",
              color: canGoPrev ? "var(--text-secondary)" : "var(--heat-1)",
              fontSize: 12,
              fontWeight: 700,
              padding: "2px 4px",
            }}
          >
            {"<"}
          </button>
          <span style={{
            fontSize: 12,
            fontWeight: 700,
            color: "var(--accent-purple)",
            minWidth: 36,
            textAlign: "center",
          }}>
            {selectedYear}
          </span>
          <button
            onClick={() => canGoNext && setSelectedYear(selectedYear + 1)}
            disabled={!canGoNext}
            style={{
              background: "none",
              border: "none",
              cursor: canGoNext ? "pointer" : "default",
              color: canGoNext ? "var(--text-secondary)" : "var(--heat-1)",
              fontSize: 12,
              fontWeight: 700,
              padding: "2px 4px",
            }}
          >
            {">"}
          </button>
        </div>
      </div>

      {/* Graph */}
      {viewMode === "2d" ? (
        <>
          {/* Month labels */}
          <div style={{
            display: "flex",
            marginLeft: labelW,
            marginBottom: 2,
          }}>
            {Array.from({ length: totalWeeks }).map((_, col) => {
              const label = monthLabels.find((m) => m.col === col);
              return (
                <div key={col} style={{
                  width: cellSize + cellGap,
                  fontSize: 7,
                  color: "var(--text-secondary)",
                  fontWeight: 600,
                  flexShrink: 0,
                }}>
                  {label?.label ?? ""}
                </div>
              );
            })}
          </div>

          {/* 2D Grid */}
          <div style={{ display: "flex" }}>
            {/* Day labels */}
            <div style={{
              display: "flex",
              flexDirection: "column",
              gap: cellGap,
              marginRight: 2,
              width: labelW - 2,
            }}>
              {DAY_LABEL_KEYS.map((key, i) => (
                <div key={i} style={{
                  height: cellSize,
                  fontSize: 7,
                  color: "var(--text-secondary)",
                  fontWeight: 600,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "flex-end",
                }}>
                  {key ? t(key) : ""}
                </div>
              ))}
            </div>

            {/* Cells */}
            <div style={{ display: "flex", flexDirection: "column", gap: cellGap }}>
              {grid.map((row, rowIdx) => (
                <div key={rowIdx} style={{ display: "flex", gap: cellGap }}>
                  {row.map((cell, colIdx) => {
                    const level = getHeatLevel(cell.tokens, thresholds);
                    const isToday = cell.date === todayStr;
                    return (
                      <div
                        key={colIdx}
                        style={{
                          width: cellSize,
                          height: cellSize,
                          borderRadius: 1,
                          background: HEAT_COLORS[level],
                          cursor: "pointer",
                          ...(isToday ? {
                            outline: "1.5px solid var(--accent-purple)",
                            outlineOffset: -0.5,
                          } : {}),
                        }}
                        onMouseEnter={(e) => {
                          const rect = containerRef.current?.getBoundingClientRect();
                          if (!rect) return;
                          const cellRect = e.currentTarget.getBoundingClientRect();
                          setTooltip({
                            date: cell.date,
                            tokens: cell.tokens,
                            cost: cell.cost,
                            x: cellRect.left + cellRect.width / 2 - rect.left,
                            y: cellRect.top - rect.top,
                          });
                        }}
                        onMouseLeave={() => setTooltip(null)}
                      />
                    );
                  })}
                </div>
              ))}
            </div>
          </div>
        </>
      ) : (
        <Heatmap3D
          grid={grid}
          totalWeeks={totalWeeks}
          thresholds={thresholds}
          onHover={(cell, x, y) => cell ? setTooltip({ ...cell, x, y }) : setTooltip(null)}
          containerRef={containerRef}
        />
      )}

      {/* Legend */}
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: 3,
        marginTop: 8,
        justifyContent: "flex-end",
      }}>
        <span style={{ fontSize: 8, color: "var(--text-secondary)", marginRight: 2 }}>{t("activity.less")}</span>
        {HEAT_COLORS.map((color, i) => (
          <div key={i} style={{
            width: 8,
            height: 8,
            borderRadius: 1,
            background: color,
          }} />
        ))}
        <span style={{ fontSize: 8, color: "var(--text-secondary)", marginLeft: 2 }}>{t("activity.more")}</span>
      </div>

      {/* Tooltip */}
      {tooltip && (
        <div style={{
          position: "absolute",
          left: tooltip.x,
          top: tooltip.y - 4,
          transform: "translateX(-50%) translateY(-100%)",
          background: "var(--text-primary)",
          color: "var(--bg-primary)",
          padding: "3px 6px",
          borderRadius: 4,
          fontSize: 9,
          fontWeight: 600,
          whiteSpace: "nowrap",
          pointerEvents: "none",
          zIndex: 20,
          boxShadow: "0 2px 8px rgba(0,0,0,0.15)",
        }}>
          <div>{new Date(tooltip.date + "T00:00:00").toLocaleDateString("en", { month: "short", day: "numeric", year: "numeric" })}</div>
          <div>
            {formatTokens(tooltip.tokens, prefs.number_format)} tokens · {formatCost(tooltip.cost)}
          </div>
        </div>
      )}

      {/* Stats */}
      <ActivityStats daily={daily} year={selectedYear} />
    </div>
  );
}
