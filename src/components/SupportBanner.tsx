import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "../i18n/I18nContext";

const SUPPORT_URL = "https://ctee.kr/place/programmingzombie";

export function SupportBanner() {
  const t = useI18n();
  return (
    <button
      onClick={() => openUrl(SUPPORT_URL)}
      style={{
        width: "100%",
        padding: "10px 0",
        marginTop: 4,
        fontSize: 12,
        fontWeight: 700,
        border: "none",
        borderRadius: "var(--radius-md)",
        cursor: "pointer",
        background: "var(--bg-card)",
        color: "var(--text-secondary)",
        boxShadow: "var(--shadow-card)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 6,
        transition: "all 0.2s ease",
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M17 8h1a4 4 0 1 1 0 8h-1"/>
        <path d="M3 8h14v9a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4Z"/>
        <line x1="6" y1="2" x2="6" y2="4"/>
        <line x1="10" y1="2" x2="10" y2="4"/>
        <line x1="14" y1="2" x2="14" y2="4"/>
      </svg>
      {t("support.buyMeCoffee")}
    </button>
  );
}
