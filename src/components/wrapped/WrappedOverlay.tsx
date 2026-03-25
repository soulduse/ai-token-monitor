import { useState, useMemo, useRef, useCallback, useEffect } from "react";
import type { AllStats } from "../../lib/types";
import { useSettings } from "../../contexts/SettingsContext";
import { useI18n } from "../../i18n/I18nContext";
import { useShareImage } from "../../hooks/useShareImage";
import {
  filterByPeriod,
  computeTotalCost,
  computeTotalTokens,
  findBusiestDay,
  getMostUsedModel,
  computeCacheHitRate,
  computeStreaks,
} from "../../lib/statsHelpers";
import { CARDS, CARD_COUNT } from "./WrappedCards";
import type { WrappedData } from "./WrappedCards";

interface Props {
  visible: boolean;
  onClose: () => void;
  stats: AllStats;
}

type WrappedPeriod = "month" | "year";

export function WrappedOverlay({ visible, onClose, stats }: Props) {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [period, setPeriod] = useState<WrappedPeriod>("month");
  const cardRef = useRef<HTMLDivElement>(null);
  const { capture, captured } = useShareImage(cardRef);
  const touchStartX = useRef(0);
  const { prefs } = useSettings();
  const t = useI18n();

  // Reset index when opening
  useEffect(() => {
    if (visible) setCurrentIndex(0);
  }, [visible]);

  const data = useMemo((): WrappedData => {
    const filterPeriod = period === "month" ? "month" : "year";
    const filtered = filterByPeriod(stats.daily, filterPeriod);
    const now = new Date();
    const periodLabel = period === "month"
      ? now.toLocaleDateString("en", { month: "long", year: "numeric" })
      : `${now.getFullYear()}`;

    return {
      period: periodLabel,
      locale: prefs.language,
      totalCost: computeTotalCost(filtered),
      totalTokens: computeTotalTokens(filtered),
      topModel: getMostUsedModel(stats.model_usage),
      busiestDay: findBusiestDay(filtered),
      cacheHitRate: computeCacheHitRate(stats.model_usage),
      streaks: computeStreaks(stats.daily),
      totalMessages: filtered.reduce((s, d) => s + d.messages, 0),
      totalSessions: filtered.reduce((s, d) => s + d.sessions, 0),
      modelUsage: stats.model_usage,
    };
  }, [stats, period, prefs.language]);

  const goNext = useCallback(() => {
    setCurrentIndex((i) => Math.min(i + 1, CARD_COUNT - 1));
  }, []);

  const goPrev = useCallback(() => {
    setCurrentIndex((i) => Math.max(i - 1, 0));
  }, []);

  // Keyboard navigation
  useEffect(() => {
    if (!visible) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight" || e.key === " ") goNext();
      else if (e.key === "ArrowLeft") goPrev();
      else if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [visible, goNext, goPrev, onClose]);

  if (!visible) return null;

  const CardComponent = CARDS[currentIndex];

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
        }}
        onTouchStart={(e) => {
          touchStartX.current = e.touches[0].clientX;
        }}
        onTouchEnd={(e) => {
          const diff = e.changedTouches[0].clientX - touchStartX.current;
          if (diff > 50) goPrev();
          else if (diff < -50) goNext();
        }}
        onClick={(e) => {
          // Click left half = prev, right half = next
          const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
          const x = e.clientX - rect.left;
          if (x < rect.width / 2) goPrev();
          else goNext();
        }}
      >
        {/* Close button */}
        <button
          onClick={(e) => { e.stopPropagation(); onClose(); }}
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

        {/* Period toggle */}
        <div
          onClick={(e) => e.stopPropagation()}
          style={{
            display: "flex",
            gap: 4,
            marginBottom: 14,
            background: "rgba(255,255,255,0.15)",
            borderRadius: 10,
            padding: 4,
            backdropFilter: "blur(8px)",
            WebkitBackdropFilter: "blur(8px)",
            border: "1px solid rgba(255,255,255,0.15)",
          }}
        >
          {(["month", "year"] as WrappedPeriod[]).map((p) => (
            <button
              key={p}
              onClick={() => setPeriod(p)}
              style={{
                fontSize: 12,
                fontWeight: 700,
                padding: "6px 14px",
                borderRadius: 7,
                border: "none",
                cursor: "pointer",
                background: period === p ? "#fff" : "transparent",
                color: period === p ? "#1a1a1a" : "rgba(255,255,255,0.7)",
                transition: "all 0.15s ease",
              }}
            >
              {t(p === "month" ? "wrapped.month" : "wrapped.year")}
            </button>
          ))}
        </div>

        {/* Card */}
        <CardComponent ref={cardRef} data={data} />

        {/* Progress dots + controls */}
        <div
          onClick={(e) => e.stopPropagation()}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            marginTop: 16,
          }}
        >
          {/* Dots */}
          <div style={{ display: "flex", gap: 6 }}>
            {Array.from({ length: CARD_COUNT }, (_, i) => (
              <div
                key={i}
                onClick={() => setCurrentIndex(i)}
                style={{
                  width: i === currentIndex ? 16 : 6,
                  height: 6,
                  borderRadius: 3,
                  background: i === currentIndex ? "#fff" : "rgba(255,255,255,0.3)",
                  cursor: "pointer",
                  transition: "all 0.2s ease",
                }}
              />
            ))}
          </div>

          {/* Share */}
          <button
            onClick={capture}
            style={{
              background: captured ? "var(--accent-mint)" : "rgba(255,255,255,0.2)",
              border: "none",
              borderRadius: 16,
              padding: "6px 14px",
              cursor: "pointer",
              color: "#fff",
              fontSize: 10,
              fontWeight: 700,
              display: "flex",
              alignItems: "center",
              gap: 4,
              transition: "all 0.2s ease",
            }}
          >
            {captured ? (
              <>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="20 6 9 17 4 12"/>
                </svg>
                {t("wrapped.copied")}
              </>
            ) : (
              <>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8"/>
                  <polyline points="16 6 12 2 8 6"/>
                  <line x1="12" y1="2" x2="12" y2="15"/>
                </svg>
                {t("wrapped.share")}
              </>
            )}
          </button>
        </div>

        {/* Card counter */}
        <div style={{
          marginTop: 8,
          fontSize: 10,
          fontWeight: 600,
          color: "rgba(255,255,255,0.4)",
        }}>
          {currentIndex + 1} / {CARD_COUNT}
        </div>
      </div>
    </>
  );
}
