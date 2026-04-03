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

    const eightWeeksAgo = new Date();
    eightWeeksAgo.setDate(eightWeeksAgo.getDate() - 56);
    const dateFrom = eightWeeksAgo.toISOString().slice(0, 10);

    supabase
      .from("daily_snapshots")
      .select("date, total_tokens, cost_usd, messages, sessions")
      .eq("user_id", userId)
      .gte("date", dateFrom)
      .order("date", { ascending: true })
      .then(({ data, error: err }) => {
        if (cancelled) return;

        if (err || !data) {
          setError(true);
          setLoading(false);
          return;
        }

        // Aggregate across providers per date
        const byDate = new Map<string, { tokens: number; cost: number; messages: number; sessions: number }>();
        for (const row of data) {
          const existing = byDate.get(row.date) ?? { tokens: 0, cost: 0, messages: 0, sessions: 0 };
          existing.tokens += Number(row.total_tokens);
          existing.cost += Number(row.cost_usd);
          existing.messages += row.messages;
          existing.sessions += row.sessions;
          byDate.set(row.date, existing);
        }

        const result: DailyUsage[] = Array.from(byDate.entries()).map(([date, agg]) => ({
          date,
          tokens: { total: agg.tokens },
          cost_usd: agg.cost,
          messages: agg.messages,
          sessions: agg.sessions,
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
