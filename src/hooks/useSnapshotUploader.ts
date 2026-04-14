import { useEffect, useRef, useState } from "react";
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
 * display hooks listen for this to update their local state — they no longer
 * force a full refetch, just optimistic-update the current user's row with
 * the new numbers included in `detail`.
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

// Shared caches so multiple uploader instances (e.g. one per provider)
// don't re-derive device IDs or re-upload the same past days.
const stableDeviceIdCache = new Map<string, string>();
const syncedPastDatesPerProvider = new Map<LeaderboardProvider, Set<string>>();

// Throttle RPC calls: skip upload when today's values haven't changed AND the
// last upload was recent. File watcher fires on every Claude/Codex write,
// which without this gate caused ~37 RPC/min cluster-wide.
const MIN_UPLOAD_INTERVAL_MS = 15 * 60 * 1000; // 15 min
const lastUploadPerKey = new Map<string, { sig: string; at: number }>();

type SnapshotPayload = Omit<SnapshotUploadedDetail, "provider">;

function todaySignature(stats: AllStats): { sig: string; payload: SnapshotPayload | null } {
  const today = toLocalDateStr(new Date());
  const t = stats.daily.find((d) => d.date === today);
  if (!t) return { sig: `${today}:empty`, payload: null };
  const total = getTotalTokens(t.tokens);
  const sig = `${today}:${total}:${t.cost_usd}:${t.messages}:${t.sessions}`;
  return {
    sig,
    payload: {
      today,
      total_tokens: total,
      cost_usd: t.cost_usd,
      messages: t.messages,
      sessions: t.sessions,
    },
  };
}

function getSyncedPastDates(provider: LeaderboardProvider): Set<string> {
  let set = syncedPastDatesPerProvider.get(provider);
  if (!set) {
    set = new Set<string>();
    syncedPastDatesPerProvider.set(provider, set);
  }
  return set;
}

/**
 * Uploads the user's daily snapshot for a given provider to Supabase,
 * decoupled from the Leaderboard UI. This hook is mounted at the App level
 * (via `LeaderboardUploader`) so every opted-in provider keeps its 60-day
 * history in sync — even if the user never opens the Leaderboard tab.
 *
 * Why this matters: the MiniProfile heatmap for *other* users reads from
 * `daily_snapshots` via `get_user_activity`. If snapshot upload is gated on
 * Leaderboard tab visits, most users only ever upload a few days and the
 * heatmap shows up empty for everyone else.
 */
export function useSnapshotUploader({ stats, user, optedIn, provider }: UseSnapshotUploaderProps) {
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

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

  // Debounced upload whenever stats change — but only if today's signature
  // actually moved OR MIN_UPLOAD_INTERVAL_MS has passed since the last upload.
  useEffect(() => {
    if (!supabase || !user || !optedIn || !stats || !deviceId) return;

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      const key = `${provider}:${deviceId}`;
      const { sig, payload } = todaySignature(stats);
      const prev = lastUploadPerKey.get(key);
      const changed = !prev || prev.sig !== sig;
      const stale = !prev || Date.now() - prev.at >= MIN_UPLOAD_INTERVAL_MS;
      if (!changed && !stale) return;

      const ok = await uploadSnapshot(stats, provider, deviceId, getSyncedPastDates(provider));
      if (!ok) return;

      lastUploadPerKey.set(key, { sig, at: Date.now() });
      if (payload) {
        window.dispatchEvent(
          new CustomEvent<SnapshotUploadedDetail>(SNAPSHOT_UPLOADED_EVENT, {
            detail: { ...payload, provider },
          }),
        );
      }
    }, 500);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [stats, user, optedIn, provider, deviceId]);
}

async function uploadSnapshot(
  stats: AllStats,
  provider: LeaderboardProvider,
  deviceId: string,
  syncedPastDates: Set<string>,
): Promise<boolean> {
  if (!supabase) return false;

  const now = new Date();
  const today = toLocalDateStr(now);
  // Sync the last 60 days so the MiniProfile 8-week heatmap has full history
  // for *every* user, not just whoever opened the app this month. Past dates
  // are still uploaded only once per session via syncedPastDates, so the
  // wider window only affects the first sync after launch.
  const windowStartDate = new Date(now);
  windowStartDate.setDate(now.getDate() - 59);
  const syncStart = toLocalDateStr(windowStartDate);

  // Build full list of dates in the sync window (last 60 days)
  const allDatesInWindow: string[] = [];
  const cursor = new Date(syncStart);
  while (toLocalDateStr(cursor) <= today) {
    allDatesInWindow.push(toLocalDateStr(cursor));
    cursor.setDate(cursor.getDate() + 1);
  }

  // Dates that have local data within the sync window
  const localDatesInWindow = new Set(
    stats.daily
      .filter((d) => d.date >= syncStart && d.date <= today)
      .map((d) => d.date),
  );

  // Delete stale rows (dates in sync window with no local data)
  const cleanKey = (date: string) => `${provider}:clean:${date}`;
  const staleDates = allDatesInWindow.filter((d) => !localDatesInWindow.has(d));
  const toClean = staleDates.filter((d) => !syncedPastDates.has(cleanKey(d)));

  if (localDatesInWindow.has(today)) {
    syncedPastDates.delete(cleanKey(today));
  }

  // Always upload today; upload past days only once per session
  const syncKey = (date: string) => `${provider}:${date}`;
  const toSync = stats.daily.filter(
    (d) => d.date >= syncStart && d.date <= today &&
      (d.date === today || !syncedPastDates.has(syncKey(d.date))),
  );
  if (toSync.length === 0 && toClean.length === 0) return false;

  const rows = toSync.map((d) => ({
    date: d.date,
    total_tokens: getTotalTokens(d.tokens),
    cost_usd: d.cost_usd,
    messages: d.messages,
    sessions: d.sessions,
  }));

  const { error } = await supabase.rpc("sync_device_snapshots", {
    p_provider: provider,
    p_device_id: deviceId,
    p_rows: rows,
    p_stale_dates: toClean,
  });

  if (error) return false;

  toClean.forEach((d) => syncedPastDates.add(cleanKey(d)));
  toSync.forEach((d) => { if (d.date !== today) syncedPastDates.add(syncKey(d.date)); });
  return true;
}
