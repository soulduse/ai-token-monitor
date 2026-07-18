import type { DateNav } from "../hooks/useDateNav";
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

/** Centered `‹ label ›` row — for period-toggle subtitles and overlays. */
export function DateNavigator({
  nav,
  label,
  tone = "card",
}: {
  nav: DateNav;
  label: string;
  tone?: DateNavTone;
}) {
  const t = useI18n();
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 4 }}>
      <ArrowButton dir="prev" disabled={!nav.canPrev} onClick={nav.goPrev} tone={tone} title={t("dateNav.prevDay")} />
      <span
        onClick={nav.isToday ? undefined : nav.goToday}
        title={nav.isToday ? undefined : t("dateNav.backToToday")}
        style={{
          fontSize: 11,
          fontWeight: 600,
          color: TONE_COLORS[tone].label,
          minWidth: 84,
          textAlign: "center",
          cursor: nav.isToday ? "default" : "pointer",
        }}
      >
        {label}
      </span>
      <ArrowButton dir="next" disabled={!nav.canNext} onClick={nav.goNext} tone={tone} title={t("dateNav.nextDay")} />
    </div>
  );
}
