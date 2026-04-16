import type { ToolCount } from "../lib/types";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  data: ToolCount[];
}

const AGENT_TOOLS = new Set(["Agent", "TaskCreate", "TaskUpdate", "TaskGet", "TaskList", "TaskOutput", "TaskStop"]);

function getToolColor(name: string): string {
  if (AGENT_TOOLS.has(name)) return "var(--accent-pink)";
  return "var(--accent-purple)";
}

export function ToolUsage({ data }: Props) {
  const t = useI18n();
  if (data.length === 0) return null;

  const maxCount = data[0]?.count ?? 0;

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
        {t("analytics.tools.title")}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {data.map((tool) => {
          const barWidth = maxCount > 0 ? (tool.count / maxCount) * 100 : 0;
          return (
            <div key={tool.name} style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{
                fontSize: 13,
                fontWeight: 600,
                color: "var(--text-primary)",
                width: 100,
                flexShrink: 0,
                textAlign: "right",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}>
                {tool.name}
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
                  background: getToolColor(tool.name),
                  transition: "width 0.3s ease",
                }} />
              </div>
              <span style={{
                fontSize: 12,
                fontWeight: 700,
                color: "var(--text-secondary)",
                width: 48,
                textAlign: "right",
                flexShrink: 0,
              }}>
                {tool.count.toLocaleString()}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
