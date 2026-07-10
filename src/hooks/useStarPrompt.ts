import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getTotalTokens } from "../lib/format";
import { useOnboarding } from "../contexts/OnboardingContext";
import type { DailyUsage } from "../lib/types";

export const REPO_URL = "https://github.com/soulduse/ai-token-monitor";

const STORAGE_KEY = "starPromptState";
const MIN_INSTALL_AGE_MS = 3 * 86400000;
const MIN_ACTIVE_DAYS = 5;
const SHOW_PROBABILITY = 0.15;
const MAX_IMPRESSIONS = 3;
const COOLDOWN_IGNORED_MS = 7 * 86400000;
const COOLDOWN_LATER_MS = 14 * 86400000;
const COOLDOWN_CLOSED_MS = 30 * 86400000;

interface StarPromptState {
  firstSeen: number;
  showCount: number;
  nextEligibleAt: number;
  done: boolean;
}

function loadState(): StarPromptState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as StarPromptState;
  } catch {
    // corrupted state — start over
  }
  const fresh: StarPromptState = {
    firstSeen: Date.now(),
    showCount: 0,
    nextEligibleAt: 0,
    done: false,
  };
  saveState(fresh);
  return fresh;
}

function saveState(state: StarPromptState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // storage unavailable — prompt simply won't persist its caps
  }
}

/** Occasional "star us on GitHub" prompt. Three gates must all pass before it
 *  shows: the user has kept the app ≥3 days AND has ≥5 active usage days
 *  (value experienced), a cooldown from previous impressions has elapsed, and
 *  a per-popover-open dice roll succeeds (the "every now and then" feel).
 *  Clicking the star CTA or reaching 3 total impressions retires it forever. */
export function useStarPrompt(daily: DailyUsage[] | undefined) {
  const [visible, setVisible] = useState(false);
  // Never compete with the onboarding tour for attention.
  const { active: tourActive } = useOnboarding();
  // One dice roll per visibility session, so re-renders while the popover
  // stays open can't re-roll their way past the probability gate.
  const rolledThisSession = useRef(false);

  const activeDays = daily
    ? daily.filter((d) => getTotalTokens(d.tokens) > 0).length
    : 0;

  useEffect(() => {
    const tryShow = () => {
      if (tourActive) return;
      if (visible) return;
      if (document.visibilityState !== "visible") return;
      if (rolledThisSession.current) return;

      const state = loadState();
      const now = Date.now();
      const eligible =
        !state.done &&
        state.showCount < MAX_IMPRESSIONS &&
        now - state.firstSeen >= MIN_INSTALL_AGE_MS &&
        now >= state.nextEligibleAt &&
        activeDays >= MIN_ACTIVE_DAYS;
      // Consume the session's single roll only once eligibility holds, so a
      // pre-stats-load call (activeDays still 0) can't burn it.
      if (!eligible) return;
      rolledThisSession.current = true;
      if (Math.random() >= SHOW_PROBABILITY) return;

      // Record the impression up front: if the user ignores the toast and the
      // popover just closes, the default (shortest) cooldown already applies.
      const showCount = state.showCount + 1;
      saveState({
        ...state,
        showCount,
        nextEligibleAt: now + COOLDOWN_IGNORED_MS,
        done: showCount >= MAX_IMPRESSIONS,
      });
      setVisible(true);
    };

    const onVisibility = () => {
      if (document.visibilityState === "hidden") {
        rolledThisSession.current = false;
        // Auto-dismiss when the popover closes: the impression's cooldown was
        // already recorded, so an ignored toast must not greet the next open.
        setVisible(false);
      } else {
        tryShow();
      }
    };

    tryShow();
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, [visible, activeDays, tourActive]);

  const dismiss = useCallback((cooldownMs: number, done = false) => {
    const state = loadState();
    saveState({
      ...state,
      nextEligibleAt: Date.now() + cooldownMs,
      done: state.done || done,
    });
    setVisible(false);
  }, []);

  const onStar = useCallback(() => {
    openUrl(REPO_URL).catch(() => {});
    dismiss(0, true);
  }, [dismiss]);

  const onLater = useCallback(() => dismiss(COOLDOWN_LATER_MS), [dismiss]);
  const onClose = useCallback(() => dismiss(COOLDOWN_CLOSED_MS), [dismiss]);

  return { visible, onStar, onLater, onClose };
}
