import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TeamAIUsage } from "../lib/types";

// TeamAI rewrites state.json on every proxied response and re-measures idle
// accounts every 5 minutes; re-reading the local file is cheap, so a short
// poll keeps the table in step without any network traffic of our own.
const POLL_INTERVAL_MS = 30_000;

export function useTeamAIUsage(enabled: boolean) {
  const [usage, setUsage] = useState<TeamAIUsage | null>(null);
  const requestIdRef = useRef(0);

  const refresh = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    if (!enabled) {
      setUsage(null);
      return;
    }
    try {
      const data = await invoke<TeamAIUsage | null>("get_teamai_usage");
      if (requestId === requestIdRef.current) {
        setUsage(data);
      }
    } catch {
      // Keep the last known table on a transient failure.
    }
  }, [enabled]);

  useEffect(() => {
    refresh();
    if (!enabled) return;
    // Poll only: "stats-updated" fires on every JSONL change and would re-read
    // TeamAI's files far more often than its own numbers move.
    const timer = window.setInterval(refresh, POLL_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [enabled, refresh]);

  return { usage, refresh };
}
