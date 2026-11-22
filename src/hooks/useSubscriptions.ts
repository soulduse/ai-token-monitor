import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AllStats, ClaudeSubscription, FxRates } from "../lib/types";
import { useSettings } from "../contexts/SettingsContext";
import { useTeamAIUsage } from "./useTeamAIUsage";
import {
  claudePlanFromSubscription,
  claudePlanFromTeamAI,
  codexPlanFromType,
  planPriceUsd,
  type PlanId,
} from "../lib/plans";
import { currencyToUsd } from "../lib/currency";

export type SubscriptionSource = "override" | "teamai" | "login" | "none";

export interface SubscriptionLine {
  plan: PlanId;
  count: number;
  /** null = 정가 없음(Team/Enterprise 등) — 합계에서 빠진다. */
  monthlyUsd: number | null;
  /** 구독이 끝난 TeamAI 계정. 표시는 하되 합계에서 뺀다. */
  inactive: boolean;
}

export interface DetectedSubscriptions {
  source: SubscriptionSource;
  lines: SubscriptionLine[];
  /** 0 이하 = 구독료를 모름 → 배수 UI를 숨긴다. */
  monthlyUsd: number;
  /** 정가를 모르는 플랜이 섞여 합계가 실제보다 작을 수 있음. */
  hasUnpriced: boolean;
}

function addLine(map: Map<string, SubscriptionLine>, plan: PlanId, inactive: boolean) {
  const key = `${plan}:${inactive}`;
  const line = map.get(key);
  if (line) {
    line.count += 1;
    if (line.monthlyUsd !== null) line.monthlyUsd += planPriceUsd(plan) ?? 0;
    return;
  }
  map.set(key, { plan, count: 1, monthlyUsd: planPriceUsd(plan), inactive });
}

interface TeamAIAccountLike {
  plan: string | null;
  status: string;
}

/**
 * 한 provider의 구독 줄을 더한다. TeamAI 계정이 감지되면 그것을, 없으면 로그인 정보 플랜을 쓴다.
 * TeamAI에서 왔는지 돌려준다.
 */
function addVendorLines(
  map: Map<string, SubscriptionLine>,
  accounts: TeamAIAccountLike[],
  toPlan: (raw: string) => PlanId | null,
  loginPlan: PlanId | null,
): boolean {
  let fromTeamAI = false;
  for (const a of accounts) {
    const plan = a.plan ? toPlan(a.plan) : null;
    if (!plan) continue;
    addLine(map, plan, a.status === "inactive");
    fromTeamAI = true;
  }
  if (!fromTeamAI && loginPlan) addLine(map, loginPlan, false);
  return fromTeamAI;
}

function summarize(source: SubscriptionSource, lines: SubscriptionLine[]): DetectedSubscriptions {
  const active = lines.filter((l) => !l.inactive);
  return {
    source,
    lines,
    monthlyUsd: active.reduce((sum, l) => sum + (l.monthlyUsd ?? 0), 0),
    hasUnpriced: active.some((l) => l.monthlyUsd === null),
  };
}

/**
 * 구독료 결정 순서: 사용자 직접 입력 > TeamAI 계정 목록 > Claude/Codex 로그인 정보.
 * 비용 합계와 범위를 맞추려고 "소스"에서 켜진 provider의 구독만 합산한다.
 */
export function useSubscriptions(stats: AllStats, rates: FxRates | null): DetectedSubscriptions {
  const { prefs } = useSettings();
  const includeClaude = prefs.include_claude;
  const includeCodex = prefs.include_codex;
  const { usage: teamai } = useTeamAIUsage(prefs.include_teamai);
  const [claudeSub, setClaudeSub] = useState<ClaudeSubscription | null>(null);

  useEffect(() => {
    if (!includeClaude) return;
    invoke<ClaudeSubscription | null>("get_claude_subscription")
      .then(setClaudeSub)
      .catch(() => setClaudeSub(null));
  }, [includeClaude]);

  const codexPlanType = stats.rate_limits?.plan_type ?? null;
  const override = prefs.subscription_override;

  return useMemo(() => {
    if (override && override.amount > 0) {
      // 사용자가 입력한 값은 환율이 없어도 버리지 않는다 — 저장 시점 USD 값으로 폴백.
      const usd = currencyToUsd(override.amount, override.currency, rates) ?? override.amount_usd ?? null;
      if (usd !== null) return { source: "override", lines: [], monthlyUsd: usd, hasUnpriced: false };
    }

    // provider별로 TeamAI가 우선, 없으면 그 provider의 로그인 정보. TeamAI에 Claude만 있어도
    // Codex 로그인 구독은 그대로 더한다.
    const map = new Map<string, SubscriptionLine>();
    const claudeFromTeamAI = includeClaude && addVendorLines(
      map, teamai?.claude ?? [], claudePlanFromTeamAI, claudePlanFromSubscription(claudeSub));
    const codexFromTeamAI = includeCodex && addVendorLines(
      map, teamai?.codex ?? [], codexPlanFromType, codexPlanFromType(codexPlanType));
    const fromTeamAI = claudeFromTeamAI || codexFromTeamAI;
    const lines = [...map.values()];
    if (lines.length === 0) return summarize("none", []);
    return summarize(fromTeamAI ? "teamai" : "login", lines);
  }, [override, rates, teamai, includeClaude, includeCodex, claudeSub, codexPlanType]);
}
