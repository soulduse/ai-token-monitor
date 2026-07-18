import { useCallback, useState } from "react";
import { toLocalDateStr } from "../lib/format";

export interface DateNav {
  /** Selected local date (YYYY-MM-DD); equals `todayStr` while following today. */
  date: string;
  /** The actual local today (upper navigation bound). */
  today: string;
  /** Earliest date with data (lower navigation bound); null when no data. */
  minDate: string | null;
  isToday: boolean;
  canPrev: boolean;
  canNext: boolean;
  goPrev: () => void;
  goNext: () => void;
  goToday: () => void;
  /** Jump to an arbitrary date, clamped to [minDate, today]. */
  goTo: (date: string) => void;
}

function shiftDate(dateStr: string, days: number): string {
  const d = new Date(dateStr + "T00:00:00");
  d.setDate(d.getDate() + days);
  return toLocalDateStr(d);
}

/**
 * Day-by-day date navigation clamped to [minDate, today].
 *
 * `null` selection means "following today", so a midnight rollover moves the
 * view to the new day automatically; an explicitly pinned past date stays put.
 * The derived date is re-clamped on every render, so a selection that falls
 * outside the range after the bounds move (e.g. a provider toggle shrinking
 * the data range) snaps back inside it. Future days are unreachable.
 */
export function useDateNav(todayStr: string, minDate: string | null): DateNav {
  const [selected, setSelected] = useState<string | null>(null);

  // YYYY-MM-DD strings compare correctly lexicographically.
  const clamp = useCallback(
    (d: string): string => {
      if (minDate === null) return todayStr;
      if (d >= todayStr) return todayStr;
      return d < minDate ? minDate : d;
    },
    [todayStr, minDate],
  );

  const date = selected === null ? todayStr : clamp(selected);
  const isToday = date === todayStr;
  const canPrev = minDate !== null && date > minDate;
  const canNext = !isToday;

  const goPrev = useCallback(() => {
    if (minDate === null) return;
    setSelected((prev) => {
      const current = prev === null ? todayStr : clamp(prev);
      if (current <= minDate) return prev;
      return clamp(shiftDate(current, -1));
    });
  }, [minDate, todayStr, clamp]);

  const goNext = useCallback(() => {
    setSelected((prev) => {
      if (prev === null) return null;
      const next = shiftDate(clamp(prev), 1);
      return next >= todayStr ? null : next;
    });
  }, [todayStr, clamp]);

  const goToday = useCallback(() => setSelected(null), []);

  const goTo = useCallback(
    (target: string) => {
      const next = clamp(target);
      setSelected(next === todayStr ? null : next);
    },
    [clamp, todayStr],
  );

  return {
    date,
    today: todayStr,
    minDate,
    isToday,
    canPrev,
    canNext,
    goPrev,
    goNext,
    goToday,
    goTo,
  };
}
