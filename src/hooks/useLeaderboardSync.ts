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
  /** Anchor date (YYYY-MM-DD) for the `today` period — enables browsing past
   *  days. Ignored for other periods; defaults to the actual today. */
  anchorDate?: string;
}

const LEADERBOARD_CACHE_TTL = 30 * 60_000; // 30 minutes
const LEADERBOARD_POLL_INTERVAL = 30 * 60_000; // 30 minutes

export type LeaderboardPeriod = "today" | "week" | "month" | "grid";

export function useLeaderboardSync({ provider, period, userId, anchorDate }: UseLeaderboardSyncProps) {
  const [leaderboard, setLeaderboard] = useState<LeaderboardEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const cacheRef = useRef<{
    data: LeaderboardEntry[];
    fetchedAt: number;
    period: LeaderboardPeriod;
    provider: LeaderboardProvider;
    anchorDate?: string;
  } | null>(null);
  // Monotonic request counter — discards out-of-order RPC responses when the
  // user navigates dates faster than the network answers.
  const requestSeqRef = useRef(0);

  // Fetch leaderboard data
  const fetchLeaderboard = useCallback(async (forceRefresh = false) => {
    if (!supabase) return;
    // Grid view uses a separate hook (useLeaderboardGrid); skip list fetch.
    if (period === "grid") return;

    // Every fetch attempt — including a cache hit — supersedes older in-flight
    // requests, so a stale response can't overwrite a fresher cached view.
    const seq = ++requestSeqRef.current;

    // Return cached data if still fresh and period+provider+anchor match
    if (
      !forceRefresh &&
      cacheRef.current &&
      cacheRef.current.period === period &&
      cacheRef.current.provider === provider &&
      cacheRef.current.anchorDate === anchorDate &&
      Date.now() - cacheRef.current.fetchedAt < LEADERBOARD_CACHE_TTL
    ) {
      setLeaderboard(cacheRef.current.data);
      // The invalidated in-flight request can no longer clear the spinner —
      // this cache hit is the freshest state, so loading is over.
      setLoading(false);
      return;
    }

    setLoading(true);

    try {
      const today = toLocalDateStr(new Date());
      // Never query past today, even with a (theoretically) stale anchor.
      const anchor = anchorDate && anchorDate < today ? anchorDate : today;
      let dateFrom: string;

      if (period === "today") {
        dateFrom = anchor;
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
        p_date_to: period === "today" ? anchor : today,
      });

      // A newer request superseded this one — drop the stale response.
      if (seq !== requestSeqRef.current) return;

      if (data) {
        const entries = (data as LeaderboardEntry[]).map((e) => ({
          ...e,
          cost_usd: Number(e.cost_usd),
        }));
        setLeaderboard(entries);
        cacheRef.current = { data: entries, fetchedAt: Date.now(), period, provider, anchorDate };
      }
    } finally {
      if (seq === requestSeqRef.current) setLoading(false);
    }
  }, [period, provider, anchorDate]);

  // When a snapshot upload completes, force-refetch the leaderboard so the
  // user's row reflects the upload immediately.
  //
  // This used to optimistically patch the user's row with the upload payload
  // instead. That payload is THIS DEVICE's totals only, while the server row
  // merges every device the account uploads from (`device_snapshots`) — so for
  // anyone running the app on two machines the patch visibly collapsed their
  // row to one device's numbers (e.g. $1901 → $161) and then pinned the wrong
  // value into the 30-minute cache. Only the server knows the merged total, so
  // the honest immediate update is a refetch. Cost is bounded: uploads fire at
  // most every 15 min per provider, and this handler only runs while the
  // leaderboard is actually being viewed.
  useEffect(() => {
    if (!userId) return;
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<SnapshotUploadedDetail>).detail;
      if (!detail || detail.provider !== provider) return;
      if (period === "grid") return;
      // For `today`, only refetch when the uploaded day is the one on screen;
      // week/month always contain today, so they refetch unconditionally.
      if (period === "today") {
        const viewed = anchorDate ?? toLocalDateStr(new Date());
        if (detail.today !== viewed) return;
      }
      fetchLeaderboard(true);
    };
    window.addEventListener(SNAPSHOT_UPLOADED_EVENT, handler);
    return () => window.removeEventListener(SNAPSHOT_UPLOADED_EVENT, handler);
  }, [provider, period, userId, anchorDate, fetchLeaderboard]);

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
    if (period === "today") {
      const anchor = anchorDate && anchorDate < today ? anchorDate : today;
      return { from: anchor, to: anchor };
    }
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
  }, [period, anchorDate]);

  return { leaderboard, loading, dateRange, refetch: () => fetchLeaderboard(true) };
}
