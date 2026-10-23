import { useState, useRef, useEffect, useCallback } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { LeaderboardEntry } from "../../hooks/useLeaderboardSync";
import type { LeaderboardView } from "../../lib/types";
import { useBadgeData } from "../../hooks/useBadgeData";
import { useShareImage } from "../../hooks/useShareImage";
import { useI18n } from "../../i18n/I18nContext";
import { generateFlatBadgeSvg, generateCardBadgeSvg } from "../../lib/badgeSvgTemplate";
import { BadgeCardPreview } from "./BadgeCardPreview";
import { BadgeCompactPreview } from "./BadgeCompactPreview";
import { BadgeLeaderboardPreview } from "./BadgeLeaderboardPreview";

interface Props {
  visible: boolean;
  onClose: () => void;
  leaderboard: LeaderboardEntry[];
  userId: string;
  provider: LeaderboardView;
  period: "today" | "week" | "month";
  dateRange: { from: string; to: string };
}

type BadgeStyle = "leaderboard" | "card" | "compact";
type CardTheme = "light" | "dark";
type CompactStyle = "flat" | "flat-square";

const SUPABASE_BADGE_URL = "https://giunmtxxvapcgrpxjopq.supabase.co/functions/v1/badge";

export function BadgeOverlay({
  visible,
  onClose,
  leaderboard,
  userId,
  provider,
  period,
  dateRange,
}: Props) {
  const [badgeStyle, setBadgeStyle] = useState<BadgeStyle>("leaderboard");
  const [cardTheme, setCardTheme] = useState<CardTheme>("dark");
  const [compactStyle, setCompactStyle] = useState<CompactStyle>("flat");
  const [toast, setToast] = useState<string | null>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const { capture, savePng } = useShareImage(cardRef);
  const t = useI18n();

  const data = useBadgeData(leaderboard, userId, provider, period, dateRange);

  // Reset state when opening
  useEffect(() => {
    if (visible) {
      setBadgeStyle("leaderboard");
      setToast(null);
    }
  }, [visible]);

  // Keyboard navigation
  useEffect(() => {
    if (!visible) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [visible, onClose]);

  // Cleanup timer on unmount
  useEffect(() => {
    return () => {
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    };
  }, []);

  const showToast = useCallback((message: string) => {
    setToast(message);
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    toastTimerRef.current = setTimeout(() => setToast(null), 2000);
  }, []);

  const handleCopySvg = useCallback(async () => {
    if (!data) return;
    const svgData = {
      nickname: data.nickname,
      rank: data.rank,
      totalTokens: data.totalTokens,
      costUsd: data.costUsd,
      provider: data.provider,
      period: data.period,
    };
    const svg = badgeStyle === "compact"
      ? generateFlatBadgeSvg(svgData, compactStyle)
      : generateCardBadgeSvg(svgData, cardTheme);
    await writeText(svg);
    showToast(t("badge.copied"));
  }, [data, badgeStyle, compactStyle, cardTheme, showToast, t]);

  const getBadgeUrl = useCallback(() => {
    return `${SUPABASE_BADGE_URL}/${userId}?provider=${provider}&period=${period}&style=${badgeStyle === "compact" ? compactStyle : "card"}`;
  }, [userId, provider, period, badgeStyle, compactStyle]);

  const handleCopyMarkdown = useCallback(async () => {
    const url = getBadgeUrl();
    await writeText(`![AI Token Monitor Badge](${url})`);
    showToast(t("badge.copied"));
  }, [getBadgeUrl, showToast, t]);

  const handleCopyUrl = useCallback(async () => {
    const url = getBadgeUrl();
    await writeText(url);
    showToast(t("badge.copied"));
  }, [getBadgeUrl, showToast, t]);

  const handleCapture = useCallback(async () => {
    await capture();
    showToast(t("badge.copied"));
  }, [capture, showToast, t]);

  const handleSavePng = useCallback(async () => {
    await savePng();
    showToast(t("badge.saved"));
  }, [savePng, showToast, t]);

  if (!visible || !data) return null;

  const pillToggle = (
    options: { key: string; label: string }[],
    active: string,
    onChange: (key: string) => void,
  ) => (
    <div
      onClick={(e) => e.stopPropagation()}
      style={{
        display: "flex",
        gap: 4,
        background: "rgba(255,255,255,0.15)",
        borderRadius: 10,
        padding: 4,
        backdropFilter: "blur(8px)",
        WebkitBackdropFilter: "blur(8px)",
        border: "1px solid rgba(255,255,255,0.15)",
      }}
    >
      {options.map((opt) => (
        <button
          key={opt.key}
          onClick={() => onChange(opt.key)}
          style={{
            fontSize: 12,
            fontWeight: 700,
            padding: "6px 14px",
            borderRadius: 7,
            border: "none",
            cursor: "pointer",
            background: active === opt.key ? "#fff" : "transparent",
            color: active === opt.key ? "#1a1a1a" : "rgba(255,255,255,0.7)",
            transition: "all 0.15s ease",
          }}
        >
          {opt.label}
        </button>
      ))}
    </div>
  );

  const actionBtn = (
    label: string,
    onClick: () => void,
    icon?: React.ReactNode,
  ) => (
    <button
      onClick={onClick}
      style={{
        background: "rgba(255,255,255,0.2)",
        border: "none",
        borderRadius: 12,
        padding: "8px 14px",
        cursor: "pointer",
        color: "#fff",
        fontSize: 11,
        fontWeight: 700,
        display: "flex",
        alignItems: "center",
        gap: 4,
        transition: "all 0.2s ease",
        whiteSpace: "nowrap",
      }}
    >
      {icon}
      {label}
    </button>
  );

  return (
    <>
      {/* Backdrop */}
      <div
        onClick={onClose}
        style={{
          position: "fixed",
          inset: 0,
          background: "rgba(0,0,0,0.7)",
          zIndex: 60,
        }}
      />

      {/* Content */}
      <div
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 61,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          padding: 16,
          gap: 14,
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Close button */}
        <button
          onClick={onClose}
          style={{
            position: "fixed",
            top: 12,
            right: 12,
            background: "rgba(255,255,255,0.15)",
            border: "none",
            borderRadius: 20,
            width: 32,
            height: 32,
            cursor: "pointer",
            color: "#fff",
            fontSize: 16,
            fontWeight: 700,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 62,
          }}
        >
          ✕
        </button>

        {/* Title */}
        <div style={{ color: "#fff", fontSize: 16, fontWeight: 800 }}>
          {t("badge.title")}
        </div>

        {/* Style toggle */}
        {pillToggle(
          [
            { key: "leaderboard", label: t("badge.leaderboard") },
            { key: "card", label: t("badge.card") },
            { key: "compact", label: t("badge.compact") },
          ],
          badgeStyle,
          (k) => setBadgeStyle(k as BadgeStyle),
        )}

        {/* Sub-style toggle */}
        {(badgeStyle === "card" || badgeStyle === "leaderboard") &&
          pillToggle(
            [
              { key: "dark", label: t("badge.dark") },
              { key: "light", label: t("badge.light") },
            ],
            cardTheme,
            (k) => setCardTheme(k as CardTheme),
          )}
        {badgeStyle === "compact" &&
          pillToggle(
            [
              { key: "flat", label: t("badge.flat") },
              { key: "flat-square", label: t("badge.flatSquare") },
            ],
            compactStyle,
            (k) => setCompactStyle(k as CompactStyle),
          )}

        {/* Badge preview */}
        {badgeStyle === "leaderboard" ? (
          <BadgeLeaderboardPreview
            ref={cardRef}
            leaderboard={leaderboard}
            userId={userId}
            provider={provider}
            period={period}
            theme={cardTheme}
          />
        ) : badgeStyle === "card" ? (
          <BadgeCardPreview ref={cardRef} data={data} theme={cardTheme} />
        ) : (
          <div ref={cardRef}>
            <BadgeCompactPreview data={data} style={compactStyle} />
          </div>
        )}

        {/* Action buttons row */}
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", justifyContent: "center" }}>
          {actionBtn(t("badge.copyImage"), handleCapture, <ShareIcon />)}
          {actionBtn(t("badge.savePng"), handleSavePng, <DownloadIcon />)}
          {actionBtn(t("badge.copySvg"), handleCopySvg, <CodeIcon />)}
          {actionBtn(t("badge.copyMarkdown"), handleCopyMarkdown, <MarkdownIcon />)}
        </div>

        {/* Dynamic Badge URL section */}
        <div
          style={{
            background: "rgba(255,255,255,0.1)",
            borderRadius: 12,
            padding: "12px 16px",
            maxWidth: 400,
            width: "100%",
            border: "1px solid rgba(255,255,255,0.1)",
          }}
        >
          <div style={{ fontSize: 11, fontWeight: 700, color: "#fff", marginBottom: 4 }}>
            {t("badge.urlSection")}
          </div>
          <div style={{ fontSize: 10, color: "rgba(255,255,255,0.5)", marginBottom: 8 }}>
            {t("badge.urlDesc")}
          </div>
          <div
            style={{
              background: "rgba(0,0,0,0.3)",
              borderRadius: 6,
              padding: "8px 10px",
              fontSize: 9,
              fontFamily: "monospace",
              color: "rgba(255,255,255,0.7)",
              wordBreak: "break-all",
              lineHeight: 1.4,
              marginBottom: 8,
            }}
          >
            {`![AI Token Monitor](${getBadgeUrl()})`}
          </div>
          {actionBtn(t("badge.copyUrl"), handleCopyUrl, <LinkIcon />)}
        </div>

        {/* Toast */}
        {toast && (
          <div style={{
            position: "fixed",
            top: 20,
            left: "50%",
            transform: "translateX(-50%)",
            background: "#10b981",
            color: "#fff",
            padding: "8px 20px",
            borderRadius: 20,
            fontSize: 12,
            fontWeight: 700,
            display: "flex",
            alignItems: "center",
            gap: 6,
            zIndex: 63,
            boxShadow: "0 4px 12px rgba(0,0,0,0.3)",
            animation: "badge-toast-in 0.2s ease",
          }}>
            <CheckIcon />
            {toast}
          </div>
        )}
        <style>{`
          @keyframes badge-toast-in {
            from { opacity: 0; transform: translateX(-50%) translateY(-8px); }
            to { opacity: 1; transform: translateX(-50%) translateY(0); }
          }
        `}</style>
      </div>
    </>
  );
}

function CheckIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function ShareIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" />
      <polyline points="16 6 12 2 8 6" />
      <line x1="12" y1="2" x2="12" y2="15" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  );
}

function CodeIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="16 18 22 12 16 6" />
      <polyline points="8 6 2 12 8 18" />
    </svg>
  );
}

function MarkdownIcon() {
  return (
    <svg width="14" height="12" viewBox="0 0 16 10" fill="currentColor">
      <path d="M1 1h2l2 3 2-3h2v8H7V4.5L5 7.5 3 4.5V9H1V1zm11 0h2l2.5 4V9h-2V5.5L13 9h-1l-1.5-3.5V9h-2V1z" />
    </svg>
  );
}

function LinkIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
      <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
    </svg>
  );
}
