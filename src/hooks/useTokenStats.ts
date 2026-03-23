import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AllStats } from "../lib/types";

interface SourceSelection {
  includeClaude: boolean;
  includeCodex: boolean;
}

export function useTokenStats({ includeClaude, includeCodex }: SourceSelection) {
  const [stats, setStats] = useState<AllStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const hasDataRef = useRef(false);
  const requestIdRef = useRef(0);

  const fetchStats = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const selection = {
        include_claude: includeClaude,
        include_codex: includeCodex,
      };
      const data = await invoke<AllStats>("get_all_stats", { selection });
      if (requestId !== requestIdRef.current) return;
      setStats(data);
      setError(null);
      hasDataRef.current = true;
    } catch (e) {
      if (requestId !== requestIdRef.current) return;
      // Only show error if we never had valid data — keeps last known data on transient failures
      if (!hasDataRef.current) {
        setError(String(e));
      }
    } finally {
      if (requestId !== requestIdRef.current) return;
      setLoading(false);
    }
  }, [includeClaude, includeCodex]);

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
    };
  }, [fetchStats]);

  return { stats, error, loading, refetch: fetchStats };
}
