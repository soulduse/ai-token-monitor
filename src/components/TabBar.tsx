import { useI18n } from "../i18n/I18nContext";

export type TabType = "overview" | "analytics" | "leaderboard" | "chat";

interface Props {
  activeTab: TabType;
  onChange: (tab: TabType) => void;
  chatBadge?: number;
}

export function TabBar({ activeTab, onChange, chatBadge }: Props) {
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
        dataTour="tab-analytics"
      />
      <TabButton
        label={t("tab.leaderboard")}
        active={activeTab === "leaderboard"}
        onClick={() => onChange("leaderboard")}
        icon="🏆"
        dataTour="tab-leaderboard"
      />
      <TabButton
        label={t("tab.chat")}
        active={activeTab === "chat"}
        onClick={() => onChange("chat")}
        icon="💬"
        badge={chatBadge}
        dataTour="tab-chat"
      />
    </div>
  );
}

function TabButton({
  label,
  active,
  onClick,
  icon,
  badge,
  dataTour,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
  icon?: string;
  badge?: number;
  dataTour?: string;
}) {
  return (
    <button
      onClick={onClick}
      data-tour={dataTour}
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
        position: "relative",
      }}
    >
      {icon && <span style={{ marginRight: 3 }}>{icon}</span>}
      {label}
      {badge != null && badge > 0 && (
        <span style={{
          position: "absolute",
          top: 2,
          right: 4,
          minWidth: 16,
          height: 16,
          borderRadius: 8,
          background: "var(--accent-pink, #e74c8a)",
          color: "#fff",
          fontSize: 9,
          fontWeight: 800,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          padding: "0 4px",
          lineHeight: 1,
        }}>
          {badge > 99 ? "99+" : badge}
        </span>
      )}
    </button>
  );
}
