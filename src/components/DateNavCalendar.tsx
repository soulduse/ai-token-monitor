import { useEffect, useMemo, useRef, useState } from "react";
import type { DateNav } from "../hooks/useDateNav";
import { formatTokens, toLocalDateStr } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

const HEAT_COLORS = [
  "var(--heat-0)",
  "var(--heat-1)",
  "var(--heat-2)",
  "var(--heat-3)",
  "var(--heat-4)",
];

function getHeatLevel(value: number, thresholds: number[]): number {
  if (value <= 0) return 0;
  for (let i = thresholds.length - 1; i >= 0; i--) {
    if (value >= thresholds[i]) return i + 1;
  }
  return 1;
}

function monthKey(dateStr: string): string {
  return dateStr.slice(0, 7); // YYYY-MM
}

function shiftMonth(ym: string, delta: number): string {
  const [y, m] = ym.split("-").map(Number);
  const d = new Date(y, m - 1 + delta, 1);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}

interface Props {
  nav: DateNav;
  /** Local date (YYYY-MM-DD) → total tokens for that day. */
  dailyTokens: Record<string, number>;
  /** Horizontal anchor relative to the positioned parent. */
  align?: "left" | "center";
  /** The trigger element — clicks inside it are not "outside" (lets the
   *  trigger's own click handler toggle the calendar closed). */
  anchorRef?: React.RefObject<HTMLElement | null>;
  onClose: () => void;
}

/**
 * Month-grid date picker where every day cell shows that day's token usage
 * (heat-colored like the activity heatmap + compact number), so a date can be
 * chosen by how much was actually used. Selection is clamped to
 * [nav.minDate, nav.today] — future days and days before the data range are
 * disabled.
 */
