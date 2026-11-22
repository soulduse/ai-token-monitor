import type { DailyUsage } from "./types";
import { toLocalDateStr } from "./format";

export type WhatIfPeriodId = "today" | "last7" | "last30" | "thisMonth" | "lastMonth";

const WHAT_IF_PERIODS: WhatIfPeriodId[] = ["today", "last7", "last30", "thisMonth", "lastMonth"];

export interface WhatIfPeriod {
  id: WhatIfPeriodId;
  start: string;
  end: string;
  days: number;
  /** 이 기간이 구독 몇 달 치인지. 지난달 = 1, 이번 달 = 경과일 ÷ 그 달 일수, 나머지 = 일수 ÷ 30. */
  months: number;
  costUsd: number;
  /** 시작일부터 종료일까지 하루 단위 비용(빈 날 0). */
  dailyCostUsd: number[];
}

/** 달력 월이 아닌 기간(오늘·최근 N일)을 구독 개월 수로 바꿀 때 쓰는 기준 일수. */
const MONTH_DAYS = 30;

function daysInMonth(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth() + 1, 0).getDate();
}

function monthsCovered(id: WhatIfPeriodId, days: number, now: Date): number {
  if (id === "lastMonth") return 1;
  if (id === "thisMonth") return days / daysInMonth(now);
  return days / MONTH_DAYS;
}

function addDays(date: Date, n: number): Date {
  const d = new Date(date);
  d.setDate(d.getDate() + n);
  return d;
}

/** 정오 기준 날짜. 자정에 서머타임이 바뀌는 시간대에서도 하루 단위 순회가 날짜를 건너뛰지 않는다. */
function atNoon(y: number, m: number, d: number): Date {
  return new Date(y, m, d, 12);
}

function range(id: WhatIfPeriodId, clock: Date): { start: Date; end: Date } {
  const now = atNoon(clock.getFullYear(), clock.getMonth(), clock.getDate());
  switch (id) {
    case "today":
      return { start: now, end: now };
    case "last7":
      return { start: addDays(now, -6), end: now };
    case "last30":
      return { start: addDays(now, -29), end: now };
    case "thisMonth":
      return { start: atNoon(now.getFullYear(), now.getMonth(), 1), end: now };
    case "lastMonth":
      return {
        start: atNoon(now.getFullYear(), now.getMonth() - 1, 1),
        end: atNoon(now.getFullYear(), now.getMonth(), 0),
      };
  }
}

export function buildPeriods(daily: DailyUsage[], now: Date = new Date()): WhatIfPeriod[] {
  const costByDate = new Map<string, number>();
  for (const d of daily) costByDate.set(d.date, (costByDate.get(d.date) ?? 0) + d.cost_usd);

  return WHAT_IF_PERIODS.map((id) => {
    const { start, end } = range(id, now);
    const dailyCostUsd: number[] = [];
    for (let d = new Date(start); d <= end; d = addDays(d, 1)) {
      dailyCostUsd.push(costByDate.get(toLocalDateStr(d)) ?? 0);
    }
    return {
      id,
      start: toLocalDateStr(start),
      end: toLocalDateStr(end),
      days: dailyCostUsd.length,
      months: monthsCovered(id, dailyCostUsd.length, now),
      costUsd: dailyCostUsd.reduce((a, b) => a + b, 0),
      dailyCostUsd,
    };
  });
}

/** 기간 API 환산 비용 ÷ 같은 기간 구독료. 구독료를 모르거나 사용이 없으면 null. */
export function subscriptionMultiple(period: WhatIfPeriod, monthlyUsd: number): number | null {
  if (monthlyUsd <= 0 || period.costUsd <= 0) return null;
  return period.costUsd / proratedSubscription(period, monthlyUsd);
}

export function proratedSubscription(period: WhatIfPeriod, monthlyUsd: number): number {
  return monthlyUsd * period.months;
}

export function formatMultiple(m: number): string {
  return m >= 10 ? m.toFixed(0) : m.toFixed(1);
}

/** 게이지 채움 비율(로그 스케일, 1×=0%, 50×=100%). */
export function multipleGauge(m: number): number {
  return Math.min(1, Math.log10(Math.max(1, m)) / Math.log10(50));
}

export function multipleEmoji(m: number): string {
  if (m >= 50) return "🚀";
  if (m >= 10) return "🔥";
  if (m >= 2) return "👍";
  return "🙂";
}
