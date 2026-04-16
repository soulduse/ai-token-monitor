import type { ProjectUsage } from "../lib/types";
import { formatTokens, formatCost } from "../lib/format";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  data: ProjectUsage[];
}

const MAX_ITEMS = 10;

export function ProjectBreakdown({ data }: Props) {
  const { prefs } = useSettings();
  const t = useI18n();

  const items = data.slice(0, MAX_ITEMS);
  if (items.length === 0) return null;

  const maxCost = items[0]?.cost_usd ?? 0;

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
        {t("analytics.projects.title")}
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {items.map((project) => {
          const barWidth = maxCost > 0 ? (project.cost_usd / maxCost) * 100 : 0;
          return (
            <div key={project.name}>
              <div style={{
                display: "flex",
                justifyContent: "space-between",
                marginBottom: 3,
              }}>
                <span style={{ fontSize: 13, fontWeight: 700, color: "var(--text-primary)" }}>
                  {project.name}
                  <span style={{
                    fontSize: 11,
                    fontWeight: 500,
                    color: "var(--text-secondary)",
                    marginLeft: 4,
                  }}>
                    {project.sessions} {t("analytics.projects.sessions")}
                  </span>
                </span>
                <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}>
                  {formatCost(project.cost_usd)}
                  <span style={{ marginLeft: 6, fontSize: 11 }}>
                    {formatTokens(project.tokens, prefs.number_format)}
                  </span>
                </span>
              </div>
              <div style={{
                height: 7,
                borderRadius: 4,
                background: "var(--heat-0)",
                overflow: "hidden",
              }}>
                <div style={{
                  width: `${barWidth}%`,
                  height: "100%",
                  borderRadius: 4,
                  background: "var(--accent-purple)",
                  transition: "width 0.3s ease",
                }} />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