export function DateNavCalendar({ nav, dailyTokens, align = "center", anchorRef, onClose }: Props) {
  const t = useI18n();
  const { prefs } = useSettings();
  const containerRef = useRef<HTMLDivElement>(null);
  const [month, setMonth] = useState(() => monthKey(nav.date));

  // Close on outside click / Escape. Capture phase + preventDefault so the
  // module-level Escape→hide_window handler (main.tsx) stays inert while the
  // calendar is open.
  useEffect(() => {
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (containerRef.current?.contains(target)) return;
      if (anchorRef?.current?.contains(target)) return;
      onClose();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    document.addEventListener("mousedown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [onClose, anchorRef]);

  // Quantile thresholds over all recorded days — same scheme as the heatmap.
  const thresholds = useMemo(() => {
    const values = Object.values(dailyTokens).filter((v) => v > 0).sort((a, b) => a - b);
    if (values.length === 0) return [0, 0, 0, 0];
    return [0.25, 0.5, 0.75, 0.9].map(
      (q) => values[Math.min(values.length - 1, Math.floor(q * values.length))],
    );
  }, [dailyTokens]);

  const minMonth = nav.minDate ? monthKey(nav.minDate) : monthKey(nav.today);
  const maxMonth = monthKey(nav.today);
  const canPrevMonth = month > minMonth;
  const canNextMonth = month < maxMonth;

  const { cells, weekdayLabels, monthLabel } = useMemo(() => {
    const [y, m] = month.split("-").map(Number);
    const first = new Date(y, m - 1, 1);
    const daysInMonth = new Date(y, m, 0).getDate();
    // Monday-first grid, matching the app's week convention.
    const leadingBlanks = (first.getDay() + 6) % 7;

    const cells: ({ date: string; day: number } | null)[] = [];
    for (let i = 0; i < leadingBlanks; i++) cells.push(null);
    for (let day = 1; day <= daysInMonth; day++) {
      cells.push({ date: toLocalDateStr(new Date(y, m - 1, day)), day });
    }

    const locale = prefs.language === "zh-CN" ? "zh-Hans" : prefs.language === "zh-TW" ? "zh-Hant" : prefs.language;
    // 2026-07-06 is a Monday.
    const weekdayLabels = Array.from({ length: 7 }, (_, i) =>
      new Date(2026, 6, 6 + i).toLocaleDateString(locale, { weekday: "narrow" }),
    );
    const monthLabel = first.toLocaleDateString(locale, { month: "short", year: "numeric" });

    return { cells, weekdayLabels, monthLabel };
  }, [month, prefs.language]);

  const navBtn: React.CSSProperties = {
    fontSize: 13,
    fontWeight: 600,
    lineHeight: 1,
    border: "none",
    borderRadius: 4,
    background: "transparent",
    color: "var(--text-secondary)",
    padding: "3px 7px",
    cursor: "pointer",
    transition: "all 0.15s ease",
  };

  return (
    <div
      ref={containerRef}
      style={{
        position: "absolute",
        top: "100%",
        marginTop: 6,
        ...(align === "center"
          ? { left: "50%", transform: "translateX(-50%)" }
          : { left: 0 }),
        zIndex: 70,
        width: 252,
        background: "var(--bg-card)",
        border: "1px solid rgba(124, 92, 252, 0.15)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "0 8px 28px rgba(0,0,0,0.45)",
        padding: 10,
      }}
    >
      {/* Month navigation */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 6 }}>
        <button
          onClick={() => canPrevMonth && setMonth(shiftMonth(month, -1))}
          disabled={!canPrevMonth}
          aria-label={t("dateNav.prevMonth")}
          style={{ ...navBtn, opacity: canPrevMonth ? 1 : 0.3, cursor: canPrevMonth ? "pointer" : "default" }}
        >
          ‹
        </button>
        <span style={{ fontSize: 12, fontWeight: 700, color: "var(--text-primary)" }}>
          {monthLabel}
        </span>
        <button
          onClick={() => canNextMonth && setMonth(shiftMonth(month, 1))}
          disabled={!canNextMonth}
          aria-label={t("dateNav.nextMonth")}
          style={{ ...navBtn, opacity: canNextMonth ? 1 : 0.3, cursor: canNextMonth ? "pointer" : "default" }}
        >
          ›
        </button>
      </div>

      {/* Weekday header */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(7, 1fr)", gap: 3, marginBottom: 3 }}>
        {weekdayLabels.map((w, i) => (
          <div key={i} style={{ textAlign: "center", fontSize: 8, fontWeight: 700, color: "var(--text-secondary)", opacity: 0.7 }}>
            {w}
          </div>
        ))}
      </div>

      {/* Day grid */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(7, 1fr)", gap: 3 }}>
        {cells.map((cell, i) => {
          if (!cell) return <div key={`b-${i}`} />;
          const tokens = dailyTokens[cell.date] ?? 0;
          const inRange =
            cell.date <= nav.today && nav.minDate !== null && cell.date >= nav.minDate;
          const isSelected = cell.date === nav.date;
          const isTodayCell = cell.date === nav.today;
          const level = getHeatLevel(tokens, thresholds);
          return (
            <button
              key={cell.date}
              onClick={() => {
                if (!inRange) return;
                nav.goTo(cell.date);
                onClose();
              }}
              disabled={!inRange}
              title={inRange ? `${cell.date} · ${formatTokens(tokens, prefs.number_format)}` : undefined}
              style={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 1,
                height: 30,
                border: "none",
                borderRadius: 5,
                padding: 0,
                background: inRange ? HEAT_COLORS[level] : "transparent",
                cursor: inRange ? "pointer" : "default",
                opacity: inRange ? 1 : 0.25,
                outline: isSelected ? "2px solid var(--accent-purple)" : "none",
                outlineOffset: -1,
                transition: "transform 0.1s ease",
              }}
            >
              <span style={{
                fontSize: 9,
                fontWeight: 700,
                lineHeight: 1,
                color: isTodayCell ? "var(--accent-purple)" : level >= 3 ? "#fff" : "var(--text-primary)",
              }}>
                {cell.day}
              </span>
              <span style={{
                fontSize: 7,
                fontWeight: 600,
                lineHeight: 1,
                color: level >= 3 ? "rgba(255,255,255,0.85)" : "var(--text-secondary)",
              }}>
                {tokens > 0 ? formatTokens(tokens, "compact") : ""}
              </span>
            </button>
          );
        })}
      </div>

      {/* Footer: back to today — also shown when browsing a non-current month */}
      {(!nav.isToday || month !== maxMonth) && (
        <div style={{ display: "flex", justifyContent: "center", marginTop: 8 }}>
          <button
            onClick={() => { nav.goToday(); onClose(); }}
            style={{
              fontSize: 10,
              fontWeight: 700,
              padding: "4px 14px",
              borderRadius: 6,
              border: "none",
              cursor: "pointer",
              background: "var(--accent-purple)",
              color: "#fff",
              transition: "all 0.15s ease",
            }}
          >
            {t("dateNav.today")}
          </button>
        </div>
      )}
    </div>
  );
}
