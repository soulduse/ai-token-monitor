/**
 * True only in dev builds running under the browser QA mock (?mockTauri=1).
 *
 * Server-write paths (leaderboard snapshot upload / backfill) check this so
 * fixture data can never reach the production Supabase tables while the UI is
 * being exercised in a plain browser. The `import.meta.env.DEV` guard makes
 * this a compile-time `false` in release builds.
 */
export function isMockTauriSession(): boolean {
  return import.meta.env.DEV && Boolean((window as unknown as Record<string, unknown>).__TAURI_MOCK__);
}
