import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useOAuthUsage } from "../hooks/useOAuthUsage";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

const REFRESH_COOLDOWN_SECONDS = 30;

function getBarColor(percent: number): string {
  if (percent >= 90) return "#ef4444";
  if (percent >= 80) return "#f97316";
  if (percent >= 50) return "#eab308";
  return "#22c55e";
}

function formatResetTime(resetsAt: string, t: (key: string, params?: Record<string, string>) => string): string {
  const reset = new Date(resetsAt);
  const now = new Date();
  const diffMs = reset.getTime() - now.getTime();
  if (diffMs <= 0) return t("usageAlert.resetsNow");
  const totalMin = Math.floor(diffMs / 60000);
  const d = Math.floor(totalMin / 1440);
  const h = Math.floor((totalMin % 1440) / 60);
  const m = totalMin % 60;
  const parts: string[] = [];
  if (d > 0) parts.push(`${d}d`);
  if (h > 0) parts.push(`${h}h`);
  if (m > 0 || parts.length === 0) parts.push(`${m}m`);
  return t("usageAlert.resetsIn", { time: parts.join(" ") });
}

const SEGMENT_COUNT = 10;

function UsageRow({
  label,
  utilization,
  subtitle,
}: {
  label: string;
  utilization: number;
  subtitle: string;
}) {
  const pct = Math.min(utilization, 100);
  const color = getBarColor(utilization);
  const filledSegments = Math.round((pct / 100) * SEGMENT_COUNT);

  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 4,
      }}>
        <span style={{ fontSize: 10, fontWeight: 600, color: "var(--text-primary)" }}>
          {label}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
            {subtitle}
          </span>
          <span style={{ fontSize: 11, fontWeight: 700, color }}>
            {utilization.toFixed(1)}%
          </span>
        </div>
      </div>
      <div style={{
        display: "flex",
        gap: 3,
        width: "100%",
        height: 10,
        padding: 2,
        background: "rgba(0,0,0,0.3)",
        borderRadius: 3,
        border: "1px solid rgba(255,255,255,0.08)",
      }}>
        {Array.from({ length: SEGMENT_COUNT }, (_, i) => (
          <div
            key={i}
            style={{
              flex: 1,
              height: "100%",
              borderRadius: 1,
              background: i < filledSegments ? color : "rgba(255,255,255,0.06)",
              boxShadow: i < filledSegments ? `0 0 4px ${color}40` : "none",
              transition: "background 0.3s ease",
            }}
          />
        ))}
      </div>
    </div>
  );
}

export function UsageAlertBar() {
  const { prefs, refreshPrefs } = useSettings();
  const { usage, refreshing, refresh } = useOAuthUsage();
  const t = useI18n();
  const [enabling, setEnabling] = useState(false);
  const [cooldown, setCooldown] = useState(0);
  const cooldownTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (cooldownTimerRef.current !== null) {
        window.clearInterval(cooldownTimerRef.current);
      }
    };
  }, []);

  const handleRefresh = async () => {
    if (refreshing || cooldown > 0) return;
    setCooldown(REFRESH_COOLDOWN_SECONDS);
    if (cooldownTimerRef.current !== null) {
      window.clearInterval(cooldownTimerRef.current);
    }
    cooldownTimerRef.current = window.setInterval(() => {
      setCooldown((prev) => {
        if (prev <= 1) {
          if (cooldownTimerRef.current !== null) {
            window.clearInterval(cooldownTimerRef.current);
            cooldownTimerRef.current = null;
          }
          return 0;
        }
        return prev - 1;
      });
    }, 1000);
    await refresh();
  };

  if (!prefs.usage_tracking_enabled) {
    return (
      <div style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-lg)",
        padding: "12px 16px",
      }}>
        <div style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-primary)",
          marginBottom: 4,
        }}>
          {t("usageTracking.title")}
        </div>
        <div style={{
          fontSize: 10,
          color: "var(--text-secondary)",
          marginBottom: 10,
          lineHeight: 1.4,
        }}>
          {t("usageTracking.description")}
        </div>
        <button
          onClick={async () => {
            setEnabling(true);
            try {
              await invoke("enable_usage_tracking");
              await refreshPrefs();
            } catch {
              // silently ignore
            } finally {
              setEnabling(false);
            }
          }}
          disabled={enabling}
          style={{
            width: "100%",
            padding: "6px 0",
            fontSize: 11,
            fontWeight: 600,
            color: "var(--text-primary)",
            background: "var(--bg-hover)",
            border: "1px solid var(--border-secondary)",
            borderRadius: "var(--radius-md)",
            cursor: enabling ? "default" : "pointer",
            opacity: enabling ? 0.6 : 1,
            transition: "opacity 0.2s ease",
          }}
        >
          {enabling ? t("usageTracking.enabling") : t("usageTracking.enable")}
        </button>
      </div>
    );
  }

  if (!usage) return null;

  const { five_hour, seven_day, extra_usage, is_stale } = usage;

  // Don't render if no data at all
  if (!five_hour && !seven_day && !extra_usage) return null;

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: "12px 16px",
    }}>
      {/* Header */}
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 8,
      }}>
        <span style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-primary)",
        }}>
          {t("usageAlert.title")}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {is_stale && (
            <span style={{
              fontSize: 9,
              fontWeight: 600,
              color: "var(--text-muted)",
            }}>
              {t("usageAlert.stale")}
            </span>
          )}
          <button
            onClick={handleRefresh}
            disabled={refreshing || cooldown > 0}
            title={
              refreshing
                ? t("usageAlert.refreshing")
                : cooldown > 0
                ? `${t("usageAlert.refresh")} (${cooldown}s)`
                : t("usageAlert.refresh")
            }
            aria-label={t("usageAlert.refresh")}
            style={{
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              width: 18,
              height: 18,
              padding: 0,
              background: "transparent",
              border: "none",
              borderRadius: 3,
              color: "var(--text-muted)",
              cursor: refreshing || cooldown > 0 ? "default" : "pointer",
              opacity: refreshing || cooldown > 0 ? 0.4 : 0.8,
              transition: "opacity 0.2s ease, color 0.2s ease",
            }}
            onMouseEnter={(e) => {
              if (!refreshing && cooldown === 0) {
                e.currentTarget.style.color = "var(--text-primary)";
              }
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.color = "var(--text-muted)";
            }}
          >
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
              style={{
                animation: refreshing ? "miniProfileSpin 0.8s linear infinite" : "none",
              }}
            >
              <path d="M3 12a9 9 0 0 1 15-6.7L21 8" />
              <path d="M21 3v5h-5" />
              <path d="M21 12a9 9 0 0 1-15 6.7L3 16" />
              <path d="M3 21v-5h5" />
            </svg>
          </button>
        </div>
      </div>

      {/* Session (5h) */}
      {five_hour && (
        <UsageRow
          label={t("usageAlert.session")}
          utilization={five_hour.utilization}
          subtitle={formatResetTime(five_hour.resets_at, t)}
        />
      )}

      {/* Weekly (7d) */}
      {seven_day && (
        <UsageRow
          label={t("usageAlert.weekly")}
          utilization={seven_day.utilization}
          subtitle={formatResetTime(seven_day.resets_at, t)}
        />
      )}

      {/* Extra usage */}
      {extra_usage && extra_usage.is_enabled && (
        <UsageRow
          label={t("usageAlert.extraUsage")}
          utilization={extra_usage.utilization}
          subtitle={`$${extra_usage.used_credits.toFixed(2)} / $${extra_usage.monthly_limit.toFixed(2)}`}
        />
      )}
    </div>
  );
}
