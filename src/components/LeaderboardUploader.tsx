import { useEffect, useMemo } from "react";
import { useTokenStats } from "../hooks/useTokenStats";
import { useSnapshotUploader } from "../hooks/useSnapshotUploader";
import { useAuth } from "../hooks/useAuth";
import { useSettings } from "../contexts/SettingsContext";
import {
  registerBackfillRunner,
  type BackfillRunner,
} from "../lib/backfillRegistry";
import type { LeaderboardProvider } from "../lib/types";

/**
 * Headless component that keeps each enabled provider's *today* snapshot
 * synced with Supabase. Past-day backfill is no longer automatic; instead
 * each uploader exposes a `manualBackfill` runner via the backfill registry,
 * which the leaderboard UI calls on first visit (one-time) and via a
 * "Upload my past data" button.
 *
 * Renders nothing.
 */
export function LeaderboardUploader() {
  const { user } = useAuth();
  const { prefs } = useSettings();
  const optedIn = !!prefs.leaderboard_opted_in;

  // Stats hooks are always called (rules of hooks) but uploads are gated by
  // `optedIn` and each provider's include flag inside `useSnapshotUploader`.
  const { stats: claudeStats } = useTokenStats("claude");
  const { stats: codexStats } = useTokenStats("codex");
  const { stats: opencodeStats } = useTokenStats("opencode");
  const { stats: kimiStats } = useTokenStats("kimi");
  const { stats: glmStats } = useTokenStats("glm");

  const claude = useSnapshotUploader({
    stats: prefs.include_claude ? claudeStats : null,
    user,
    optedIn,
    provider: "claude",
  });
  const codex = useSnapshotUploader({
    stats: prefs.include_codex ? codexStats : null,
    user,
    optedIn,
    provider: "codex",
  });
  const opencode = useSnapshotUploader({
    stats: prefs.include_opencode ? opencodeStats : null,
    user,
    optedIn,
    provider: "opencode",
  });
  const kimi = useSnapshotUploader({
    stats: prefs.include_kimi ? kimiStats : null,
    user,
    optedIn,
    provider: "kimi",
  });
  const glm = useSnapshotUploader({
    stats: prefs.include_glm ? glmStats : null,
    user,
    optedIn,
    provider: "glm",
  });

  const runners = useMemo<Partial<Record<LeaderboardProvider, BackfillRunner>>>(
    () => ({
      claude: prefs.include_claude && claude.ready ? claude.manualBackfill : undefined,
      codex: prefs.include_codex && codex.ready ? codex.manualBackfill : undefined,
      opencode: prefs.include_opencode && opencode.ready ? opencode.manualBackfill : undefined,
      kimi: prefs.include_kimi && kimi.ready ? kimi.manualBackfill : undefined,
      glm: prefs.include_glm && glm.ready ? glm.manualBackfill : undefined,
    }),
    [
      prefs.include_claude,
      prefs.include_codex,
      prefs.include_opencode,
      prefs.include_kimi,
      prefs.include_glm,
      claude.ready,
      codex.ready,
      opencode.ready,
      kimi.ready,
      glm.ready,
      claude.manualBackfill,
      codex.manualBackfill,
      opencode.manualBackfill,
      kimi.manualBackfill,
      glm.manualBackfill,
    ],
  );

  useEffect(() => {
    return registerBackfillRunner(runners);
  }, [runners]);

  return null;
}
