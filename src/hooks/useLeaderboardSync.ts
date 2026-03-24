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
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
  profiles: { nickname: string; avatar_url: string | null }
    | { nickname: string; avatar_url: string | null }[];
}

function toLeaderboardEntry(row: SnapshotRow): LeaderboardEntry {
  const profile = Array.isArray(row.profiles) ? row.profiles[0] : row.profiles;
  return {
    user_id: row.user_id,
    nickname: profile?.nickname ?? "Unknown",
    avatar_url: profile?.avatar_url ?? null,
    total_tokens: row.total_tokens,
    cost_usd: Number(row.cost_usd),
    messages: row.messages,
    sessions: row.sessions,
  };
}

interface UseLeaderboardSyncProps {
  stats: AllStats | null;
  user: User | null;
  optedIn: boolean;
  provider: LeaderboardProvider;
}

const LEADERBOARD_CACHE_TTL = 60_000; // 60 seconds

export function useLeaderboardSync({ stats, user, optedIn, provider }: UseLeaderboardSyncProps) {
  const [leaderboard, setLeaderboard] = useState<LeaderboardEntry[]>([]);
  const [period, setPeriod] = useState<"today" | "week">("today");
  const [loading, setLoading] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  // Track which past days (before today) have already been synced this session, per provider
  const syncedPastDatesRef = useRef<Set<string>>(new Set());
  const cacheRef = useRef<{
    data: LeaderboardEntry[];
    fetchedAt: number;
    period: "today" | "week";
    provider: LeaderboardProvider;
  } | null>(null);

  // Fetch leaderboard data
  const fetchLeaderboard = useCallback(async (forceRefresh = false) => {
    if (!supabase) return;

    // Return cached data if still fresh and period+provider match
    if (
      !forceRefresh &&
      cacheRef.current &&
      cacheRef.current.period === period &&
      cacheRef.current.provider === provider &&
      Date.now() - cacheRef.current.fetchedAt < LEADERBOARD_CACHE_TTL
    ) {
      setLeaderboard(cacheRef.current.data);
      return;
    }

    setLoading(true);

    try {
      const today = toLocalDateStr(new Date());

      if (period === "today") {
        const query = supabase
          .from("daily_snapshots")
          .select("user_id, total_tokens, cost_usd, messages, sessions, profiles(nickname, avatar_url)")
          .eq("date", today)
          .eq("provider", provider)
          .order("total_tokens", { ascending: false })
          .limit(100);

        const { data } = await query;

        if (data) {
          const entries = (data as SnapshotRow[]).map(toLeaderboardEntry);
          setLeaderboard(entries);
          cacheRef.current = { data: entries, fetchedAt: Date.now(), period, provider };
        }
      } else {
        // Weekly: aggregate snapshots from monday to today
        const now = new Date();
        const dow = now.getDay();
        const mondayOffset = dow === 0 ? 6 : dow - 1;
        const monday = new Date(now);
        monday.setDate(now.getDate() - mondayOffset);
        const weekStart = toLocalDateStr(monday);

        const { data } = await supabase
          .from("daily_snapshots")
          .select("user_id, total_tokens, cost_usd, messages, sessions, profiles(nickname, avatar_url)")
          .gte("date", weekStart)
          .lte("date", today)
          .eq("provider", provider)
          .limit(5000);

        if (data) {
          const userMap = new Map<string, LeaderboardEntry>();
          for (const row of (data as SnapshotRow[])) {
            const existing = userMap.get(row.user_id);
            if (existing) {
              existing.total_tokens += row.total_tokens;
              existing.cost_usd += Number(row.cost_usd);
              existing.messages += row.messages;
              existing.sessions += row.sessions;
            } else {
              userMap.set(row.user_id, toLeaderboardEntry(row));
            }
          }
          const sorted = Array.from(userMap.values()).sort((a, b) => b.total_tokens - a.total_tokens);
          setLeaderboard(sorted);
          cacheRef.current = { data: sorted, fetchedAt: Date.now(), period, provider };
        }
      }
    } finally {
      setLoading(false);
    }
  }, [period, provider]);

  // Upload snapshot (debounced), then immediately refresh leaderboard
  useEffect(() => {
    if (!supabase || !user || !optedIn || !stats) return;

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      await uploadSnapshot(user.id, stats, provider, syncedPastDatesRef.current);
      fetchLeaderboard(true);
    }, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [stats, user, optedIn, provider, fetchLeaderboard]);

  // Auto-refresh every 60s (force refresh to bypass cache on interval)
  useEffect(() => {
    fetchLeaderboard();
    const interval = setInterval(() => fetchLeaderboard(true), 60_000);
    return () => clearInterval(interval);
  }, [fetchLeaderboard]);

  return { leaderboard, loading, period, setPeriod, refetch: () => fetchLeaderboard(true) };
}

async function uploadSnapshot(
  userId: string,
  stats: AllStats,
  provider: LeaderboardProvider,
  syncedPastDates: Set<string>,
) {
  if (!supabase) return;

  const now = new Date();
  const today = toLocalDateStr(now);
  const dow = now.getDay();
  const mondayOffset = dow === 0 ? 6 : dow - 1;
  const monday = new Date(now);
  monday.setDate(now.getDate() - mondayOffset);
  const weekStart = toLocalDateStr(monday);

  // Always upload today; upload past days of this week only once per session
  const syncKey = (date: string) => `${provider}:${date}`;
  const toSync = stats.daily.filter(
    (d) => d.date >= weekStart && d.date <= today &&
      (d.date === today || !syncedPastDates.has(syncKey(d.date)))
  );
  if (toSync.length === 0) return;

  const rows = toSync.map((d) => ({
    user_id: userId,
    date: d.date,
    provider,
    total_tokens: getTotalTokens(d.tokens),
    cost_usd: d.cost_usd,
    messages: d.messages,
    sessions: d.sessions,
  }));

  const { error } = await supabase.from("daily_snapshots").upsert(rows, {
    onConflict: "user_id,date,provider",
  });

  // Mark past days as synced so they won't be re-uploaded until next session
  if (!error) {
    toSync.forEach((d) => { if (d.date !== today) syncedPastDates.add(syncKey(d.date)); });
  }
}
