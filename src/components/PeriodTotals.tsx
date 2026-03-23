import { useState, useMemo } from "react";
import type { DailyUsage } from "../lib/types";
import { formatTokens, formatCost, getTotalTokens, toLocalDateStr } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { PeriodSelector } from "./PeriodSelector";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  daily: DailyUsage[];
}

function getWeekRange(offset: number): { start: string; end: string; labelKey: string | null; labelFallback: string } {
  const now = new Date();
  const dow = now.getDay();
  const mondayOffset = dow === 0 ? 6 : dow - 1;
  const monday = new Date(now);
  monday.setDate(now.getDate() - mondayOffset - offset * 7);
  const sunday = new Date(monday);
  sunday.setDate(monday.getDate() + 6);

  const start = toLocalDateStr(monday);
  const end = toLocalDateStr(sunday);

  if (offset === 0) return { start, end, labelKey: "period.thisWeek", labelFallback: "" };
  if (offset === 1) return { start, end, labelKey: "period.lastWeek", labelFallback: "" };

  const m1 = monday.toLocaleDateString("en", { month: "short", day: "numeric" });
  const m2 = sunday.toLocaleDateString("en", { day: "numeric" });
  return { start, end, labelKey: null, labelFallback: `${m1}-${m2}` };
}

function getMonthRange(offset: number): { start: string; end: string; labelKey: string | null; labelFallback: string } {
  const now = new Date();
  const targetMonth = new Date(now.getFullYear(), now.getMonth() - offset, 1);
  const lastDay = new Date(targetMonth.getFullYear(), targetMonth.getMonth() + 1, 0);

  const start = toLocalDateStr(targetMonth);
  const end = toLocalDateStr(lastDay);

  if (offset === 0) return { start, end, labelKey: "period.thisMonth", labelFallback: "" };
  if (offset === 1) return { start, end, labelKey: "period.lastMonth", labelFallback: "" };

  const monthLabel = targetMonth.toLocaleDateString("en", { month: "short", year: "numeric" });
  return { start, end, labelKey: null, labelFallback: monthLabel };
}

function aggregate(daily: DailyUsage[], start: string, end: string) {
  let tokens = 0;
  let cost = 0;
  let sessions = 0;
  let messages = 0;

  for (const d of daily) {
    if (d.date >= start && d.date <= end) {
      tokens += getTotalTokens(d.tokens);
      cost += d.cost_usd;
      sessions += d.sessions;
      messages += d.messages;
    }
  }

  return { tokens, cost, sessions, messages };
}

export function PeriodTotals({ daily }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();
  const [weekOffset, setWeekOffset] = useState(0);
  const [monthOffset, setMonthOffset] = useState(0);

  const weekRange = useMemo(() => getWeekRange(weekOffset), [weekOffset]);
  const monthRange = useMemo(() => getMonthRange(monthOffset), [monthOffset]);

  const weekData = useMemo(
    () => aggregate(daily, weekRange.start, weekRange.end),
    [daily, weekRange]
  );
  const monthData = useMemo(
    () => aggregate(daily, monthRange.start, monthRange.end),
    [daily, monthRange]
  );

  const weekLabel = weekRange.labelKey ? t(weekRange.labelKey) : weekRange.labelFallback;
  const monthLabel = monthRange.labelKey ? t(monthRange.labelKey) : monthRange.labelFallback;

  return (
    <div style={{
      display: "flex",
      gap: 10,
    }}>
      <PeriodCard
        label={weekLabel}
        tokens={weekData.tokens}
        cost={weekData.cost}
        sessions={weekData.sessions}
        color="var(--accent-purple)"
        numberFormat={prefs.number_format}
        onPrev={() => setWeekOffset((o) => o + 1)}
        onNext={() => setWeekOffset((o) => Math.max(0, o - 1))}
        canNext={weekOffset > 0}
      />
      <PeriodCard
        label={monthLabel}
        tokens={monthData.tokens}
        cost={monthData.cost}
        sessions={monthData.sessions}
        color="var(--accent-pink)"
        numberFormat={prefs.number_format}
        onPrev={() => setMonthOffset((o) => o + 1)}
        onNext={() => setMonthOffset((o) => Math.max(0, o - 1))}
        canNext={monthOffset > 0}
      />
    </div>
  );
}

function PeriodCard({
  label,
  tokens,
  cost,
  sessions,
  color,
  numberFormat,
  onPrev,
  onNext,
  canNext,
}: {
  label: string;
  tokens: number;
  cost: number;
  sessions: number;
  color: string;
  numberFormat: "compact" | "full";
  onPrev: () => void;
  onNext: () => void;
  canNext: boolean;
}) {
  const t = useI18n();
  return (
    <div style={{
      flex: 1,
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 14,
      boxShadow: "var(--shadow-card)",
    }}>
      <PeriodSelector
        label={label}
        onPrev={onPrev}
        onNext={onNext}
        canNext={canNext}
      />
      <div style={{
        fontSize: 20,
        fontWeight: 800,
        color,
        letterSpacing: "-0.5px",
        marginTop: 6,
      }}>
        {formatTokens(tokens, numberFormat)}
      </div>
      <div style={{
        display: "flex",
        gap: 8,
        marginTop: 4,
        fontSize: 10,
        color: "var(--text-secondary)",
        fontWeight: 600,
      }}>
        <span>{formatCost(cost)}</span>
        <span>&middot;</span>
        <span>{sessions} {t("period.sessions")}</span>
      </div>
    </div>
  );
}
