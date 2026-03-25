import { useState, useCallback } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { invoke } from "@tauri-apps/api/core";
import { SettingsOverlay } from "./SettingsOverlay";
import { WrappedOverlay } from "./wrapped/WrappedOverlay";
import { ReceiptOverlay } from "./receipt/ReceiptOverlay";
import type { AllStats } from "../lib/types";
import type { UpdaterState } from "../hooks/useUpdater";
import { formatTokens, formatCost, getTotalTokens, toLocalDateStr } from "../lib/format";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  stats?: AllStats | null;
  updater?: UpdaterState;
}

export function Header({ stats, updater }: Props) {
  const [showSettings, setShowSettings] = useState(false);
  const [showWrapped, setShowWrapped] = useState(false);
  const [showReceipt, setShowReceipt] = useState(false);
  const [copied, setCopied] = useState(false);
  const [captured, setCaptured] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const t = useI18n();

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2000);
  }, []);

  const handleCapture = useCallback(async () => {
    try {
      await invoke("capture_window");
      setCaptured(true);
      setTimeout(() => setCaptured(false), 2000);
      showToast(t("header.copied"));
    } catch (e) {
      console.error("Capture failed:", e);
      showToast(t("header.captureFailed"));
    }
  }, []);

  const handleExport = useCallback(() => {
    if (!stats) return;

    const todayStr = toLocalDateStr(new Date());
    const today = stats.daily.find((d) => d.date === todayStr);
    const todayTokens = today ? getTotalTokens(today.tokens) : 0;
    const todayCost = today?.cost_usd ?? 0;

    const totalTokens = stats.daily.reduce((sum, d) => sum + getTotalTokens(d.tokens), 0);
    const totalCost = stats.daily.reduce((sum, d) => sum + d.cost_usd, 0);

    const lines = [
      `# ${t("export.title")}`,
      `**${t("export.date")}:** ${todayStr}`,
      ``,
      `## ${t("export.today")}`,
      `- ${t("export.tokens")}: ${formatTokens(todayTokens, "full")}`,
      `- ${t("export.cost")}: ${formatCost(todayCost)}`,
      `- ${t("export.messages")}: ${today?.messages ?? 0}`,
      ``,
      `## ${t("export.allTime")}`,
      `- ${t("export.totalTokens")}: ${formatTokens(totalTokens, "full")}`,
      `- ${t("export.totalCost")}: ${formatCost(totalCost)}`,
      `- ${t("export.totalSessions")}: ${stats.total_sessions}`,
      `- ${t("export.totalMessages")}: ${stats.total_messages}`,
      ``,
      `## ${t("export.models")}`,
      ...Object.entries(stats.model_usage).map(
        ([model, u]) => `- **${model}**: ${formatTokens(u.input_tokens + u.output_tokens + u.cache_read, "full")} tokens, ${formatCost(u.cost_usd)}`
      ),
    ];

    writeText(lines.join("\n")).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
      showToast(t("header.summaryCopied"));
    });
  }, [stats]);

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        paddingBottom: 4,
        position: "relative",
      }}
    >
      <div style={{
        width: 36,
        height: 36,
        borderRadius: "var(--radius-sm)",
        background: "linear-gradient(135deg, var(--accent-purple), var(--accent-pink))",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontSize: 20,
        flexShrink: 0,
      }}>
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <line x1="18" y1="20" x2="18" y2="10"/>
          <line x1="12" y1="20" x2="12" y2="4"/>
          <line x1="6" y1="20" x2="6" y2="14"/>
          <polyline points="4 7 8 3 12 7" stroke="white" strokeWidth="1.5" fill="none"/>
        </svg>
      </div>
      <div style={{ flex: 1 }}>
        <div style={{
          fontSize: 15,
          fontWeight: 800,
          letterSpacing: "-0.3px",
          color: "var(--text-primary)",
        }}>
          {t("app.title")}
        </div>
        {updater?.updateAvailable ? (
          <UpdateIndicator updater={updater} t={t} />
        ) : (
          <div style={{
            fontSize: 11,
            color: "var(--text-secondary)",
            fontWeight: 600,
          }}>
            {t("app.subtitle")}
          </div>
        )}
      </div>

      {/* Wrapped button */}
      <button
        onClick={() => setShowWrapped(true)}
        title={t("wrapped.title")}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12 3l1.5 4.5H18l-3.5 2.5L16 14.5 12 11.5 8 14.5l1.5-4.5L6 7.5h4.5z"/>
          <circle cx="12" cy="12" r="10"/>
        </svg>
      </button>

      {/* Receipt button */}
      <button
        onClick={() => setShowReceipt(true)}
        title={t("receipt.title")}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M4 2v20l3-2 3 2 3-2 3 2 3-2 3 2V2l-3 2-3-2-3 2-3-2-3 2-3-2z"/>
          <line x1="8" y1="8" x2="16" y2="8"/>
          <line x1="8" y1="12" x2="16" y2="12"/>
          <line x1="8" y1="16" x2="12" y2="16"/>
        </svg>
      </button>

      {/* Share button */}
      <button
        onClick={handleExport}
        title={t("header.copySummary")}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: copied ? "var(--accent-mint)" : "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        {copied ? (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12"/>
          </svg>
        ) : (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
          </svg>
        )}
      </button>

      {/* Capture button */}
      <button
        onClick={handleCapture}
        title={t("header.captureScreenshot")}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: captured ? "var(--accent-mint)" : "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        {captured ? (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12"/>
          </svg>
        ) : (
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/>
            <circle cx="12" cy="13" r="4"/>
          </svg>
        )}
      </button>

      {/* Settings button */}
      <button
        onClick={() => setShowSettings(!showSettings)}
        title={t("header.settings")}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: 4,
          borderRadius: 6,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: showSettings ? "var(--accent-purple)" : "var(--text-secondary)",
          transition: "color 0.2s ease",
        }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/>
          <circle cx="12" cy="12" r="3"/>
        </svg>
      </button>

      <SettingsOverlay
        visible={showSettings}
        onClose={() => setShowSettings(false)}
      />

      {stats && (
        <>
          <WrappedOverlay
            visible={showWrapped}
            onClose={() => setShowWrapped(false)}
            stats={stats}
          />
          <ReceiptOverlay
            visible={showReceipt}
            onClose={() => setShowReceipt(false)}
            stats={stats}
          />
        </>
      )}

      {/* Toast */}
      {toast && (
        <div style={{
          position: "absolute",
          top: "100%",
          left: "50%",
          transform: "translateX(-50%)",
          marginTop: 8,
          padding: "6px 14px",
          borderRadius: 8,
          background: "var(--bg-card)",
          color: "var(--text-primary)",
          fontSize: 12,
          fontWeight: 600,
          boxShadow: "0 2px 12px rgba(0,0,0,0.2)",
          whiteSpace: "nowrap",
          zIndex: 100,
          animation: "toast-in 0.2s ease",
          display: "flex",
          alignItems: "center",
          gap: 6,
        }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent-mint)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12"/>
          </svg>
          {toast}
        </div>
      )}
    </div>
  );
}

