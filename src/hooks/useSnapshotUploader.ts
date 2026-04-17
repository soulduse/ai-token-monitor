import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { supabase } from "../lib/supabase";
import type { AllStats, LeaderboardProvider } from "../lib/types";
import { getTotalTokens, toLocalDateStr } from "../lib/format";
import type { User } from "@supabase/supabase-js";

interface UseSnapshotUploaderProps {
  stats: AllStats | null;
  user: User | null;
  optedIn: boolean;
  provider: LeaderboardProvider;
}

/**
 * Custom event dispatched after a successful snapshot upload. Leaderboard
 * display hooks listen for this to optimistic-patch the current user's row
 * with the new numbers in `detail` instead of forcing a full refetch.
 */
export const SNAPSHOT_UPLOADED_EVENT = "leaderboard-snapshot-uploaded";

export interface SnapshotUploadedDetail {
  provider: LeaderboardProvider;
  today: string;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

// 15 min auto-upload floor. File watcher fires on every Claude/Codex write,
// which without this gate caused ~37 RPC/min cluster-wide (PR #117).
const MIN_AUTO_UPLOAD_INTERVAL_MS = 15 * 60 * 1000;
const BACKFILL_DAYS = 60;

// Shared caches so multiple uploader instances (e.g. one per provider)
// don't re-derive device IDs.
const stableDeviceIdCache = new Map<string, string>();

interface UploadState {
  lastUploadAt: number;
  lastTodayPayload: string | null;
  lastCleanupAt: number;
}

const uploadStateByKey = new Map<string, UploadState>();

function getUploadState(key: string): UploadState {
  let state = uploadStateByKey.get(key);
  if (!state) {
    state = { lastUploadAt: 0, lastTodayPayload: null, lastCleanupAt: 0 };
    uploadStateByKey.set(key, state);
  }
  return state;
}

interface RowPayload {
  date: string;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

function buildTodayRow(stats: AllStats, today: string): RowPayload | null {
  const todayEntry = stats.daily.find((d) => d.date === today);
  if (!todayEntry) return null;
  return {
    date: today,
    total_tokens: getTotalTokens(todayEntry.tokens),
    cost_usd: todayEntry.cost_usd,
    messages: todayEntry.messages,
    sessions: todayEntry.sessions,
  };
}

function payloadFingerprint(row: RowPayload): string {
  return `${row.total_tokens}|${row.cost_usd}|${row.messages}|${row.sessions}`;
}

function buildStaleDates(stats: AllStats, today: string): string[] {
  const start = new Date();
  start.setDate(start.getDate() - (BACKFILL_DAYS - 1));
  const startStr = toLocalDateStr(start);
  const local = new Set(
    stats.daily.filter((d) => d.date >= startStr && d.date <= today).map((d) => d.date),
  );
  const all: string[] = [];
  const cursor = new Date(startStr);
  while (toLocalDateStr(cursor) <= today) {
    const ds = toLocalDateStr(cursor);
    if (!local.has(ds)) all.push(ds);
    cursor.setDate(cursor.getDate() + 1);
  }
  return all;
}

async function callSyncRpc(
  provider: LeaderboardProvider,
  deviceId: string,
  rows: RowPayload[],
  staleDates: string[],
): Promise<boolean> {
  if (!supabase) return false;
  if (rows.length === 0 && staleDates.length === 0) return false;
  const { error } = await supabase.rpc("sync_device_snapshots", {
    p_provider: provider,
    p_device_id: deviceId,
    p_rows: rows,
    p_stale_dates: staleDates,
  });
  return !error;
}

function dispatchUploaded(provider: LeaderboardProvider, todayRow: RowPayload | null) {
  if (!todayRow) return;
  window.dispatchEvent(
    new CustomEvent<SnapshotUploadedDetail>(SNAPSHOT_UPLOADED_EVENT, {
      detail: {
        provider,
        today: todayRow.date,
        total_tokens: todayRow.total_tokens,
        cost_usd: todayRow.cost_usd,
        messages: todayRow.messages,
        sessions: todayRow.sessions,
      },
    }),
  );
}

/**
 * Uploads the user's *today* snapshot for a given provider to Supabase on a
 * throttled, change-only basis. Past-day backfill is no longer automatic — it
 * runs once per (user, provider) on first leaderboard entry, plus on demand
 * via the `manualBackfill` returned function.
 *
 * Why: Supabase Free/Nano hit the IO Budget because the previous policy
 * uploaded today + 60 days of history every time stats changed (~240 DB ops
 * per call). Restricting auto uploads to today only, with a 15-minute floor
 * and value-change skip, drops per-call ops to ~2 and call frequency by orders
 * of magnitude.
 */
export function useSnapshotUploader({ stats, user, optedIn, provider }: UseSnapshotUploaderProps) {
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const throttleRetryRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const statsRef = useRef<AllStats | null>(null);
  statsRef.current = stats;

  // Resolve stable device id once per user
  useEffect(() => {
    let cancelled = false;

    if (!user) {
      setDeviceId(null);
      return () => { cancelled = true; };
    }

    const cached = stableDeviceIdCache.get(user.id);
    if (cached) {
      setDeviceId(cached);
      return () => { cancelled = true; };
    }

    invoke<string>("get_stable_device_id", { userId: user.id })
      .then((derivedId) => {
        if (cancelled) return;
        stableDeviceIdCache.set(user.id, derivedId);
        setDeviceId(derivedId);
      })
      .catch(() => {
        if (!cancelled) setDeviceId(null);
      });

    return () => { cancelled = true; };
  }, [user?.id]);

  const stateKey = user && deviceId ? `${user.id}:${provider}:${deviceId}` : null;

  // Auto upload: today only, 15min throttle, skipped if values unchanged.
  // If a stats change lands inside the throttle window, we don't drop it —
  // we defer it via `throttleRetryRef` so the very last observed value still
  // makes it to Supabase when the floor expires.
  useEffect(() => {
    if (!supabase || !user || !optedIn || !stats || !deviceId || !stateKey) return;

    const attempt = async () => {
      throttleRetryRef.current = undefined;
      const liveStats = statsRef.current;
      if (!liveStats) return;

      const today = toLocalDateStr(new Date());
      const todayRow = buildTodayRow(liveStats, today);
      if (!todayRow) return;

      const state = getUploadState(stateKey);
      const fingerprint = payloadFingerprint(todayRow);
      const now = Date.now();

      if (state.lastTodayPayload === fingerprint) return;

      const sinceLast = now - state.lastUploadAt;
      if (sinceLast < MIN_AUTO_UPLOAD_INTERVAL_MS) {
        const wait = MIN_AUTO_UPLOAD_INTERVAL_MS - sinceLast;
        if (throttleRetryRef.current) clearTimeout(throttleRetryRef.current);
        throttleRetryRef.current = setTimeout(attempt, wait);
        return;
      }

      // Stale-date cleanup is intentionally skipped on the auto-upload path:
      // a 60-day scan every 24h per (user × provider) was dominating Disk IO
      // on the Nano instance. The 30-day cutoff inside sync_device_snapshots
      // already self-prunes unused device entries, and manualBackfill still
      // runs buildStaleDates when users trigger a full resync.
      const ok = await callSyncRpc(provider, deviceId, [todayRow], []);
      if (ok) {
        state.lastUploadAt = now;
        state.lastTodayPayload = fingerprint;
        dispatchUploaded(provider, todayRow);
      }
    };

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(attempt, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [stats, user, optedIn, provider, deviceId, stateKey]);

  // Clear any pending throttle retry when the uploader unmounts or loses its key
  useEffect(() => {
    return () => {
      if (throttleRetryRef.current) clearTimeout(throttleRetryRef.current);
    };
  }, []);

  // Explicit backfill entry point. Called by:
  //   1. Leaderboard first-visit auto-trigger (one-time, gated by localStorage flag)
  //   2. Manual "Upload my past data" button
  const manualBackfill = useCallback(
    async (days: number = BACKFILL_DAYS): Promise<boolean> => {
      if (!supabase || !user || !optedIn || !stats || !deviceId || !stateKey) return false;
      const today = toLocalDateStr(new Date());
      const start = new Date();
      start.setDate(start.getDate() - (days - 1));
      const startStr = toLocalDateStr(start);

      const rows: RowPayload[] = stats.daily
        .filter((d) => d.date >= startStr && d.date <= today)
        .map((d) => ({
          date: d.date,
          total_tokens: getTotalTokens(d.tokens),
          cost_usd: d.cost_usd,
          messages: d.messages,
          sessions: d.sessions,
        }));

      const staleDates = buildStaleDates(stats, today);
      const ok = await callSyncRpc(provider, deviceId, rows, staleDates);
      if (ok) {
        const state = getUploadState(stateKey);
        state.lastUploadAt = Date.now();
        state.lastCleanupAt = Date.now();
        const todayRow = rows.find((r) => r.date === today) ?? null;
        if (todayRow) state.lastTodayPayload = payloadFingerprint(todayRow);
        dispatchUploaded(provider, todayRow);
      }
      return ok;
    },
    [user, optedIn, stats, deviceId, provider, stateKey],
  );

  return { manualBackfill, deviceId, ready: !!stateKey };
}
