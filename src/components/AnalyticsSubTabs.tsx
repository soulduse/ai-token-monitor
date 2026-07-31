import { useI18n } from "../i18n/I18nContext";

export type AnalyticsSubTab = "usage" | "projects" | "tools" | "kiro";

interface Props {
  active: AnalyticsSubTab;
  onChange: (tab: AnalyticsSubTab) => void;
  /** Kiro meters in credits, not tokens, so it gets its own tab — shown only
   *  when the source is enabled. */
  showKiro?: boolean;
}

const BASE_TABS: AnalyticsSubTab[] = ["usage", "projects", "tools"];

export function AnalyticsSubTabs({ active, onChange, showKiro = false }: Props) {
  const t = useI18n();
  const TABS = showKiro ? [...BASE_TABS, "kiro" as const] : BASE_TABS;

  return (
    <div style={{
      display: "flex",
      gap: 4,
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 4,
      boxShadow: "var(--shadow-card)",
    }}>
      {TABS.map((tab) => (
        <button
          key={tab}
          onClick={() => onChange(tab)}
          style={{
            flex: 1,
            padding: "6px 0",
            border: "none",
            borderRadius: "var(--radius-md, 8px)",
            background: active === tab ? "var(--accent-purple)" : "transparent",
            color: active === tab ? "#fff" : "var(--text-secondary)",
            fontSize: 12,
            fontWeight: 700,
            cursor: "pointer",
            transition: "all 0.15s ease",
            letterSpacing: "0.3px",
          }}
        >
          {t(`analytics.subtab.${tab}`)}
        </button>
      ))}
    </div>
  );
}
