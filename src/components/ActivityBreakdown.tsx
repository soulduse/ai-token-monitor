import type { ActivityCategory } from "../lib/types";
import { formatCost } from "../lib/format";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  data: ActivityCategory[];
}

const CATEGORY_COLORS: Record<string, string> = {
  "Coding": "#5B9EF5",
  "Exploration": "#5BF5E0",
  "Debugging": "#F55B5B",
  "Feature Dev": "#5BF58C",
  "Delegation": "#F5A05B",
  "Conversation": "#888888",
  "Testing": "#E05BF5",
  "Brainstorming": "#F5E05B",
  "Refactoring": "#F5E05B",
  "Build/Deploy": "#5BF5A0",
  "Git Ops": "#CCCCCC",
  "Planning": "#7B9EF5",
  "General": "#AAAAAA",
};

function getCategoryColor(category: string): string {
  return CATEGORY_COLORS[category] ?? "var(--accent-purple)";
}

export function ActivityBreakdown({ data }: Props) {
  const t = useI18n();
  if (data.length === 0) return null;

  const maxCost = data[0]?.cost_usd ?? 0;

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 16,
      boxShadow: "var(--shadow-card)",
    }}>
      <div style={{
        fontSize: 12,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
        marginBottom: 10,
      }}>
        {t("analytics.activity.title")}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {data.map((item) => {
          const barWidth = maxCost > 0 ? (item.cost_usd / maxCost) * 100 : 0;
          const color = getCategoryColor(item.category);
          return (
            <div key={item.category} style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span style={{
                fontSize: 13,
                fontWeight: 600,
                color,
                width: 86,
                flexShrink: 0,
                textAlign: "right",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}>
                {t(`analytics.activity.${item.category}`) || item.category}
              </span>
              <div style={{
                flex: 1,
                height: 7,
                borderRadius: 4,
                background: "var(--heat-0)",
                overflow: "hidden",
              }}>
                <div style={{
                  width: `${barWidth}%`,
                  height: "100%",
                  borderRadius: 4,
                  background: color,
                  transition: "width 0.3s ease",
                }} />
              </div>
              <span style={{
                fontSize: 12,
                fontWeight: 700,
                color: "var(--text-secondary)",
                width: 50,
                textAlign: "right",
                flexShrink: 0,
              }}>
                {formatCost(item.cost_usd)}
              </span>
              <span style={{
                fontSize: 11,
                fontWeight: 600,
                color: "var(--text-secondary)",
                width: 40,
                textAlign: "right",
                flexShrink: 0,
              }}>
                {item.messages.toLocaleString()}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
