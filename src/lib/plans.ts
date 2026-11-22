import type { ClaudeSubscription } from "./types";

/**
 * 구독 플랜 월 정가(USD, 미국 웹 월 결제 기준, 2026-11-22 공식 페이지 확인).
 * "API였다면?" 화면에서 구독료 대비 배수를 계산할 때만 쓴다.
 * null = 좌석 종류·계약에 따라 달라 정가를 특정할 수 없음 → 사용자 직접 입력 유도.
 *
 * - Claude: claude.com/pricing, support.claude.com "What is the Ultra plan"
 *   (2026-10-30 Max 5x → "Max", Max 20x → "Ultra" 이름 변경, 가격 동일)
 * - ChatGPT/Codex: learn.chatgpt.com/docs/pricing (Go $8, Plus $20, Pro $100/$200/$500)
 *   Codex plan_type prolite/pro/promax 의 표시 이름이 "Pro"/"Pro (More)"/"Pro (Max)"
 *   (openai/codex protocol/src/auth.rs) 이므로 각각 Pro 100/200/500 으로 대응시킨다 — 공식
 *   문서에 매핑이 명시되진 않았다.
 */
const PLAN_PRICES_USD = {
  claude_pro: 20,
  claude_max_5x: 100,
  claude_max_20x: 200,
  claude_team: null,
  claude_enterprise: null,
  chatgpt_go: 8,
  chatgpt_plus: 20,
  chatgpt_pro_100: 100,
  chatgpt_pro_200: 200,
  chatgpt_pro_500: 500,
  chatgpt_business: null,
  chatgpt_enterprise: null,
  chatgpt_edu: null,
} as const satisfies Record<string, number | null>;

export type PlanId = keyof typeof PLAN_PRICES_USD;

export const PLAN_LABELS: Record<PlanId, string> = {
  claude_pro: "Claude Pro",
  claude_max_5x: "Claude Max (5x)",
  claude_max_20x: "Claude Ultra (20x)",
  claude_team: "Claude Team",
  claude_enterprise: "Claude Enterprise",
  chatgpt_go: "ChatGPT Go",
  chatgpt_plus: "ChatGPT Plus",
  chatgpt_pro_100: "ChatGPT Pro 100",
  chatgpt_pro_200: "ChatGPT Pro 200",
  chatgpt_pro_500: "ChatGPT Pro 500",
  chatgpt_business: "ChatGPT Business",
  chatgpt_enterprise: "ChatGPT Enterprise",
  chatgpt_edu: "ChatGPT Edu",
};

export function planPriceUsd(id: PlanId): number | null {
  return PLAN_PRICES_USD[id];
}

/**
 * Claude rate-limit tier → plan. "…_max_20x" / "max20x" / "Max 20x" / "ultra" 모두 허용.
 * 배수 없는 "max"는 이름 변경 전후로 뜻이 달라(5x 또는 20x) 추측하지 않는다.
 */
function claudeFromTier(raw: string): PlanId | null {
  const t = raw.toLowerCase().replace(/\s+/g, "");
  if (/20x$/.test(t) || /ultra$/.test(t)) return "claude_max_20x";
  if (/5x$/.test(t)) return "claude_max_5x";
  return null;
}

function claudeFromType(raw: string): PlanId | null {
  switch (raw.toLowerCase()) {
    case "pro":
    case "claude_pro":
      return "claude_pro";
    case "team":
    case "claude_team":
      return "claude_team";
    case "enterprise":
    case "claude_enterprise":
      return "claude_enterprise";
    default:
      return null;
  }
}

/** Claude Code 로그인 정보(subscriptionType/rateLimitTier)에서 플랜 판별. */
export function claudePlanFromSubscription(sub: ClaudeSubscription | null | undefined): PlanId | null {
  if (!sub) return null;
  const byTier = sub.rate_limit_tier ? claudeFromTier(sub.rate_limit_tier) : null;
  if (byTier) return byTier;
  // "max"만 있고 배수를 모르면 5x/20x 중 무엇인지 추측하지 않는다.
  return sub.subscription_type ? claudeFromType(sub.subscription_type) : null;
}

/** TeamAI 표시용 플랜 문자열("Max 20x", "Max", "Pro", "OAuth") → 플랜. */
export function claudePlanFromTeamAI(label: string): PlanId | null {
  return claudeFromTier(label) ?? claudeFromType(label.trim());
}

/** Codex plan_type("plus", "pro", "team", …) 또는 TeamAI 라벨("Plus", "Pro") → 플랜. */
export function codexPlanFromType(raw: string | null | undefined): PlanId | null {
  if (!raw) return null;
  switch (raw.toLowerCase().replace(/[\s_-]+/g, "")) {
    case "go":
      return "chatgpt_go";
    case "plus":
      return "chatgpt_plus";
    case "prolite":
      return "chatgpt_pro_100";
    case "pro":
      return "chatgpt_pro_200";
    case "promax":
      return "chatgpt_pro_500";
    case "team":
    case "business":
      return "chatgpt_business";
    case "enterprise":
      return "chatgpt_enterprise";
    case "edu":
    case "education":
    case "eduplus":
    case "edupro":
      return "chatgpt_edu";
    default:
      // free·알 수 없는 신규 티어는 구독료 0/미상으로 두지 않고 감지에서 뺀다.
      return null;
  }
}
