import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AllStats } from "../lib/types";

export type StatsProvider = "claude" | "codex" | "opencode" | "kimi" | "glm" | "gjc" | "grok" | "kiro" | "cursor";

const STATS_COMMANDS: Record<StatsProvider, string> = {
  claude: "get_all_stats",
  codex: "get_codex_stats",
  opencode: "get_opencode_stats",
  kimi: "get_kimi_stats",
  glm: "get_glm_stats",
  gjc: "get_gjc_stats",
  grok: "get_grok_stats",
  kiro: "get_kiro_stats",
  cursor: "get_cursor_stats",
};

export function useTokenStats(
  provider: StatsProvider = "claude",
  enabled = true,
  scopeKey: string = provider,
  invokeArgs?: Record<string, unknown>,
) {
  const [stats, setStats] = useState<AllStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const hasDataRef = useRef(false);
  const requestIdRef = useRef(0);
  const inFlightRef = useRef<Promise<void> | null>(null);

  const fetchStats = useCallback((): Promise<void> => {
    if (!enabled) return Promise.resolve();
    // File events and polling can land while a provider is still parsing.
    // Reuse that request instead of letting a fast stale/error response make
    // the earlier successful parse look obsolete.
    if (inFlightRef.current) return inFlightRef.current;
    const requestId = requestIdRef.current;
    if (!hasDataRef.current) setLoading(true);
    const request = (async () => {
      try {
        const command = STATS_COMMANDS[provider] ?? STATS_COMMANDS.claude;
        const data = await invoke<AllStats>(command, invokeArgs);
        if (requestId !== requestIdRef.current) return;
        setStats(data);
        setError(null);
        setRefreshError(null);
        hasDataRef.current = true;
      } catch (e) {
        if (requestId !== requestIdRef.current) return;
        setRefreshError(String(e));
        // Only show error if we never had valid data — keeps last known data on transient failures
        if (!hasDataRef.current) {
          setError(String(e));
        }
      } finally {
        if (requestId === requestIdRef.current) setLoading(false);
      }
    })();
    const tracked = request.finally(() => {
      if (inFlightRef.current === tracked) inFlightRef.current = null;
    });
    inFlightRef.current = tracked;
    return tracked;
  }, [provider, enabled, scopeKey, invokeArgs]);

  const previousScopeRef = useRef(scopeKey);
  useEffect(() => {
    if (previousScopeRef.current === scopeKey) return;
    previousScopeRef.current = scopeKey;
    requestIdRef.current += 1;
    inFlightRef.current = null;
    hasDataRef.current = false;
    setStats(null);
    setError(null);
    setRefreshError(null);
    setLoading(enabled);
  }, [scopeKey, enabled]);

  useEffect(() => {
    if (!enabled) {
      requestIdRef.current += 1;
      inFlightRef.current = null;
      hasDataRef.current = false;
      setStats(null);
      setError(null);
      setRefreshError(null);
      setLoading(false);
      return;
    }

    fetchStats();

    // Listen for file watcher events
    const unlisten = listen("stats-updated", () => {
      fetchStats();
    }).catch(() => null);

    // Fallback polling every 60s
    const interval = setInterval(fetchStats, 60_000);

    return () => {
      unlisten.then((fn) => fn?.());
      clearInterval(interval);
    };
  }, [fetchStats, enabled]);

  return { stats, error, refreshError, loading, refetch: fetchStats };
}
