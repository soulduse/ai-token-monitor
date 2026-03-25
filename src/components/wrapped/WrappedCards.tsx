import { forwardRef } from "react";
import { formatCost, formatTokens, formatDate } from "../../lib/format";
import { shortenModelName } from "../../lib/statsHelpers";
import type { StreakInfo } from "../../lib/statsHelpers";
import type { ModelUsage } from "../../lib/types";
import { useI18n } from "../../i18n/I18nContext";

export interface WrappedData {
  period: string;
  locale: string;
  totalCost: number;
  totalTokens: number;
  topModel: { name: string; totalTokens: number; cost: number } | null;
  busiestDay: { date: string; tokens: number };
  cacheHitRate: number;
  streaks: StreakInfo;
  totalMessages: number;
  totalSessions: number;
  modelUsage: Record<string, ModelUsage>;
}

interface CardProps {
  data: WrappedData;
}

const CARD_STYLE: React.CSSProperties = {
  width: 360,
  height: 480,
  borderRadius: 20,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  color: "#fff",
  padding: 32,
  position: "relative",
  overflow: "hidden",
  textAlign: "center",
  flexShrink: 0,
};

const GRADIENTS = [
  "linear-gradient(135deg, #7C5CFC, #B5A0EF)",       // TotalCost - purple
  "linear-gradient(135deg, #0ea5e9, #7C5CFC)",        // TopModel - ocean-purple
  "linear-gradient(135deg, #f59e0b, #ef4444)",         // BusiestDay - sunset-red
  "linear-gradient(135deg, #10b981, #0ea5e9)",         // CacheHero - green-ocean
  "linear-gradient(135deg, #ef4444, #f59e0b)",         // Streak - fire
  "linear-gradient(135deg, #7C5CFC, #ec4899)",         // Summary - brand
];

// Card 1: Total Cost
const TotalCostCard = forwardRef<HTMLDivElement, CardProps>(({ data }, ref) => {
  const t = useI18n();
  return (
    <div ref={ref} style={{ ...CARD_STYLE, background: GRADIENTS[0] }}>
      <div style={{ fontSize: 13, fontWeight: 600, opacity: 0.8, marginBottom: 8 }}>
        {data.period}
      </div>
      <div style={{ fontSize: 14, fontWeight: 600, opacity: 0.9, marginBottom: 16 }}>
        {t("wrapped.totalCost")}
      </div>
      <div style={{ fontSize: 56, fontWeight: 800, letterSpacing: "-2px", lineHeight: 1 }}>
        {formatCost(data.totalCost)}
      </div>
      <div style={{ fontSize: 12, fontWeight: 600, opacity: 0.7, marginTop: 16 }}>
        {formatTokens(data.totalTokens, "compact")} tokens
      </div>
      <Watermark />
    </div>
  );
});

// Card 2: Top Model
const TopModelCard = forwardRef<HTMLDivElement, CardProps>(({ data }, ref) => {
  const t = useI18n();
  const top = data.topModel;
  if (!top) return <EmptyCard ref={ref} gradient={GRADIENTS[1]} />;

  // Calculate usage percentages for top 3 models
  const allModels = Object.entries(data.modelUsage)
    .map(([name, u]) => ({ name: shortenModelName(name), total: u.input_tokens + u.output_tokens + u.cache_read }))
    .sort((a, b) => b.total - a.total)
    .slice(0, 3);
  const maxTotal = allModels[0]?.total ?? 1;

  return (
    <div ref={ref} style={{ ...CARD_STYLE, background: GRADIENTS[1] }}>
      <div style={{ fontSize: 14, fontWeight: 600, opacity: 0.9, marginBottom: 20 }}>
        {t("wrapped.topModel")}
      </div>
      <div style={{ fontSize: 36, fontWeight: 800, letterSpacing: "-1px", marginBottom: 24 }}>
        {shortenModelName(top.name)}
      </div>
      <div style={{ width: "100%" }}>
        {allModels.map((m) => (
          <div key={m.name} style={{ marginBottom: 8 }}>
            <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, fontWeight: 600, marginBottom: 3 }}>
              <span>{m.name}</span>
              <span style={{ opacity: 0.7 }}>{formatTokens(m.total, "compact")}</span>
            </div>
            <div style={{ height: 6, background: "rgba(255,255,255,0.2)", borderRadius: 3 }}>
              <div style={{
                height: "100%",
                width: `${(m.total / maxTotal) * 100}%`,
                background: "rgba(255,255,255,0.8)",
                borderRadius: 3,
              }} />
            </div>
          </div>
        ))}
      </div>
      <Watermark />
    </div>
  );
});

// Card 3: Busiest Day
const BusiestDayCard = forwardRef<HTMLDivElement, CardProps>(({ data }, ref) => {
  const t = useI18n();
  return (
    <div ref={ref} style={{ ...CARD_STYLE, background: GRADIENTS[2] }}>
      <div style={{ fontSize: 14, fontWeight: 600, opacity: 0.9, marginBottom: 16 }}>
        {t("wrapped.busiestDay")}
      </div>
      <div style={{ fontSize: 48, marginBottom: 12 }}>
        🔥
      </div>
      <div style={{ fontSize: 28, fontWeight: 800 }}>
        {data.busiestDay.date ? formatDate(data.busiestDay.date, data.locale) : "—"}
      </div>
      <div style={{ fontSize: 40, fontWeight: 800, letterSpacing: "-1px", marginTop: 8 }}>
        {formatTokens(data.busiestDay.tokens, "compact")}
      </div>
      <div style={{ fontSize: 12, fontWeight: 600, opacity: 0.7, marginTop: 4 }}>
        tokens
      </div>
      <Watermark />
    </div>
  );
});

