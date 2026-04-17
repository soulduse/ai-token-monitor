import { useCallback, useEffect, useRef, useState } from "react";
import { supabase } from "../lib/supabase";
import type { LeaderboardProvider } from "../lib/types";
import { toLocalDateStr } from "../lib/format";

export interface GridCell {
  rank: number;
  user_id: string;
  nickname: string;
  avatar_url: string | null;
  total_tokens: number;
}

export interface GridRow {
  date: string; // YYYY-MM-DD, local
  entries: GridCell[];
}

interface UseLeaderboardGridProps {
  provider: LeaderboardProvider;
  enabled: boolean;
  days?: number;
  topN?: number;
}

interface RpcRow {
  date: string;
  rank: number;
  user_id: string;
  nickname: string;
  avatar_url: string | null;
  total_tokens: number;
}

const CACHE_TTL = 30 * 60_000; // 30 minutes — matches useLeaderboardSync
const POLL_INTERVAL = 30 * 60_000;

export function useLeaderboardGrid({
  provider,
  enabled,
  days = 7,
  topN = 10,
}: UseLeaderboardGridProps) {
  const [gridData, setGridData] = useState<GridRow[]>([]);
  const [loading, setLoading] = useState(false);
  const cacheRef = useRef<{
    data: GridRow[];
    fetchedAt: number;
    provider: LeaderboardProvider;
    days: number;
    topN: number;
  } | null>(null);

  const fetchGrid = useCallback(async (forceRefresh = false) => {
    if (!supabase || !enabled) return;

    if (
      !forceRefresh &&
      cacheRef.current &&
      cacheRef.current.provider === provider &&
      cacheRef.current.days === days &&
      cacheRef.current.topN === topN &&
      Date.now() - cacheRef.current.fetchedAt < CACHE_TTL
    ) {
      setGridData(cacheRef.current.data);
      return;
    }

    setLoading(true);
    try {
      const now = new Date();
      const to = toLocalDateStr(now);
      const fromDate = new Date(now);
      fromDate.setDate(now.getDate() - (days - 1));
      const from = toLocalDateStr(fromDate);

      const { data, error } = await supabase.rpc("get_leaderboard_date_grid", {
        p_provider: provider,
        p_date_from: from,
        p_date_to: to,
        p_top_n: topN,
      });

      if (error) {
        console.error("[leaderboard-grid] rpc error:", error);
        return;
      }
      if (!data) {
        console.warn("[leaderboard-grid] rpc returned no data");
        return;
      }
      console.debug("[leaderboard-grid] rpc rows:", (data as RpcRow[]).length, { provider, from, to, topN });

      // Build a full list of `days` dates (desc), even if a date has no rows.
      // This keeps the grid shape stable so the UI doesn't jump.
      const dates: string[] = [];
      for (let i = 0; i < days; i++) {
        const d = new Date(now);
        d.setDate(now.getDate() - i);
        dates.push(toLocalDateStr(d));
      }

      const byDate = new Map<string, GridCell[]>();
      for (const row of data as RpcRow[]) {
        const cell: GridCell = {
          rank: row.rank,
          user_id: row.user_id,
          nickname: row.nickname,
          avatar_url: row.avatar_url,
          total_tokens: Number(row.total_tokens),
        };
        const existing = byDate.get(row.date);
        if (existing) existing.push(cell);
        else byDate.set(row.date, [cell]);
      }

      const rows: GridRow[] = dates.map((date) => ({
        date,
        entries: (byDate.get(date) ?? []).sort((a, b) => a.rank - b.rank),
      }));

      setGridData(rows);
      cacheRef.current = { data: rows, fetchedAt: Date.now(), provider, days, topN };
    } finally {
      setLoading(false);
    }
  }, [provider, enabled, days, topN]);

  // Initial fetch + visibility-aware polling
  useEffect(() => {
    if (!enabled) return;
    fetchGrid();

    let intervalId: ReturnType<typeof setInterval> | undefined;

    const startPolling = () => {
      if (intervalId) clearInterval(intervalId);
      intervalId = setInterval(() => fetchGrid(false), POLL_INTERVAL);
    };

    const handleVisibility = () => {
      if (document.hidden) {
        if (intervalId) { clearInterval(intervalId); intervalId = undefined; }
      } else {
        fetchGrid(false);
        startPolling();
      }
    };

    startPolling();
    document.addEventListener("visibilitychange", handleVisibility);

    return () => {
      if (intervalId) clearInterval(intervalId);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [enabled, fetchGrid]);

  // Always invalidate the cache before refetching. When the grid is not
  // currently enabled (user is on the list tab), fetchGrid() early-returns,
  // but clearing the cache guarantees the next enable→fetch will read fresh
  // data instead of a stale TTL-warm entry.
  const refetch = useCallback(() => {
    cacheRef.current = null;
    return fetchGrid(true);
  }, [fetchGrid]);

  // Grid is a 7-day × topN view — a single day's snapshot change rarely
  // reshuffles the top-N ranking in a way the user notices. Skip the
  // event-driven refetch and let the 15-minute poll reconcile.

  return { gridData, loading, refetch };
}
