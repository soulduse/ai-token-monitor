import { useEffect, useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { OAuthUsage } from "../lib/types";

export function useOAuthUsage() {
  const [usage, setUsage] = useState<OAuthUsage | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const requestIdRef = useRef(0);

  const fetchUsage = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    try {
      const data = await invoke<OAuthUsage | null>("get_oauth_usage");
      if (requestId !== requestIdRef.current) return;
      setUsage(data);
    } catch {
      // Ignore errors silently
    } finally {
      if (requestId === requestIdRef.current) {
        setLoading(false);
      }
    }
  }, []);

  const refresh = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setRefreshing(true);
    try {
      const data = await invoke<OAuthUsage | null>("refresh_oauth_usage");
      if (requestId !== requestIdRef.current) return;
      setUsage(data);
    } catch {
      // Ignore errors silently
    } finally {
      if (requestId === requestIdRef.current) {
        setRefreshing(false);
        setLoading(false);
      }
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

  return { usage, loading, refreshing, refresh };
}