// Card 4: Cache Hero
const CacheHeroCard = forwardRef<HTMLDivElement, CardProps>(({ data }, ref) => {
  const t = useI18n();
  const rate = data.cacheHitRate;
  const circumference = 2 * Math.PI * 60;
  const filled = (rate / 100) * circumference;

  return (
    <div ref={ref} style={{ ...CARD_STYLE, background: GRADIENTS[3] }}>
      <div style={{ fontSize: 14, fontWeight: 600, opacity: 0.9, marginBottom: 20 }}>
        {t("wrapped.cacheHero")}
      </div>
      <svg width="150" height="150" viewBox="0 0 150 150" style={{ marginBottom: 16 }}>
        <circle cx="75" cy="75" r="60" fill="none" stroke="rgba(255,255,255,0.2)" strokeWidth="12" />
        <circle
          cx="75" cy="75" r="60"
          fill="none" stroke="#fff" strokeWidth="12"
          strokeLinecap="round"
          strokeDasharray={`${filled} ${circumference - filled}`}
          strokeDashoffset={circumference / 4}
          style={{ transition: "stroke-dasharray 0.5s ease" }}
        />
        <text x="75" y="75" textAnchor="middle" dominantBaseline="central"
          fill="#fff" fontSize="28" fontWeight="800" fontFamily="Nunito, sans-serif"
        >
          {rate.toFixed(0)}%
        </text>
      </svg>
      <div style={{ fontSize: 12, fontWeight: 600, opacity: 0.8 }}>
        {t("wrapped.cacheDesc")}
      </div>
      <Watermark />
    </div>
  );
});

// Card 5: Streak
const StreakCard = forwardRef<HTMLDivElement, CardProps>(({ data }, ref) => {
  const t = useI18n();
  const streak = Math.max(data.streaks.currentStreak, data.streaks.longestStreak);
  const isLongest = data.streaks.longestStreak > data.streaks.currentStreak;

  return (
    <div ref={ref} style={{ ...CARD_STYLE, background: GRADIENTS[4] }}>
      <div style={{ fontSize: 14, fontWeight: 600, opacity: 0.9, marginBottom: 16 }}>
        {t("wrapped.streak")}
      </div>
      <div style={{ fontSize: 64, marginBottom: 8 }}>
        🔥
      </div>
      <div style={{ fontSize: 64, fontWeight: 800, lineHeight: 1, letterSpacing: "-3px" }}>
        {streak}
      </div>
      <div style={{ fontSize: 16, fontWeight: 700, marginTop: 8 }}>
        {t("wrapped.streakDays")}
      </div>
      <div style={{ fontSize: 11, fontWeight: 600, opacity: 0.7, marginTop: 8 }}>
        {isLongest ? t("wrapped.longestStreak") : t("wrapped.currentStreak")}
      </div>
      <Watermark />
    </div>
  );
});

// Card 6: Summary
const SummaryCard = forwardRef<HTMLDivElement, CardProps>(({ data }, ref) => {
  const t = useI18n();

  const summaryItems = [
    { label: t("wrapped.summaryTokens"), value: formatTokens(data.totalTokens, "compact") },
    { label: t("wrapped.summaryCost"), value: formatCost(data.totalCost) },
    { label: t("wrapped.summaryMessages"), value: data.totalMessages.toLocaleString() },
    { label: t("wrapped.summarySessions"), value: data.totalSessions.toLocaleString() },
    { label: t("wrapped.summaryStreak"), value: `${data.streaks.currentStreak}d` },
    { label: t("wrapped.summaryCache"), value: `${data.cacheHitRate.toFixed(0)}%` },
  ];

  return (
    <div ref={ref} style={{ ...CARD_STYLE, background: GRADIENTS[5] }}>
      <div style={{ fontSize: 13, fontWeight: 600, opacity: 0.8, marginBottom: 4 }}>
        {data.period}
      </div>
      <div style={{ fontSize: 20, fontWeight: 800, marginBottom: 24 }}>
        {t("wrapped.summary")}
      </div>

      <div style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr",
        gap: 10,
        width: "100%",
      }}>
        {summaryItems.map((item) => (
          <div key={item.label} style={{
            background: "rgba(255,255,255,0.15)",
            borderRadius: 12,
            padding: "10px 8px",
          }}>
            <div style={{ fontSize: 22, fontWeight: 800 }}>
              {item.value}
            </div>
            <div style={{ fontSize: 9, fontWeight: 600, opacity: 0.7, textTransform: "uppercase", marginTop: 2 }}>
              {item.label}
            </div>
          </div>
        ))}
      </div>

      <Watermark />
    </div>
  );
});

function Watermark() {
  return (
    <div style={{
      position: "absolute",
      bottom: 12,
      fontSize: 9,
      fontWeight: 600,
      opacity: 0.4,
      letterSpacing: "0.5px",
    }}>
      AI Token Monitor
    </div>
  );
}

const EmptyCard = forwardRef<HTMLDivElement, { gradient: string }>(({ gradient }, ref) => {
  return <div ref={ref} style={{ ...CARD_STYLE, background: gradient }} />;
});

export const CARDS = [TotalCostCard, TopModelCard, BusiestDayCard, CacheHeroCard, StreakCard, SummaryCard];
export const CARD_COUNT = CARDS.length;
