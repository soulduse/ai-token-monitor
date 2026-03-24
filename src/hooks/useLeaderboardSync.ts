import { useEffect, useRef, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import type { AllStats, LeaderboardProvider } from "../lib/types";
import { getTotalTokens, toLocalDateStr } from "../lib/format";
import type { User } from "@supabase/supabase-js";

export interface LeaderboardEntry {
  user_id: string;
  nickname: string;
  avatar_url: string | null;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

interface SnapshotRow {
  user_id: string;
  nickname: string;
  avatar_url: string | null;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

function toLeaderboardEntry(row: SnapshotRow): LeaderboardEntry {
  return {
    user_id: row.user_id,
    nickname: row.nickname ?? "Unknown",
    avatar_url: row.avatar_url ?? null,
    total_tokens: row.total_tokens,
    cost_usd: Number(row.cost_usd),
    messages: Number(row.messages),
    sessions: Number(row.sessions),
  };
}

interface UseLeaderboardSyncProps {
  user: User | null;
  optedIn: boolean;
  providers: LeaderboardProvider[];
  providerStats: Partial<Record<LeaderboardProvider, AllStats | null>>;
}

const LEADERBOARD_CACHE_TTL = 60_000; // 60 seconds

export function useLeaderboardSync({ user, optedIn, providers, providerStats }: UseLeaderboardSyncProps) {
  const [leaderboard, setLeaderboard] = useState<LeaderboardEntry[]>([]);
  const [period, setPeriod] = useState<"today" | "week">("today");
  const [loading, setLoading] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const providersKey = providers.slice().sort().join(",");
  const cacheRef = useRef<{
    data: LeaderboardEntry[];
    fetchedAt: number;
    period: "today" | "week";
    providersKey: string;
  } | null>(null);

  // Upload provider-scoped snapshots (debounced)
  useEffect(() => {
    if (!supabase || !user || !optedIn) return;

    const snapshots = buildSnapshots(providerStats);
    if (snapshots.length === 0) return;

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      uploadSnapshots(user.id, snapshots);
    }, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [providerStats.claude, providerStats.codex, user, optedIn]);

  // Fetch leaderboard data
  const fetchLeaderboard = useCallback(async (forceRefresh = false) => {
    if (!supabase || providers.length === 0) {
      setLeaderboard([]);
      return;
    }

    // Return cached data if still fresh and period matches
    if (
      !forceRefresh &&
      cacheRef.current &&
      cacheRef.current.period === period &&
      cacheRef.current.providersKey === providersKey &&
      Date.now() - cacheRef.current.fetchedAt < LEADERBOARD_CACHE_TTL
    ) {
      setLeaderboard(cacheRef.current.data);
      return;
    }

    setLoading(true);

    try {
      const today = toLocalDateStr(new Date());
      const startDate = (() => {
        if (period === "today") return today;

        const now = new Date();
        const dow = now.getDay();
        const mondayOffset = dow === 0 ? 6 : dow - 1;
        const monday = new Date(now);
        monday.setDate(now.getDate() - mondayOffset);
        return toLocalDateStr(monday);
      })();

      const { data, error } = await supabase.rpc("get_leaderboard", {
        p_start_date: startDate,
        p_end_date: today,
        p_providers: providers,
      });

      if (error) {
        console.error("Failed to fetch leaderboard:", error);
        setLeaderboard([]);
        return;
      }

      if (data) {
        const entries = (data as SnapshotRow[]).map(toLeaderboardEntry);
        setLeaderboard(entries);
        cacheRef.current = { data: entries, fetchedAt: Date.now(), period, providersKey };
      }
    } finally {
      setLoading(false);
    }
  }, [period, providers, providersKey]);

  // Auto-refresh every 60s (force refresh to bypass cache on interval)
  useEffect(() => {
    fetchLeaderboard();
    const interval = setInterval(() => fetchLeaderboard(true), 60_000);
    return () => clearInterval(interval);
  }, [fetchLeaderboard]);

  return { leaderboard, loading, period, setPeriod, refetch: () => fetchLeaderboard(true) };
}

function buildSnapshots(providerStats: Partial<Record<LeaderboardProvider, AllStats | null>>) {
  const today = toLocalDateStr(new Date());
  const snapshots: Array<{
    provider: LeaderboardProvider;
    total_tokens: number;
    cost_usd: number;
    messages: number;
    sessions: number;
  }> = [];

  for (const provider of ["claude", "codex"] as const) {
    const stats = providerStats[provider];
    if (!stats) continue;

    const todayData = stats.daily.find((d) => d.date === today);
    if (!todayData) continue;

    snapshots.push({
      provider,
      total_tokens: getTotalTokens(todayData.tokens),
      cost_usd: todayData.cost_usd,
      messages: todayData.messages,
      sessions: todayData.sessions,
    });
  }

  return snapshots;
}

async function uploadSnapshots(
  userId: string,
  snapshots: Array<{
    provider: LeaderboardProvider;
    total_tokens: number;
    cost_usd: number;
    messages: number;
    sessions: number;
  }>
) {
  if (!supabase || snapshots.length === 0) return;

  const today = toLocalDateStr(new Date());

  await supabase.from("daily_snapshots").upsert(
    snapshots.map((snapshot) => ({
      user_id: userId,
      date: today,
      provider: snapshot.provider,
      total_tokens: snapshot.total_tokens,
      cost_usd: snapshot.cost_usd,
      messages: snapshot.messages,
      sessions: snapshot.sessions,
    })),
    { onConflict: "user_id,date,provider" }
  );
}
