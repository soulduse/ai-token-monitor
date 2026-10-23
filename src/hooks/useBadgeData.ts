import { useMemo } from "react";
import type { LeaderboardEntry } from "./useLeaderboardSync";
import type { LeaderboardView } from "../lib/types";

export interface BadgeData {
  nickname: string;
  avatarUrl: string | null;
  rank: number;
  totalEntries: number;
  totalTokens: number;
  costUsd: number;
  messages: number;
  provider: LeaderboardView;
  period: "today" | "week" | "month";
  dateRange: { from: string; to: string };
}

export function useBadgeData(
  leaderboard: LeaderboardEntry[],
  userId: string | undefined,
  provider: LeaderboardView,
  period: "today" | "week" | "month",
  dateRange: { from: string; to: string },
): BadgeData | null {
  return useMemo(() => {
    if (!userId || leaderboard.length === 0) return null;

    const index = leaderboard.findIndex((e) => e.user_id === userId);
    if (index === -1) return null;

    const entry = leaderboard[index];
    return {
      nickname: entry.nickname,
      avatarUrl: entry.avatar_url,
      rank: index + 1,
      totalEntries: leaderboard.length,
      totalTokens: entry.total_tokens,
      costUsd: entry.cost_usd,
      messages: entry.messages,
      provider,
      period,
      dateRange,
    };
  }, [leaderboard, userId, provider, period, dateRange]);
}
