import { useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { OAuthUsage, OAuthUsageStatus } from "../lib/types";

export function useOAuthUsage() {
  const [usage, setUsage] = useState<OAuthUsage | null>(null);
  const [status, setStatus] = useState<OAuthUsageStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  // Seconds remaining in a 429 back-off window, or null when refresh is
  // unthrottled. Surfaced so the badge can explain an inert refresh button.
  const [rateLimitRemaining, setRateLimitRemaining] = useState<number | null>(null);
  const requestIdRef = useRef(0);

  const fetchUsage = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    try {
      const [data, nextStatus, rateLimit] = await Promise.all([
        invoke<OAuthUsage | null>("get_oauth_usage"),
        invoke<OAuthUsageStatus>("get_oauth_usage_status").catch(() => null),
        invoke<number | null>("get_oauth_rate_limit_remaining").catch(() => null),
      ]);
      if (requestId === requestIdRef.current) {
        setUsage(data);
        if (nextStatus) setStatus(nextStatus);
        setRateLimitRemaining(rateLimit ?? null);
      }
    } catch {
      // Ignore errors silently
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setRefreshing(true);
    try {
      // Status must be read AFTER the refresh completes — refresh_oauth_usage is
      // an async network call that writes the cache only once it resolves, while
      // get_oauth_usage_status is a synchronous cache read. Running them in
      // parallel would let the status read see the pre-refresh (empty) cache and
      // report "unavailable" even on a successful refresh.
      const data = await invoke<OAuthUsage | null>("refresh_oauth_usage");
      const nextStatus = await invoke<OAuthUsageStatus>("get_oauth_usage_status").catch(() => null);
      const rateLimit = await invoke<number | null>("get_oauth_rate_limit_remaining").catch(() => null);
      // Only adopt the result if no newer request has started; otherwise keep
      // the more recent data. The spinner state is reset unconditionally below.
      if (requestId === requestIdRef.current) {
        setUsage(data);
        if (nextStatus) setStatus(nextStatus);
        setRateLimitRemaining(rateLimit ?? null);
      }
    } catch {
      // Ignore errors silently
    } finally {
      // Always clear the spinner — never tie it to requestId, otherwise a
      // concurrent fetchUsage() (e.g. from a "usage-updated" event fired by
      // this very refresh) bumps requestIdRef and strands refreshing=true.
      setRefreshing(false);
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchUsage();

    // Listen for backend polling updates
    const unlisten = listen("usage-updated", () => {
      fetchUsage();
    }).catch(() => null);

    return () => {
      unlisten.then((fn) => fn?.());
    };
  }, [fetchUsage]);

  return { usage, status, loading, refreshing, rateLimitRemaining, refresh };
}
