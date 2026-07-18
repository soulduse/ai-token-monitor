import { useState, useEffect, useMemo, useRef } from "react";
import { getVersion } from "@tauri-apps/api/app";
import type { AllStats } from "../../lib/types";
import type { Period } from "../../lib/statsHelpers";
import { useI18n } from "../../i18n/I18nContext";
import { useShareImage } from "../../hooks/useShareImage";
import { useToday } from "../../hooks/useToday";
import { useDateNav } from "../../hooks/useDateNav";
import { useSettings } from "../../contexts/SettingsContext";
import { formatNavDate } from "../../lib/format";
import { DateNavigator } from "../DateNavigator";
import { Receipt } from "./Receipt";

interface Props {
  visible: boolean;
  onClose: () => void;
  stats: AllStats;
}

const PERIODS: { id: Period; key: string }[] = [
  { id: "today", key: "receipt.today" },
  { id: "week", key: "receipt.thisWeek" },
  { id: "month", key: "receipt.thisMonth" },
  { id: "all", key: "receipt.allTime" },
];

export function ReceiptOverlay({ visible, onClose, stats }: Props) {
  const [period, setPeriod] = useState<Period>("today");
  const [appVersion, setAppVersion] = useState("0.0.0");
  const receiptRef = useRef<HTMLDivElement>(null);
  const { capture, captured } = useShareImage(receiptRef);
  const t = useI18n();
  const { prefs } = useSettings();

  const todayStr = useToday();
  const minDate = useMemo(() => {
    if (stats.daily.length === 0) return null;
    return stats.daily.reduce((min, d) => (d.date < min ? d.date : min), stats.daily[0].date);
  }, [stats.daily]);
  const dateNav = useDateNav(todayStr, minDate);

  useEffect(() => {
    getVersion().then(setAppVersion);
  }, []);

  if (!visible) return null;

  return (
    <>
      {/* Backdrop */}
      <div
        onClick={onClose}
        style={{
          position: "fixed",
          inset: 0,
          background: "rgba(0,0,0,0.6)",
          zIndex: 60,
        }}
      />

      {/* Content */}
      <div style={{
        position: "fixed",
        inset: 0,
        zIndex: 61,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        padding: "16px 0",
        overflowY: "auto",
        WebkitOverflowScrolling: "touch",
      }}>
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

        {/* Period selector */}
        <div style={{
          display: "flex",
          gap: 4,
          marginBottom: 14,
          background: "rgba(255,255,255,0.15)",
          borderRadius: 10,
          padding: 4,
          backdropFilter: "blur(8px)",
          WebkitBackdropFilter: "blur(8px)",
          border: "1px solid rgba(255,255,255,0.15)",
        }}>
          {PERIODS.map((p) => (
            <button
              key={p.id}
              onClick={() => setPeriod(p.id)}
              style={{
                fontSize: 12,
                fontWeight: 700,
                padding: "6px 14px",
                borderRadius: 7,
                border: "none",
                cursor: "pointer",
                background: period === p.id ? "#fff" : "transparent",
                color: period === p.id ? "#1a1a1a" : "rgba(255,255,255,0.7)",
                transition: "all 0.15s ease",
              }}
            >
              {t(p.key)}
            </button>
          ))}
        </div>

        {/* Day navigation — only meaningful for the daily receipt */}
        {period === "today" && (
          <div style={{ marginTop: -8, marginBottom: 10 }}>
            <DateNavigator
              nav={dateNav}
              label={dateNav.isToday ? t("receipt.today") : formatNavDate(dateNav.date, prefs.language)}
              tone="overlay"
            />
          </div>
        )}

        {/* Receipt */}
        <Receipt
          ref={receiptRef}
          stats={stats}
          period={period}
          date={period === "today" ? dateNav.date : undefined}
          appVersion={appVersion}
        />

        {/* Share button */}
        <button
          onClick={capture}
          style={{
            marginTop: 12,
            padding: "8px 24px",
            borderRadius: 20,
            border: "none",
            cursor: "pointer",
            background: captured
              ? "var(--accent-mint)"
              : "linear-gradient(135deg, var(--accent-purple), var(--accent-pink))",
            color: "#fff",
            fontSize: 12,
            fontWeight: 700,
            display: "flex",
            alignItems: "center",
            gap: 6,
            transition: "all 0.2s ease",
          }}
        >
          {captured ? (
            <>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="20 6 9 17 4 12"/>
              </svg>
              {t("receipt.copied")}
            </>
          ) : (
            <>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8"/>
                <polyline points="16 6 12 2 8 6"/>
                <line x1="12" y1="2" x2="12" y2="15"/>
              </svg>
              {t("receipt.share")}
            </>
          )}
        </button>
      </div>
    </>
  );
}
