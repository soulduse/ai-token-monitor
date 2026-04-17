import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { supabase } from "../lib/supabase";
import type { LeaderboardProvider } from "../lib/types";
import { toLocalDateStr } from "../lib/format";
import { SNAPSHOT_UPLOADED_EVENT, type SnapshotUploadedDetail } from "./useSnapshotUploader";

export interface LeaderboardEntry {
  user_id: string;
  nickname: string;
  avatar_url: string | null;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

interface UseLeaderboardSyncProps {
  provider: LeaderboardProvider;
  period: LeaderboardPeriod;
  userId?: string;
}

const LEADERBOARD_CACHE_TTL = 30 * 60_000; // 30 minutes
const LEADERBOARD_POLL_INTERVAL = 30 * 60_000; // 30 minutes

export type LeaderboardPeriod = "today" | "week" | "month" | "grid";

export function useLeaderboardSync({ provider, period, userId }: UseLeaderboardSyncProps) {
  const [leaderboard, setLeaderboard] = useState<LeaderboardEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const cacheRef = useRef<{
    data: LeaderboardEntry[];
    fetchedAt: number;
    period: LeaderboardPeriod;
    provider: LeaderboardProvider;
  } | null>(null);

  // Fetch leaderboard data
  const fetchLeaderboard = useCallback(async (forceRefresh = false) => {
    if (!supabase) return;
    // Grid view uses a separate hook (useLeaderboardGrid); skip list fetch.
    if (period === "grid") return;

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
      let dateFrom: string;

      if (period === "today") {
        dateFrom = today;
      } else if (period === "week") {
        const now = new Date();
        const dow = now.getDay();
        const mondayOffset = dow === 0 ? 6 : dow - 1;
        const monday = new Date(now);
        monday.setDate(now.getDate() - mondayOffset);
        dateFrom = toLocalDateStr(monday);
      } else {
        const now = new Date();
        const firstOfMonth = new Date(now.getFullYear(), now.getMonth(), 1);
        dateFrom = toLocalDateStr(firstOfMonth);
      }

      const { data } = await supabase.rpc("get_leaderboard_entries", {
        p_provider: provider,
        p_date_from: dateFrom,
        p_date_to: today,
      });

      if (data) {
        const entries = (data as LeaderboardEntry[]).map((e) => ({
          ...e,
          cost_usd: Number(e.cost_usd),
        }));
        setLeaderboard(entries);
        cacheRef.current = { data: entries, fetchedAt: Date.now(), period, provider };
      }
    } finally {
      setLoading(false);
    }
  }, [period, provider]);

  // When a snapshot upload completes, optimistically patch only the user's own
  // row instead of refetching the whole leaderboard. The next scheduled poll
  // (every 15 min) will reconcile any drift. This avoids ~1 extra RPC per
  // local Claude/Codex write cluster-wide.
  //
  // Scope: only `today` can be updated precisely from a single-day signature.
  // For `week`/`month` the detail values are *today's totals*, so we apply the
  // delta between the row's current "today slice" and the new one — but we
  // don't have per-day breakdown server-side here. The simplest correct
  // behavior is to no-op for non-today periods and let the poll handle it.
  useEffect(() => {
    if (!userId) return;
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<SnapshotUploadedDetail>).detail;
      if (!detail || detail.provider !== provider) return;
      if (period !== "today") return;
      setLeaderboard((prev) => {
        const idx = prev.findIndex((entry) => entry.user_id === userId);
        if (idx < 0) return prev;
        const next = prev.slice();
        next[idx] = {
          ...next[idx],
          total_tokens: detail.total_tokens,
          cost_usd: detail.cost_usd,
          messages: detail.messages,
          sessions: detail.sessions,
        };
        next.sort((a, b) => b.total_tokens - a.total_tokens);
        return next;
      });
    };
    window.addEventListener(SNAPSHOT_UPLOADED_EVENT, handler);
    return () => window.removeEventListener(SNAPSHOT_UPLOADED_EVENT, handler);
  }, [provider, period, userId]);

  // Auto-refresh with visibility-aware polling
  useEffect(() => {
    // Grid view is served by useLeaderboardGrid; skip list polling entirely.
    if (period === "grid") return;

    fetchLeaderboard();

    let intervalId: ReturnType<typeof setInterval> | undefined;

    const startPolling = () => {
      if (intervalId) clearInterval(intervalId);
      intervalId = setInterval(() => fetchLeaderboard(false), LEADERBOARD_POLL_INTERVAL);
    };

    const handleVisibility = () => {
      if (document.hidden) {
        if (intervalId) { clearInterval(intervalId); intervalId = undefined; }
      } else {
        fetchLeaderboard(false);
        startPolling();
      }
    };

    startPolling();
    document.addEventListener("visibilitychange", handleVisibility);

    return () => {
      if (intervalId) clearInterval(intervalId);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [period, fetchLeaderboard]);

  const dateRange = useMemo(() => {
    const now = new Date();
    const today = toLocalDateStr(now);
    if (period === "today") return { from: today, to: today };
    if (period === "week") {
      const dow = now.getDay();
      const mondayOffset = dow === 0 ? 6 : dow - 1;
      const monday = new Date(now);
      monday.setDate(now.getDate() - mondayOffset);
      return { from: toLocalDateStr(monday), to: today };
    }
    if (period === "grid") {
      const weekAgo = new Date(now);
      weekAgo.setDate(now.getDate() - 6);
      return { from: toLocalDateStr(weekAgo), to: today };
    }
    const firstOfMonth = new Date(now.getFullYear(), now.getMonth(), 1);
    return { from: toLocalDateStr(firstOfMonth), to: today };
  }, [period]);

  return { leaderboard, loading, dateRange, refetch: () => fetchLeaderboard(true) };
}
