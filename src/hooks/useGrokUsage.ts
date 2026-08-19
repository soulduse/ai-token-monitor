import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { GrokCredits } from "../lib/types";

export function useGrokUsage(enabled: boolean) {
  const [credits, setCredits] = useState<GrokCredits | null>(null);
  const requestIdRef = useRef(0);

  const fetchCredits = useCallback(async () => {
    if (!enabled) {
      setCredits(null);
      return;
    }
    const requestId = ++requestIdRef.current;
    try {
      const data = await invoke<GrokCredits | null>("get_grok_usage");
      if (requestId === requestIdRef.current) {
        setCredits(data);
      }
    } catch {
      // Keep the last known snapshot on a transient failure.
    }
  }, [enabled]);

  useEffect(() => {
    fetchCredits();
    const unlisten = listen("stats-updated", () => {
      fetchCredits();
    }).catch(() => null);
    return () => {
      unlisten.then((fn) => fn?.());
    };
  }, [fetchCredits]);

  return { credits };
}
