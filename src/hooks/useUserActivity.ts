import { useEffect, useState } from "react";
import { supabase } from "../lib/supabase";
import type { DailyUsage } from "../lib/types";

interface CacheEntry {
  daily: DailyUsage[];
  fetchedAt: number;
}

const CACHE_TTL = 180_000; // 3 minutes
const cache = new Map<string, CacheEntry>();

export function useUserActivity(userId: string | null) {
  const [daily, setDaily] = useState<DailyUsage[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!userId) {
      setDaily([]);
      setLoading(false);
      setError(false);
      return;
    }

    // Check cache
    const cached = cache.get(userId);
    if (cached && Date.now() - cached.fetchedAt < CACHE_TTL) {
      setDaily(cached.daily);
      setLoading(false);
      setError(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(false);

    // Use security-definer RPC instead of direct SELECT so the query works
    // regardless of daily_snapshots RLS state, and stays consistent with
    // leaderboard_hidden filtering applied elsewhere. Server aggregates across
    // providers, so no client-side byDate loop is needed.
    supabase
      .rpc("get_user_activity", { p_user_id: userId, p_weeks: 8 })
      .then(({ data, error: err }) => {
        if (cancelled) return;

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

        cache.set(userId, { daily: result, fetchedAt: Date.now() });
        setDaily(result);
        setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [userId]);

  return { daily, loading, error };
}
