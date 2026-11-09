import { useEffect, useState } from "react";
import { supabase } from "../lib/supabase";
import { toLocalDateStr } from "../lib/format";
import type { DailyUsage } from "../lib/types";

interface CacheEntry {
  daily: DailyUsage[];
  streak: number | null;
  fetchedAt: number;
}

const CACHE_TTL = 180_000; // 3 minutes
const cache = new Map<string, CacheEntry>();

export function useUserActivity(userId: string | null) {
  const [daily, setDaily] = useState<DailyUsage[]>([]);
  // Full-history current streak from the server. null = unavailable (RPC
  // failed / not deployed yet) — callers fall back to computing from `daily`.
  const [streak, setStreak] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!userId) {
      setDaily([]);
      setStreak(null);
      setLoading(false);
      setError(false);
      return;
    }

    // Check cache
    const cached = cache.get(userId);
    if (cached && Date.now() - cached.fetchedAt < CACHE_TTL) {
      setDaily(cached.daily);
      setStreak(cached.streak);
      setLoading(false);
      setError(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(false);

    // Use security-definer RPCs instead of direct SELECT so the query works
    // regardless of daily_snapshots RLS state, and stays consistent with
    // leaderboard_hidden filtering applied elsewhere. Server aggregates across
    // providers, so no client-side byDate loop is needed.
    // The heatmap only needs 8 weeks, but the streak must see the full history
    // (deriving it from 8 weeks capped every other user's streak at 56 days),
    // so it comes from a separate scalar RPC.
    Promise.all([
      supabase.rpc("get_user_activity", { p_user_id: userId, p_weeks: 8 }),
      supabase.rpc("get_user_streak", { p_user_id: userId, p_today: toLocalDateStr(new Date()) }),
    ]).then(([activity, streakRes]) => {
      if (cancelled) return;

      const { data, error: err } = activity;
      if (err || !data) {
        setError(true);
        setLoading(false);
        return;
      }

      const result: DailyUsage[] = data.map((row: {
        date: string;
        total_tokens: number | string;
        cost_usd: number | string;
        messages: number;
        sessions: number;
      }) => ({
        date: row.date,
        tokens: { total: Number(row.total_tokens) },
        cost_usd: Number(row.cost_usd),
        messages: row.messages,
        sessions: row.sessions,
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
      }));

      const serverStreak = !streakRes.error && typeof streakRes.data === "number" ? streakRes.data : null;

      cache.set(userId, { daily: result, streak: serverStreak, fetchedAt: Date.now() });
      setDaily(result);
      setStreak(serverStreak);
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [userId]);

  return { daily, streak, loading, error };
}