function UpdateIndicator({ updater, t }: { updater: UpdaterState; t: ReturnType<typeof useI18n> }) {
  const { version, downloading, downloaded, progress, error, download, install } = updater;

  if (error) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11, fontWeight: 600 }}>
        <span style={{ color: "var(--red, #ef4444)" }}>{t("update.error")}</span>
        <button onClick={download} style={indicatorBtnStyle}>{t("update.download")}</button>
      </div>
    );
  }

  if (downloaded) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11, fontWeight: 600 }}>
        <span style={{ color: "var(--accent-mint, #34d399)" }}>{t("update.restart")}</span>
        <button onClick={install} style={indicatorBtnStyle}>{t("update.restartBtn")}</button>
      </div>
    );
  }

  if (downloading) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11, fontWeight: 600 }}>
        <span style={{ color: "var(--text-secondary)" }}>
          {t("update.downloading", { progress: String(progress) })}
        </span>
        <div style={{
          width: 40, height: 3, borderRadius: 2,
          background: "var(--bg-tertiary, rgba(128,128,128,0.2))",
          overflow: "hidden",
        }}>
          <div style={{
            width: `${progress}%`, height: "100%", borderRadius: 2,
            background: "var(--accent, #3b82f6)",
            transition: "width 0.3s ease",
          }} />
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11, fontWeight: 600 }}>
      <span style={{ color: "var(--accent, #3b82f6)" }}>
        {t("update.available", { version })}
      </span>
      <button onClick={download} style={indicatorBtnStyle}>{t("update.download")}</button>
    </div>
  );
}

const indicatorBtnStyle: React.CSSProperties = {
  padding: "2px 8px",
  fontSize: 10,
  fontWeight: 700,
  border: "none",
  borderRadius: 4,
  cursor: "pointer",
  background: "var(--accent, #3b82f6)",
  color: "#fff",
  lineHeight: 1.4,
};

