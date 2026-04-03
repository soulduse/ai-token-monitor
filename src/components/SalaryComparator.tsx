import { useMemo, useRef, useState } from "react";
import type { AllStats } from "../lib/types";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";
import { formatCost } from "../lib/format";
import { filterByPeriod, computeTotalCost } from "../lib/statsHelpers";
import { useShareImage } from "../hooks/useShareImage";
import { SettingsOverlay } from "./SettingsOverlay";

interface Props {
  stats: AllStats;
}

export function SalaryComparator({ stats }: Props) {
  const [showSettings, setShowSettings] = useState(false);
  const { prefs } = useSettings();
  const t = useI18n();
  const cardRef = useRef<HTMLDivElement>(null);
  const { capture, captured } = useShareImage(cardRef);

  const { monthCost, activeDays } = useMemo(() => {
    const monthData = filterByPeriod(stats.daily, "month");
    return {
      monthCost: computeTotalCost(monthData),
      activeDays: monthData.filter((d) => d.cost_usd > 0).length,
    };
  }, [stats.daily]);

  if (!prefs.salary_enabled) return null;

  const salary = prefs.monthly_salary;

  if (!salary) {
    return (
      <div style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-md)",
        padding: "12px 14px",
        boxShadow: "var(--shadow-card)",
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
      }}>
        <div>
          <div style={{ fontSize: 12, fontWeight: 700, color: "var(--text-primary)" }}>
            {t("salary.title")}
          </div>
          <div style={{ fontSize: 10, color: "var(--text-secondary)", marginTop: 2 }}>
            {t("salary.setup")}
          </div>
        </div>
        <button
          onClick={() => setShowSettings(true)}
          style={{
            fontSize: 10,
            fontWeight: 700,
            padding: "5px 12px",
            borderRadius: 6,
            border: "none",
            cursor: "pointer",
            background: "var(--accent-purple)",
            color: "#fff",
            transition: "opacity 0.15s ease",
          }}
        >
          {t("salary.setupButton")}
        </button>
        <SettingsOverlay visible={showSettings} onClose={() => setShowSettings(false)} />
      </div>
    );
  }

  const percent = (monthCost / salary) * 100;
  const dailyAvg = activeDays > 0 ? monthCost / activeDays : 0;

  const equivalents = [
    { icon: "☕", count: Math.floor(monthCost / 5.5), label: t("salary.coffee") },
    { icon: "📺", count: Math.floor(monthCost / 17.99), label: t("salary.netflix") },
    { icon: "🍗", count: Math.floor(monthCost / 20), label: t("salary.chicken") },
  ];

  return (
    <div
      ref={cardRef}
      style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-md)",
        padding: "14px",
        boxShadow: "var(--shadow-card)",
        position: "relative",
      }}
    >
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
        <div style={{
          fontSize: 10,
          fontWeight: 700,
          color: "var(--text-secondary)",
          textTransform: "uppercase",
          letterSpacing: "0.5px",
        }}>
          {t("salary.title")}
        </div>
        <button
          onClick={capture}
          title={t("salary.share")}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            padding: 2,
            color: captured ? "var(--accent-mint)" : "var(--text-secondary)",
            transition: "color 0.2s ease",
          }}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            {captured ? (
              <polyline points="20 6 9 17 4 12"/>
            ) : (
              <>
                <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8"/>
                <polyline points="16 6 12 2 8 6"/>
                <line x1="12" y1="2" x2="12" y2="15"/>
              </>
            )}
          </svg>
        </button>
      </div>

      {/* Percentage */}
      <div style={{ marginBottom: 12 }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 6 }}>
          <span style={{
            fontSize: 28,
            fontWeight: 800,
            color: "var(--accent-purple)",
            letterSpacing: "-1px",
          }}>
            {percent < 0.01 ? "<0.01" : percent.toFixed(percent < 1 ? 2 : 1)}%
          </span>
          <span style={{ fontSize: 11, fontWeight: 600, color: "var(--text-secondary)" }}>
            {t("salary.percentOfSalary")}
          </span>
        </div>
        {/* Progress bar — segmented HP bar style */}
        {(() => {
          const segCount = 10;
          const filled = Math.round((Math.min(percent, 100) / 100) * segCount);
          return (
            <div style={{
              display: "flex",
              gap: 3,
              width: "100%",
              height: 10,
              padding: 2,
              background: "rgba(0,0,0,0.3)",
              borderRadius: 3,
              border: "1px solid rgba(255,255,255,0.08)",
              marginTop: 6,
            }}>
              {Array.from({ length: segCount }, (_, i) => (
                <div
                  key={i}
                  style={{
                    flex: 1,
                    height: "100%",
                    borderRadius: 1,
                    background: i < filled ? "var(--accent-purple)" : "rgba(255,255,255,0.06)",
                    boxShadow: i < filled ? "0 0 4px rgba(168,85,247,0.4)" : "none",
                    transition: "background 0.3s ease",
                  }}
                />
              ))}
            </div>
          );
        })()}
        <div style={{
          display: "flex",
          justifyContent: "space-between",
          marginTop: 4,
          fontSize: 9,
          color: "var(--text-secondary)",
          fontWeight: 600,
        }}>
          <span>AI: {formatCost(monthCost)}</span>
          <span>{t("salary.perDay")}: {formatCost(dailyAvg)}</span>
        </div>
      </div>

      {/* Equivalents */}
      <div style={{
        display: "flex",
        gap: 6,
      }}>
        {equivalents.map((eq) => (
          <div
            key={eq.label}
            style={{
              flex: 1,
              background: "var(--bg-primary)",
              borderRadius: "var(--radius-sm)",
              padding: "8px 6px",
              textAlign: "center",
            }}
          >
            <div style={{ fontSize: 16 }}>{eq.icon}</div>
            <div style={{
              fontSize: 16,
              fontWeight: 800,
              color: "var(--accent-purple)",
              marginTop: 2,
            }}>
              {eq.count}
            </div>
            <div style={{
              fontSize: 8,
              fontWeight: 600,
              color: "var(--text-secondary)",
              textTransform: "uppercase",
              marginTop: 1,
            }}>
              {eq.label}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
