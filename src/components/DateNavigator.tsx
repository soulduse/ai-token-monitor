import { useRef, useState } from "react";
import type { DateNav } from "../hooks/useDateNav";
import { DateNavCalendar } from "./DateNavCalendar";
import { useI18n } from "../i18n/I18nContext";

export type DateNavTone = "card" | "overlay";

const TONE_COLORS: Record<DateNavTone, { arrow: string; label: string }> = {
  card: { arrow: "var(--text-secondary)", label: "var(--text-secondary)" },
  overlay: { arrow: "rgba(255,255,255,0.75)", label: "rgba(255,255,255,0.9)" },
};

function ArrowButton({
  dir,
  disabled,
  onClick,
  tone,
  title,
}: {
  dir: "prev" | "next";
  disabled: boolean;
  onClick: () => void;
  tone: DateNavTone;
  title: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      aria-label={title}
      title={title}
      style={{
        fontSize: 13,
        fontWeight: 600,
        lineHeight: 1,
        border: "none",
        borderRadius: 4,
        background: "transparent",
        color: TONE_COLORS[tone].arrow,
        padding: "3px 7px",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.3 : 1,
        transition: "all 0.15s ease",
      }}
    >
      {dir === "prev" ? "‹" : "›"}
    </button>
  );
}

/** Bare ‹ › arrow pair — for embedding into an existing header row. */
export function DateNavArrows({ nav, tone = "card" }: { nav: DateNav; tone?: DateNavTone }) {
  const t = useI18n();
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 2 }}>
      <ArrowButton dir="prev" disabled={!nav.canPrev} onClick={nav.goPrev} tone={tone} title={t("dateNav.prevDay")} />
      <ArrowButton dir="next" disabled={!nav.canNext} onClick={nav.goNext} tone={tone} title={t("dateNav.nextDay")} />
    </div>
  );
}

/**
 * Centered `‹ label ›` row. When `dailyTokens` is provided, clicking the
 * label opens a calendar picker showing per-day token usage.
 */
export function DateNavigator({
  nav,
  label,
  tone = "card",
  dailyTokens,
}: {
  nav: DateNav;
  label: string;
  tone?: DateNavTone;
  dailyTokens?: Record<string, number>;
}) {
  const t = useI18n();
  const [calendarOpen, setCalendarOpen] = useState(false);
  const anchorRef = useRef<HTMLButtonElement>(null);
  // No selectable range without data — don't offer a fully-disabled picker.
  const pickable = !!dailyTokens && nav.minDate !== null;

  return (
    <div style={{ position: "relative", display: "flex", alignItems: "center", justifyContent: "center", gap: 4 }}>
      <ArrowButton dir="prev" disabled={!nav.canPrev} onClick={nav.goPrev} tone={tone} title={t("dateNav.prevDay")} />
      <button
        ref={anchorRef}
        onClick={pickable ? () => setCalendarOpen((v) => !v) : undefined}
        disabled={!pickable}
        title={pickable ? t("dateNav.openCalendar") : undefined}
        aria-label={pickable ? t("dateNav.openCalendar") : undefined}
        aria-haspopup={pickable ? "dialog" : undefined}
        aria-expanded={pickable ? calendarOpen : undefined}
        style={{
          fontSize: 11,
          fontWeight: 600,
          border: "none",
          background: "transparent",
          padding: 0,
          color: TONE_COLORS[tone].label,
          minWidth: 84,
          cursor: pickable ? "pointer" : "default",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 4,
        }}
      >
        {label}
        {pickable && (
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.6 }}>
            <polyline points="6 9 12 15 18 9" />
          </svg>
        )}
      </button>
      <ArrowButton dir="next" disabled={!nav.canNext} onClick={nav.goNext} tone={tone} title={t("dateNav.nextDay")} />

      {calendarOpen && dailyTokens && (
        <DateNavCalendar
          nav={nav}
          dailyTokens={dailyTokens}
          align="center"
          anchorRef={anchorRef}
          onClose={() => setCalendarOpen(false)}
        />
      )}
    </div>
  );
}
