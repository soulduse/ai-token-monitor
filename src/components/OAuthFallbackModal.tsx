import { useEffect, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAuth } from "../contexts/AuthContext";
import { useI18n } from "../i18n/I18nContext";

const COPY_FEEDBACK_MS = 1500;

export function OAuthFallbackModal() {
  const { oauthFallback, dismissOauthFallback } = useAuth();
  const t = useI18n();
  const [copied, setCopied] = useState(false);
  const [retryError, setRetryError] = useState(false);

  useEffect(() => {
    if (!oauthFallback) return;
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        dismissOauthFallback();
      }
    };
    document.addEventListener("keydown", handleEsc, true);
    return () => document.removeEventListener("keydown", handleEsc, true);
  }, [oauthFallback, dismissOauthFallback]);

  if (!oauthFallback) return null;

  const handleCopy = async () => {
    try {
      await writeText(oauthFallback.url);
      setCopied(true);
      setTimeout(() => setCopied(false), COPY_FEEDBACK_MS);
    } catch (err) {
      console.error("[OAuth] clipboard write failed:", err);
    }
  };

  const handleRetry = async () => {
    setRetryError(false);
    try {
      await openUrl(oauthFallback.url);
    } catch (err) {
      console.error("[OAuth] retry openUrl failed:", err);
      setRetryError(true);
    }
  };

  const titleKey = (() => {
    switch (oauthFallback.reason) {
      case "open-failed": return "oauthFallback.title.openFailed";
      case "timeout":     return "oauthFallback.title.timeout";
      case "manual":      return "oauthFallback.title.manual";
    }
  })();

  const descriptionKey = (() => {
    switch (oauthFallback.reason) {
      case "open-failed": return "oauthFallback.description.openFailed";
      case "timeout":     return "oauthFallback.description.timeout";
      case "manual":      return "oauthFallback.description.manual";
    }
  })();

  return (
    <>
      <div
        onClick={dismissOauthFallback}
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 1100,
          background: "rgba(0, 0, 0, 0.55)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          onClick={(e) => e.stopPropagation()}
          style={{
            width: "min(440px, 92vw)",
            background: "#1f2024",
            color: "#f5f5f5",
            borderRadius: 12,
            border: "1px solid rgba(255,255,255,0.08)",
            boxShadow: "0 20px 40px rgba(0,0,0,0.4)",
            padding: 20,
            display: "flex",
            flexDirection: "column",
            gap: 14,
          }}
        >
          <div style={{ fontSize: 15, fontWeight: 600 }}>
            {t(titleKey)}
          </div>

          <div style={{ fontSize: 12.5, lineHeight: 1.55, color: "#c9c9d1" }}>
            {t(descriptionKey)}
          </div>

          <div
            style={{
              fontSize: 11.5,
              fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
              padding: "10px 12px",
              borderRadius: 8,
              background: "#101114",
              border: "1px solid rgba(255,255,255,0.06)",
              wordBreak: "break-all",
              color: "#e3e3eb",
              maxHeight: 110,
              overflowY: "auto",
            }}
          >
            {oauthFallback.url}
          </div>

          {retryError && (
            <div style={{ fontSize: 11.5, color: "#ff8888" }}>
              {t("oauthFallback.retryFailed")}
            </div>
          )}

          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button
              type="button"
              onClick={dismissOauthFallback}
              style={{
                padding: "7px 14px",
                fontSize: 12.5,
                background: "transparent",
                color: "#c9c9d1",
                border: "1px solid rgba(255,255,255,0.12)",
                borderRadius: 7,
                cursor: "pointer",
              }}
            >
              {t("oauthFallback.close")}
            </button>
            <button
              type="button"
              onClick={handleCopy}
              style={{
                padding: "7px 14px",
                fontSize: 12.5,
                background: copied ? "#2d6a4f" : "rgba(255,255,255,0.06)",
                color: "#ffffff",
                border: "1px solid rgba(255,255,255,0.12)",
                borderRadius: 7,
                cursor: "pointer",
                transition: "background 120ms ease",
              }}
            >
              {copied ? t("oauthFallback.copied") : t("oauthFallback.copyLink")}
            </button>
            <button
              type="button"
              onClick={handleRetry}
              style={{
                padding: "7px 14px",
                fontSize: 12.5,
                background: "#3b82f6",
                color: "#ffffff",
                border: "1px solid #3b82f6",
                borderRadius: 7,
                cursor: "pointer",
              }}
            >
              {t("oauthFallback.openInBrowser")}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
