import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AllStats } from "../lib/types";

export type StatsProvider = "claude" | "codex" | "opencode" | "gemini" | "kimi" | "glm" | "gjc" | "grok" | "kiro" | "omo" | "pi" | "omp" | "agy" | "hermes";

const STATS_COMMANDS: Record<StatsProvider, string> = {
  claude: "get_all_stats",
  codex: "get_codex_stats",
  opencode: "get_opencode_stats",
  gemini: "get_gemini_stats",
  kimi: "get_kimi_stats",
  glm: "get_glm_stats",
  gjc: "get_gjc_stats",
  grok: "get_grok_stats",
  kiro: "get_kiro_stats",
  omo: "get_omo_stats",
  pi: "get_pi_stats",
  omp: "get_omp_stats",
  agy: "get_agy_stats",
  hermes: "get_hermes_stats",
};

/** Backends answer "…stats computation in progress" when another caller holds the parse lock. */
function isComputationInProgress(e: unknown): boolean {
  return String(e).includes("computation in progress");
}

/** How often to ask again while another caller is still running the first parse. */
const IN_PROGRESS_RETRY_MS = 1_500;

export function useTokenStats(provider: StatsProvider = "claude") {
  const [stats, setStats] = useState<AllStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const hasDataRef = useRef(false);
  const requestIdRef = useRef(0);
  // Id of the request whose result is on screen. A slow request (the one doing the
  // first parse) may finish after newer ones; it is only stale if something newer
  // already landed, not merely because a newer request was sent.
  const appliedIdRef = useRef(0);
  const retryTimerRef = useRef<number | undefined>(undefined);
  const fetchRef = useRef<() => Promise<void>>(async () => {});

  const fetchStats = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    try {
      const command = STATS_COMMANDS[provider] ?? STATS_COMMANDS.claude;
      const data = await invoke<AllStats>(command);
      if (requestId < appliedIdRef.current) return;
      appliedIdRef.current = requestId;
      setStats(data);
      setError(null);
      hasDataRef.current = true;
      setLoading(false);
    } catch (e) {
      if (isComputationInProgress(e)) {
        // Not a failure: another caller (e.g. the leaderboard uploader's own hook)
        // is parsing right now. Stay loading — treating this as done is what let
        // views compute from a partial source set at startup — and ask again until
        // the parse lands in the backend cache.
        if (!hasDataRef.current) {
          window.clearTimeout(retryTimerRef.current);
          retryTimerRef.current = window.setTimeout(() => fetchRef.current(), IN_PROGRESS_RETRY_MS);
        }
        return;
      }
      if (requestId < appliedIdRef.current) return;
      // Only show error if we never had valid data — keeps last known data on transient failures
      if (!hasDataRef.current) {
        setError(String(e));
      }
      setLoading(false);
    }
  }, [provider]);

  useEffect(() => {
    fetchRef.current = fetchStats;
  }, [fetchStats]);

  useEffect(() => {
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
      window.clearTimeout(retryTimerRef.current);
    };
  }, [fetchStats]);

  return { stats, error, loading, refetch: fetchStats };
}
