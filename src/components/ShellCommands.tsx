import type { ToolCount, McpServerUsage } from "../lib/types";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  commands: ToolCount[];
  mcp: McpServerUsage[];
}

const MAX_COMMANDS = 15;

export function ShellCommands({ commands, mcp }: Props) {
  const t = useI18n();
  const items = commands.slice(0, MAX_COMMANDS);
  const hasData = items.length > 0 || mcp.length > 0;

  if (!hasData) return null;

  const maxCount = items[0]?.count ?? 0;
  const maxMcpCount = mcp[0]?.calls ?? 0;

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 16,
      boxShadow: "var(--shadow-card)",
      display: "flex",
      flexDirection: "column",
      gap: 16,
    }}>
      {items.length > 0 && (
        <div>
          <div style={{
            fontSize: 12,
            fontWeight: 700,
            color: "var(--text-secondary)",
            textTransform: "uppercase",
            letterSpacing: "0.5px",
            marginBottom: 10,
          }}>
            {t("analytics.shell.title")}
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            {items.map((cmd) => {
              const barWidth = maxCount > 0 ? (cmd.count / maxCount) * 100 : 0;
              return (
                <div key={cmd.name} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <span style={{
                    fontSize: 13,
                    fontWeight: 600,
                    color: "var(--accent-mint)",
                    width: 80,
                    flexShrink: 0,
                    textAlign: "right",
                    fontFamily: "monospace",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}>
                    {cmd.name}
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
                      background: "var(--accent-mint)",
                      transition: "width 0.3s ease",
                    }} />
                  </div>
                  <span style={{
                    fontSize: 12,
                    fontWeight: 700,
                    color: "var(--text-secondary)",
                    width: 40,
                    textAlign: "right",
                    flexShrink: 0,
                  }}>
                    {cmd.count.toLocaleString()}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {mcp.length > 0 && (
        <div>
          <div style={{
            fontSize: 12,
            fontWeight: 700,
            color: "var(--text-secondary)",
            textTransform: "uppercase",
            letterSpacing: "0.5px",
            marginBottom: 10,
          }}>
            {t("analytics.mcp.title")}
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            {mcp.map((server) => {
              const barWidth = maxMcpCount > 0 ? (server.calls / maxMcpCount) * 100 : 0;
              return (
                <div key={server.server} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <span style={{
                    fontSize: 13,
                    fontWeight: 600,
                    color: "var(--accent-orange, var(--accent-pink))",
                    width: 100,
                    flexShrink: 0,
                    textAlign: "right",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}>
                    {server.server}
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
                      background: "var(--accent-orange, var(--accent-pink))",
                      transition: "width 0.3s ease",
                    }} />
                  </div>
                  <span style={{
                    fontSize: 12,
                    fontWeight: 700,
                    color: "var(--text-secondary)",
                    width: 40,
                    textAlign: "right",
                    flexShrink: 0,
                  }}>
                    {server.calls.toLocaleString()}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
