import { useI18n } from "../i18n/I18nContext";
import { useStarPrompt } from "../hooks/useStarPrompt";
import type { DailyUsage } from "../lib/types";

interface Props {
  daily: DailyUsage[] | undefined;
}

function StarIcon({ filled }: { filled?: boolean }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill={filled ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  );
}

export function StarPromptToast({ daily }: Props) {
  const t = useI18n();
  const { visible, onStar, onLater, onClose } = useStarPrompt(daily);

  if (!visible) return null;

  return (
    <div
      role="dialog"
      aria-label={t("starPrompt.title")}
      style={{
        position: "fixed",
        left: 12,
        right: 12,
        bottom: 12,
        // Below WrappedOverlay (60+) and OAuthFallbackModal (1100) so a rare
        // collision leaves the toast underneath instead of covering them.
        zIndex: 50,
        background: "var(--bg-card)",
        borderRadius: "var(--radius-lg)",
        border: "1px solid rgba(124, 92, 252, 0.15)",
        boxShadow: "var(--shadow-soft), var(--shadow-card)",
        padding: 14,
        display: "flex",
        flexDirection: "column",
        gap: 10,
        animation: "star-prompt-in 0.25s ease",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span
          style={{
            width: 24,
            height: 24,
            borderRadius: "50%",
            background: "rgba(230, 175, 60, 0.16)",
            color: "var(--accent-orange)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <StarIcon filled />
        </span>
        <span style={{ fontSize: 13, fontWeight: 800, color: "var(--text-primary)", flex: 1 }}>
          {t("starPrompt.title")}
        </span>
        <button
          onClick={onClose}
          aria-label={t("starPrompt.close")}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-secondary)",
            cursor: "pointer",
            fontSize: 14,
            lineHeight: 1,
            padding: 4,
          }}
        >
          ✕
        </button>
      </div>

      <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5, color: "var(--text-secondary)" }}>
        {t("starPrompt.body")}
      </p>

      <div style={{ display: "flex", gap: 8 }}>
        <button
          onClick={onStar}
          style={{
            flex: 1,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 6,
            padding: "8px 0",
            fontSize: 12,
            fontWeight: 700,
            border: "none",
            borderRadius: "var(--radius-md)",
            cursor: "pointer",
            background: "var(--accent-purple)",
            color: "#fff",
          }}
        >
          <StarIcon />
          {t("starPrompt.cta")}
        </button>
        <button
          onClick={onLater}
          style={{
            padding: "8px 14px",
            fontSize: 12,
            fontWeight: 700,
            border: "none",
            borderRadius: "var(--radius-md)",
            cursor: "pointer",
            background: "transparent",
            color: "var(--text-secondary)",
          }}
        >
          {t("starPrompt.later")}
        </button>
      </div>
    </div>
  );
}
