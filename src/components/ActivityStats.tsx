import { useMemo, useState } from "react";
import type { DailyUsage } from "../lib/types";
import { getTotalTokens, toLocalDateStr, formatTokens } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  daily: DailyUsage[];
  year: number;
}

function shortDate(dateStr: string): string {
  return new Date(dateStr + "T00:00:00").toLocaleDateString("en", {
    month: "short",
    day: "numeric",
  });
}

export function ActivityStats({ daily, year }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();

  const stats = useMemo(() => {
    const yearData = daily
      .filter((d) => d.date.startsWith(`${year}-`))
      .map((d) => ({ date: d.date, tokens: getTotalTokens(d.tokens) }));

    // Total tokens
    const total = yearData.reduce((sum, d) => sum + d.tokens, 0);

    // Active days
    const activeDays = yearData.filter((d) => d.tokens > 0).length;

    // Average per day
    const average = activeDays > 0 ? Math.round(total / activeDays) : 0;

    // Best day
    let bestDay = { date: "", tokens: 0 };
    for (const d of yearData) {
      if (d.tokens > bestDay.tokens) bestDay = d;
    }

    // This week
    const today = new Date();
    const dow = today.getDay();
    const mondayOffset = dow === 0 ? 6 : dow - 1;
    const monday = new Date(today);
    monday.setDate(today.getDate() - mondayOffset);
    const mondayStr = toLocalDateStr(monday);
    const todayStr = toLocalDateStr(today);
    const weekTotal = yearData
      .filter((d) => d.date >= mondayStr && d.date <= todayStr)
      .reduce((sum, d) => sum + d.tokens, 0);

    // Streaks
    const activeDates = new Set(
      yearData.filter((d) => d.tokens > 0).map((d) => d.date)
    );

    // Current streak (from today backwards)
    let currentStreak = 0;
    let currentStart = todayStr;
    const checkDate = new Date(today);
    while (true) {
      const ds = toLocalDateStr(checkDate);
      if (activeDates.has(ds)) {
        currentStreak++;
        currentStart = ds;
        checkDate.setDate(checkDate.getDate() - 1);
      } else {
        break;
      }
    }

    // Longest streak
    const sortedDates = Array.from(activeDates).sort();
    let longestStreak = 0;
    let longestStart = "";
    let longestEnd = "";
    let streak = 0;
    let streakStart = "";
    let prevDate = "";

    for (const ds of sortedDates) {
      if (prevDate) {
        const prev = new Date(prevDate + "T00:00:00");
        const curr = new Date(ds + "T00:00:00");
        const diff = (curr.getTime() - prev.getTime()) / 86400000;
        if (diff === 1) {
          streak++;
        } else {
          if (streak > longestStreak) {
            longestStreak = streak;
            longestStart = streakStart;
            longestEnd = prevDate;
          }
          streak = 1;
          streakStart = ds;
        }
      } else {
        streak = 1;
        streakStart = ds;
      }
      prevDate = ds;
    }
    if (streak > longestStreak) {
      longestStreak = streak;
      longestStart = streakStart;
      longestEnd = prevDate;
    }

    return {
      total,
      weekTotal,
      bestDay,
      average,
      currentStreak,
      currentStart,
      currentEnd: todayStr,
      longestStreak,
      longestStart,
      longestEnd,
    };
  }, [daily, year]);

  const fmt = prefs.number_format;

  return (
    <div style={{ marginTop: 12 }}>
      {/* Summary row */}
      <div style={{
        display: "flex",
        gap: 8,
        marginBottom: 8,
      }}>
        <StatBox
          value={formatTokens(stats.total, fmt)}
          label={t("activity.total")}
          sub={`${shortDate(`${year}-01-01`)} → ${year === new Date().getFullYear() ? shortDate(toLocalDateStr(new Date())) : shortDate(`${year}-12-31`)}`}
        />
        <StatBox
          value={formatTokens(stats.weekTotal, fmt)}
          label={t("activity.thisWeek")}
        />
        <StatBox
          value={stats.bestDay.tokens > 0 ? formatTokens(stats.bestDay.tokens, fmt) : "—"}
          label={t("activity.bestDay")}
          sub={stats.bestDay.date ? shortDate(stats.bestDay.date) : ""}
        />
      </div>

      {/* Average */}
      <div style={{
        fontSize: 11,
        color: "var(--text-secondary)",
        fontWeight: 600,
        marginBottom: 10,
        textAlign: "right",
      }}>
        {t("activity.average")}: <span style={{ color: "var(--accent-purple)" }}>{formatTokens(stats.average, fmt)}</span> {t("activity.perDay")}
      </div>

      {/* Streaks */}
      <div style={{
        fontSize: 12,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
        marginBottom: 6,
      }}>
        {t("activity.streaks")}
      </div>
      <div style={{ display: "flex", gap: 8 }}>
        <StreakBox
          days={stats.longestStreak}
          label={t("activity.longest")}
          start={stats.longestStart}
          end={stats.longestEnd}
        />
        <StreakBox
          days={stats.currentStreak}
          label={t("activity.current")}
          start={stats.currentStart}
          end={stats.currentEnd}
        />
      </div>
    </div>
  );
}

function StatBox({ value, label, sub }: { value: string; label: string; sub?: string }) {
  const [hover, setHover] = useState(false);
  return (
    <div
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        flex: 1,
        background: "var(--bg-primary)",
        borderRadius: "var(--radius-sm)",
        padding: "8px 10px",
        minWidth: 0,
        overflow: "visible",
        cursor: "default",
        position: "relative",
      }}
    >
      {hover && (
        <div style={{
          position: "absolute",
          bottom: "100%",
          left: "50%",
          transform: "translateX(-50%)",
          marginBottom: 4,
          background: "var(--text-primary)",
          color: "var(--bg-primary)",
          padding: "4px 8px",
          borderRadius: 6,
          fontSize: 12,
          fontWeight: 700,
          whiteSpace: "nowrap",
          pointerEvents: "none",
          zIndex: 100,
        }}>
          {value}
        </div>
      )}
      <div style={{
        fontSize: 18,
        fontWeight: 800,
        color: "var(--accent-purple)",
        letterSpacing: "-0.3px",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      }}>
        {value}
      </div>
      <div style={{
        fontSize: 10,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
      }}>
        {label}
      </div>
      {sub && (
        <div style={{
          fontSize: 9,
          color: "var(--text-secondary)",
          marginTop: 2,
        }}>
          {sub}
        </div>
      )}
    </div>
  );
}

function StreakBox({ days, label, start, end }: { days: number; label: string; start: string; end: string }) {
  const t = useI18n();
  return (
    <div style={{
      flex: 1,
      background: "var(--bg-primary)",
      borderRadius: "var(--radius-sm)",
      padding: "8px 10px",
    }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 4 }}>
        <span style={{
          fontSize: 20,
          fontWeight: 800,
          color: "var(--accent-purple)",
        }}>
          {days}
        </span>
        <span style={{
          fontSize: 11,
          fontWeight: 600,
          color: "var(--text-secondary)",
        }}>
          {t("activity.days")}
        </span>
      </div>
      <div style={{
        fontSize: 10,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
      }}>
        {label}
      </div>
      {days > 0 && start && end && (
        <div style={{
          fontSize: 9,
          color: "var(--text-secondary)",
          marginTop: 2,
        }}>
          {shortDate(start)} → {shortDate(end)}
        </div>
      )}
    </div>
  );
}
