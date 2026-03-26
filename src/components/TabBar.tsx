import { useI18n } from "../i18n/I18nContext";

export type TabType = "overview" | "analytics" | "leaderboard" | "chat";

interface Props {
  activeTab: TabType;
  onChange: (tab: TabType) => void;
}

export function TabBar({ activeTab, onChange }: Props) {
  const t = useI18n();

  return (
    <div style={{
      display: "flex",
      background: "var(--heat-0)",
      borderRadius: "var(--radius-sm)",
      padding: 3,
      gap: 2,
    }}>
      <TabButton
        label={t("tab.overview")}
        active={activeTab === "overview"}
        onClick={() => onChange("overview")}
      />
      <TabButton
        label={t("tab.analytics")}
        active={activeTab === "analytics"}
        onClick={() => onChange("analytics")}
      />
      <TabButton
        label={t("tab.leaderboard")}
        active={activeTab === "leaderboard"}
        onClick={() => onChange("leaderboard")}
        icon="🏆"
      />
      <TabButton
        label={t("tab.chat")}
        active={activeTab === "chat"}
        onClick={() => onChange("chat")}
        icon="💬"
      />
    </div>
  );
}

function TabButton({
  label,
  active,
  onClick,
  icon,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
  icon?: string;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        flex: 1,
        padding: "6px 8px",
        fontSize: 11,
        fontWeight: 700,
        border: "none",
        borderRadius: 6,
        cursor: "pointer",
        background: active ? "var(--bg-card)" : "transparent",
        color: active ? "var(--accent-purple)" : "var(--text-secondary)",
        boxShadow: active ? "0 1px 4px rgba(0,0,0,0.08)" : "none",
        transition: "all 0.15s ease",
      }}
    >
      {icon && <span style={{ marginRight: 3 }}>{icon}</span>}
      {label}
    </button>
  );
}
