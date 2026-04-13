import { useState, useCallback, useEffect, useRef } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { SettingsOverlay } from "./SettingsOverlay";
import { WrappedOverlay } from "./wrapped/WrappedOverlay";
import { ReceiptOverlay } from "./receipt/ReceiptOverlay";
import type { AllStats } from "../lib/types";
import type { UpdaterState } from "../hooks/useUpdater";
import { formatTokens, formatCost, getTotalTokens, toLocalDateStr } from "../lib/format";
import { useI18n } from "../i18n/I18nContext";
import { useSettings } from "../contexts/SettingsContext";

interface Props {
  stats?: AllStats | null;
  updater?: UpdaterState;
}

export function Header({ stats, updater }: Props) {
  const [showSettings, setShowSettings] = useState(false);
  const [showWrapped, setShowWrapped] = useState(false);
  const [showReceipt, setShowReceipt] = useState(false);
  const [showMenu, setShowMenu] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const t = useI18n();
  const { prefs, updatePrefs } = useSettings();

  const toggleQuickAction = useCallback((key: string) => {
    const items = prefs.quick_action_items ?? [];
    const next = items.includes(key)
      ? items.filter((k) => k !== key)
      : [...items, key];
    updatePrefs({ quick_action_items: next });
  }, [prefs.quick_action_items, updatePrefs]);

  // Outside click + ESC to close the actions menu
  useEffect(() => {
    if (!showMenu) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setShowMenu(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Capture phase + stopPropagation prevents AppContent from hiding the window
        e.preventDefault();
        e.stopPropagation();
        setShowMenu(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [showMenu]);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2000);
  }, []);

  const handleCapture = useCallback(async () => {
    try {
      await invoke("capture_window");
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
      showToast(t("header.summaryCopied"));
    });
  }, [stats]);

  const handleShareApp = useCallback(() => {
    writeText(t("share.appMessage")).then(() => {
      showToast(t("share.copied"));
    });
  }, [t, showToast]);

  const menuItems: {
    key: string;
    label: string;
    icon: React.ReactNode;
    onClick: () => void;
  }[] = [
    {
      key: "github",
      label: t("header.github"),
      onClick: () => openUrl("https://github.com/soulduse/ai-token-monitor"),
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <path d="M12 .5C5.73.5.5 5.73.5 12a11.5 11.5 0 0 0 7.86 10.92c.58.11.79-.25.79-.56 0-.28-.01-1.02-.02-2-3.2.7-3.88-1.54-3.88-1.54-.52-1.33-1.28-1.68-1.28-1.68-1.05-.72.08-.71.08-.71 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.56-.29-5.25-1.28-5.25-5.7 0-1.26.45-2.29 1.19-3.1-.12-.29-.52-1.47.11-3.06 0 0 .97-.31 3.18 1.18a11 11 0 0 1 5.79 0c2.21-1.49 3.18-1.18 3.18-1.18.63 1.59.23 2.77.11 3.06.74.81 1.19 1.84 1.19 3.1 0 4.43-2.69 5.41-5.26 5.69.41.36.78 1.06.78 2.13 0 1.54-.01 2.78-.01 3.16 0 .31.21.68.8.56A11.5 11.5 0 0 0 23.5 12C23.5 5.73 18.27.5 12 .5z"/>
        </svg>
      ),
    },
    {
      key: "wrapped",
      label: t("wrapped.title"),
      onClick: () => setShowWrapped(true),
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12 3l1.5 4.5H18l-3.5 2.5L16 14.5 12 11.5 8 14.5l1.5-4.5L6 7.5h4.5z"/>
          <circle cx="12" cy="12" r="10"/>
        </svg>
      ),
    },
    {
      key: "receipt",
      label: t("receipt.title"),
      onClick: () => setShowReceipt(true),
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M4 2v20l3-2 3 2 3-2 3 2 3-2 3 2V2l-3 2-3-2-3 2-3-2-3 2-3-2z"/>
          <line x1="8" y1="8" x2="16" y2="8"/>
          <line x1="8" y1="12" x2="16" y2="12"/>
          <line x1="8" y1="16" x2="12" y2="16"/>
        </svg>
      ),
    },
    {
      key: "share",
      label: t("header.copySummary"),
      onClick: handleExport,
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
        </svg>
      ),
    },
    {
      key: "capture",
      label: t("header.captureScreenshot"),
      onClick: handleCapture,
      icon: (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/>
          <circle cx="12" cy="13" r="4"/>
        </svg>
      ),
    },
  ];

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
      {/* Draggable title area */}
      <div
        onMouseDown={(e) => {
          if (e.button === 0) {
            e.preventDefault();
            getCurrentWindow().startDragging();
          }
        }}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          flex: 1,
          cursor: "grab",
          userSelect: "none",
        } as React.CSSProperties}
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
          pointerEvents: "none",
        }}>
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="18" y1="20" x2="18" y2="10"/>
            <line x1="12" y1="20" x2="12" y2="4"/>
            <line x1="6" y1="20" x2="6" y2="14"/>
            <polyline points="4 7 8 3 12 7" stroke="white" strokeWidth="1.5" fill="none"/>
          </svg>
        </div>
        <div style={{ pointerEvents: "none" }}>
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
      </div>

      {/* Quick action buttons (pinned items) */}
      {(prefs.quick_action_items ?? []).length > 0 && menuItems
        .filter((item) => (prefs.quick_action_items ?? []).includes(item.key))
        .map((item) => (
          <button
            key={item.key}
            onClick={item.onClick}
            title={item.label}
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
            {item.icon}
          </button>
        ))}

      {/* Share app button */}
      <button
        onClick={handleShareApp}
        title={t("header.share")}
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
          <circle cx="18" cy="5" r="3"/>
          <circle cx="6" cy="12" r="3"/>
          <circle cx="18" cy="19" r="3"/>
          <line x1="8.59" y1="13.51" x2="15.42" y2="17.49"/>
          <line x1="15.41" y1="6.51" x2="8.59" y2="10.49"/>
        </svg>
      </button>

      {/* Actions menu (collapsed dropdown) */}
      <div ref={menuRef} style={{ position: "relative" }}>
        <button
          onClick={() => setShowMenu((v) => !v)}
          title={t("header.menu")}
          aria-haspopup="menu"
          aria-expanded={showMenu}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            padding: 4,
            borderRadius: 6,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: showMenu ? "var(--accent-purple)" : "var(--text-secondary)",
            transition: "color 0.2s ease",
          }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <circle cx="5" cy="12" r="1.7"/>
            <circle cx="12" cy="12" r="1.7"/>
            <circle cx="19" cy="12" r="1.7"/>
          </svg>
        </button>

        {showMenu && (
          <div
            role="menu"
            style={{
              position: "absolute",
              top: "calc(100% + 8px)",
              right: 0,
              minWidth: 220,
              padding: 6,
              background: "var(--bg-card)",
              borderRadius: 12,
              border: "1px solid rgba(128,128,128,0.15)",
              boxShadow: "0 12px 32px rgba(0,0,0,0.18), 0 2px 8px rgba(0,0,0,0.08)",
              zIndex: 60,
              transformOrigin: "top right",
              animation: "headerMenuPop 0.16s cubic-bezier(.2,.9,.2,1) both",
            }}
          >
            {menuItems.map((item, i) => {
              const pinned = (prefs.quick_action_items ?? []).includes(item.key);
              return (
                <div
                  key={item.key}
                  className="header-action-menu-item"
                  style={{
                    animationDelay: `${40 + 35 * i}ms`,
                    justifyContent: "space-between",
                  }}
                >
                  <button
                    role="menuitem"
                    onClick={() => {
                      setShowMenu(false);
                      item.onClick();
                    }}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 10,
                      background: "none",
                      border: "none",
                      padding: 0,
                      cursor: "pointer",
                      color: "inherit",
                      font: "inherit",
                      flex: 1,
                      minWidth: 0,
                    }}
                  >
                    <span style={{ display: "flex", color: "var(--text-secondary)" }}>{item.icon}</span>
                    <span>{item.label}</span>
                  </button>
                  <button
                    aria-label={t("header.quickActions")}
                    onClick={(e) => {
                      e.stopPropagation();
                      toggleQuickAction(item.key);
                    }}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      background: "none",
                      border: "none",
                      padding: 2,
                      cursor: "pointer",
                      borderRadius: 4,
                      color: pinned ? "var(--accent-purple)" : "rgba(128,128,128,0.3)",
                      transition: "color 0.2s ease",
                      flexShrink: 0,
                    }}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill={pinned ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/>
                    </svg>
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>

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

const indicatorWrapStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  fontSize: 11,
  fontWeight: 600,
  pointerEvents: "auto",
  cursor: "default",
};

function UpdateIndicator({ updater, t }: { updater: UpdaterState; t: ReturnType<typeof useI18n> }) {
  const { version, downloading, downloaded, progress, error, restartFailed, download, install } = updater;

  const stopDrag = (e: React.MouseEvent) => e.stopPropagation();

  if (error) {
    return (
      <div style={indicatorWrapStyle} onMouseDown={stopDrag}>
        <span title={error} style={{ color: "var(--red, #ef4444)", cursor: "help" }}>{t("update.error")}</span>
        <button onClick={download} style={indicatorBtnStyle}>{t("update.download")}</button>
      </div>
    );
  }

  if (downloaded) {
    return (
      <div style={indicatorWrapStyle} onMouseDown={stopDrag}>
        <span style={{ color: restartFailed ? "var(--red, #ef4444)" : "var(--accent-mint, #34d399)" }}>
          {restartFailed ? t("update.restartManual") : t("update.restart")}
        </span>
        <button onClick={install} style={indicatorBtnStyle}>{t("update.restartBtn")}</button>
      </div>
    );
  }

  if (downloading) {
    return (
      <div style={indicatorWrapStyle} onMouseDown={stopDrag}>
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
    <div style={indicatorWrapStyle} onMouseDown={stopDrag}>
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

