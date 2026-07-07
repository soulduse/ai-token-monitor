import { useEffect, useRef } from "react";
import { useAuth } from "../hooks/useAuth";
import { restoreServerHistory, clearServerHistory } from "../lib/serverHistory";

// Once per app session per user — login events are rare and the fetch is a few
// hundred rows, so re-running on the next app launch keeps restored history
// reasonably fresh without a persistent flag.
const restoredThisSession = new Set<string>();

/**
 * Headless: after sign-in, pulls the user's daily history from the server so a
 * fresh install / new machine shows past usage immediately. Clears the restored
 * data on sign-out so an account switch never displays another user's history.
 */
export function ServerHistoryRestorer() {
  const { user } = useAuth();
  const prevUserIdRef = useRef<string | null>(null);

  useEffect(() => {
    const prevUserId = prevUserIdRef.current;
    prevUserIdRef.current = user?.id ?? null;

    if (!user) {
      // Only clear on a real sign-out transition — the initial null before the
      // stored session resolves must keep the on-disk data (offline launches
      // would otherwise lose restored history for the whole session).
      if (prevUserId) clearServerHistory().catch(() => {});
      return;
    }
    if (restoredThisSession.has(user.id)) return;
    restoredThisSession.add(user.id);

    restoreServerHistory(user.id)
      .then((n) => {
        if (n > 0) console.info(`[server-history] restored ${n} daily rows`);
      })
      .catch((e) => {
        // Allow a retry on the next login/mount if this attempt failed.
        restoredThisSession.delete(user.id);
        console.warn("[server-history] restore failed:", e);
      });
  }, [user?.id]);

  return null;
}
