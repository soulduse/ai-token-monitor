/**
 * Kiro meters in credits, not tokens, so its leaderboard rows carry a different
 * quantity than every other provider's.
 *
 * `daily_snapshots.total_tokens` is a `bigint`, and a day's Kiro usage is a small
 * fractional number (20.562976 credits, not millions of tokens) — storing it
 * directly would truncate to `20` and throw away almost all of the precision the
 * ranking depends on. So Kiro rows store **millicredits**: credits × 1000, which
 * keeps three decimals inside an integer column with no schema change.
 *
 * This is safe only because the leaderboard is scoped per provider — the ranking
 * RPC filters on `p_provider` and the UI is split into provider tabs — so a Kiro
 * row is never sorted against a token-based one.
 */
import type { LeaderboardProvider } from "./types";

/**
 * USD per credit. Mirrors `providers::kiro::CREDIT_RATE_USD` on the Rust side;
 * both must move together. This is Kiro's overage rate, i.e. the marginal cost
 * of a credit once the monthly plan allotment is spent.
 */
export const KIRO_CREDIT_RATE_USD = 0.04;

/** Scale factor applied before writing credits into the integer column. */
export const KIRO_CREDIT_SCALE = 1000;

export function isCreditProvider(provider: LeaderboardProvider): boolean {
  return provider === "kiro";
}

/**
 * Recover credits from the cost the Rust provider computed. `cost_usd` is the
 * only per-day credit signal that survives into `AllStats`, which has no credit
 * field of its own.
 */
export function creditsFromCostUsd(costUsd: number): number {
  return costUsd / KIRO_CREDIT_RATE_USD;
}

/** Credits → the integer stored in `total_tokens` for Kiro rows. */
export function toMilliCredits(credits: number): number {
  return Math.round(credits * KIRO_CREDIT_SCALE);
}

/** The stored integer → credits, for display. */
export function fromMilliCredits(milliCredits: number): number {
  return milliCredits / KIRO_CREDIT_SCALE;
}

/** Display string for a Kiro leaderboard quantity. */
export function formatCredits(milliCredits: number): string {
  const credits = fromMilliCredits(milliCredits);
  return credits < 1 ? credits.toFixed(3) : credits.toFixed(2);
}
