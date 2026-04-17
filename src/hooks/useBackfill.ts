import { useCallback, useEffect, useRef, useState } from "react";
import { useAuth } from "./useAuth";
import { useSettings } from "../contexts/SettingsContext";
import {
  BACKFILL_INITIAL_FLAG,
  BACKFILL_LAST_RUN_KEY,
  MANUAL_BACKFILL_COOLDOWN_MS,
  getBackfillRunners,
  subscribeBackfillRunners,
} from "../lib/backfillRegistry";
import type { LeaderboardProvider } from "../lib/types";

const PROVIDERS: LeaderboardProvider[] = ["claude", "codex", "opencode", "kimi", "glm"];

function activeProviders(prefs: {
  include_claude: boolean;
  include_codex: boolean;
  include_opencode: boolean;
  include_kimi: boolean;
  include_glm: boolean;
}): LeaderboardProvider[] {
  return PROVIDERS.filter((p) => {
    if (p === "claude") return prefs.include_claude;
    if (p === "codex") return prefs.include_codex;
    if (p === "opencode") return prefs.include_opencode;
    if (p === "kimi") return prefs.include_kimi;
    return prefs.include_glm;
  });
}

export function useBackfill() {
  const { user } = useAuth();
  const { prefs } = useSettings();
  const [runners, setRunners] = useState(() => getBackfillRunners());
  const [running, setRunning] = useState(false);

  useEffect(() => {
    setRunners(getBackfillRunners());
    return subscribeBackfillRunners(setRunners);
  }, []);

  const runForProvider = useCallback(
    async (provider: LeaderboardProvider, days = 60) => {
      const runner = runners[provider];
      if (!runner) return false;
      return runner(days);
    },
    [runners],
  );

  const runAll = useCallback(
    async (days = 60): Promise<{ ok: number; failed: number }> => {
      if (!user) return { ok: 0, failed: 0 };
      setRunning(true);
      try {
        let ok = 0;
        let failed = 0;
        for (const provider of activeProviders(prefs)) {
          const success = await runForProvider(provider, days);
          if (success) ok += 1;
          else failed += 1;
        }
        if (ok > 0) {
          localStorage.setItem(BACKFILL_LAST_RUN_KEY(user.id), String(Date.now()));
        }
        return { ok, failed };
      } finally {
        setRunning(false);
      }
    },
    [user, prefs, runForProvider],
  );

  // Auto-trigger first-visit backfill once per (user, provider). Skips silently
  // if any provider's runner isn't ready yet — the next mount will retry.
  const initialBackfillRunningRef = useRef(false);
  const ensureInitialBackfill = useCallback(async () => {
    if (!user) return;
    if (initialBackfillRunningRef.current) return;
    const providers = activeProviders(prefs);
    const pending = providers.filter(
      (p) => !localStorage.getItem(BACKFILL_INITIAL_FLAG(user.id, p)) && runners[p],
    );
    if (pending.length === 0) return;

    initialBackfillRunningRef.current = true;
    try {
      for (const provider of pending) {
        const runner = runners[provider];
        if (!runner) continue;
        const success = await runner(60);
        if (success) {
          localStorage.setItem(BACKFILL_INITIAL_FLAG(user.id, provider), String(Date.now()));
        }
      }
    } finally {
      initialBackfillRunningRef.current = false;
    }
  }, [user, prefs, runners]);

  const cooldownRemainingMs = useCallback((): number => {
    if (!user) return 0;
    const last = Number(localStorage.getItem(BACKFILL_LAST_RUN_KEY(user.id)) ?? 0);
    if (!last) return 0;
    const remaining = MANUAL_BACKFILL_COOLDOWN_MS - (Date.now() - last);
    return Math.max(0, remaining);
  }, [user]);

  return {
    running,
    hasRunners: activeProviders(prefs).some((p) => !!runners[p]),
    runAll,
    ensureInitialBackfill,
    cooldownRemainingMs,
  };
}
