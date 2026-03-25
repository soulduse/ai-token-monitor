import { useI18n } from "../i18n/I18nContext";
import type { UpdaterState } from "../hooks/useUpdater";

interface Props {
  updater: UpdaterState;
}

export function UpdateBanner({ updater }: Props) {
  const t = useI18n();
  const { updateAvailable, version, downloading, downloaded, progress, error, download, install, dismiss } = updater;

  if (!updateAvailable) return null;

  return (
    <div style={{
      width: "100%",
      marginTop: 4,
      padding: "8px 12px",
      fontSize: 12,
      fontWeight: 600,
      borderRadius: "var(--radius-md)",
      background: "var(--bg-card)",
      color: "var(--text-secondary)",
      boxShadow: "var(--shadow-card)",
      display: "flex",
      alignItems: "center",
      gap: 8,
    }}>
      {error ? (
        <>
          <span style={{ color: "var(--red, #ef4444)", flex: 1 }}>
            {t("update.error")}
          </span>
          <button onClick={dismiss} style={closeBtnStyle}>
            <CloseIcon />
          </button>
        </>
      ) : downloaded ? (
        <>
          <span style={{ flex: 1 }}>{t("update.restart")}</span>
          <button onClick={install} style={actionBtnStyle}>
            {t("update.restartBtn")}
          </button>
        </>
      ) : downloading ? (
        <>
          <span style={{ flex: 1 }}>
            {t("update.downloading", { progress: String(progress) })}
          </span>
          <div style={{
            width: 60,
            height: 4,
            borderRadius: 2,
            background: "var(--bg-tertiary, rgba(128,128,128,0.2))",
            overflow: "hidden",
          }}>
            <div style={{
              width: `${progress}%`,
              height: "100%",
              borderRadius: 2,
              background: "var(--accent, #3b82f6)",
              transition: "width 0.3s ease",
            }} />
          </div>
        </>
      ) : (
        <>
          <UpgradeIcon />
          <span style={{ flex: 1 }}>
            {t("update.available", { version })}
          </span>
          <button onClick={download} style={actionBtnStyle}>
            {t("update.download")}
          </button>
          <button onClick={dismiss} style={closeBtnStyle}>
            <CloseIcon />
          </button>
        </>
      )}
    </div>
  );
}

const actionBtnStyle: React.CSSProperties = {
  padding: "4px 10px",
  fontSize: 11,
  fontWeight: 700,
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  background: "var(--accent, #3b82f6)",
  color: "#fff",
};

const closeBtnStyle: React.CSSProperties = {
  padding: 2,
  border: "none",
  background: "transparent",
  cursor: "pointer",
  color: "var(--text-secondary)",
  display: "flex",
  alignItems: "center",
};

function UpgradeIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}
