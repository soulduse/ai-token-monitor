import { useTokenStats } from "../hooks/useTokenStats";
import { useSnapshotUploader } from "../hooks/useSnapshotUploader";
import { useAuth } from "../hooks/useAuth";
import { useSettings } from "../contexts/SettingsContext";

/**
 * Headless component that keeps each enabled provider's 60-day snapshot
 * history in sync with Supabase, regardless of which tab the user is
 * currently viewing.
 *
 * This exists because the per-provider upload used to live inside
 * `useLeaderboardSync`, which only mounts when the user opens the
 * Leaderboard tab *and* selects a particular provider. As a result most
 * users only ever uploaded a handful of days, and the MiniProfile "활동
 * (8주)" heatmap showed up empty for anyone who wasn't an active leaderboard
 * visitor. Moving the uploader to the App level fixes that without forcing
 * the Leaderboard UI to mount.
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

  useSnapshotUploader({
    stats: prefs.include_claude ? claudeStats : null,
    user,
    optedIn,
    provider: "claude",
  });
  useSnapshotUploader({
    stats: prefs.include_codex ? codexStats : null,
    user,
    optedIn,
    provider: "codex",
  });
  useSnapshotUploader({
    stats: prefs.include_opencode ? opencodeStats : null,
    user,
    optedIn,
    provider: "opencode",
  });

  return null;
}
