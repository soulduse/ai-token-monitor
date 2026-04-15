import type { LeaderboardProvider } from "./types";

export type BackfillRunner = (days?: number) => Promise<boolean>;

type RunnerMap = Partial<Record<LeaderboardProvider, BackfillRunner>>;

let currentRunners: RunnerMap = {};
const listeners = new Set<(runners: RunnerMap) => void>();

export function registerBackfillRunner(runners: RunnerMap): () => void {
  currentRunners = { ...currentRunners, ...runners };
  for (const listener of listeners) listener(currentRunners);
  return () => {
    // Clear only the keys this caller registered.
    const next: RunnerMap = { ...currentRunners };
    for (const key of Object.keys(runners) as LeaderboardProvider[]) {
      if (next[key] === runners[key]) delete next[key];
    }
    currentRunners = next;
    for (const listener of listeners) listener(currentRunners);
  };
}

export function getBackfillRunners(): RunnerMap {
  return currentRunners;
}

export function subscribeBackfillRunners(listener: (runners: RunnerMap) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export const BACKFILL_INITIAL_FLAG = (userId: string, provider: LeaderboardProvider) =>
  `leaderboard_initial_backfill_done_${userId}_${provider}`;

export const BACKFILL_LAST_RUN_KEY = (userId: string) =>
  `leaderboard_last_manual_backfill_${userId}`;

export const MANUAL_BACKFILL_COOLDOWN_MS = 60 * 60 * 1000;
