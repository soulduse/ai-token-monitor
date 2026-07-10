import { createContext, useContext, useState, useCallback } from "react";
import type { ReactNode } from "react";

export interface OnboardingState {
  version: 1;
  completedAt?: number;
  skippedAtStep?: number;
}

const STORAGE_KEY = "onboardingState";

/** Any stored record — completed or skipped — means the tour never auto-shows
 *  again; it stays reachable via the Settings replay button. */
export function loadOnboardingState(): OnboardingState | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as OnboardingState;
  } catch {
    // corrupted state — treat as never onboarded
  }
  return null;
}

export function saveOnboardingState(state: OnboardingState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // storage unavailable — the tour may auto-show again next launch
  }
}

interface OnboardingContextType {
  active: boolean;
  start: () => void;
  stop: () => void;
}

const OnboardingContext = createContext<OnboardingContextType>({
  active: false,
  start: () => {},
  stop: () => {},
});

export function OnboardingProvider({ children }: { children: ReactNode }) {
  const [active, setActive] = useState(false);
  const start = useCallback(() => setActive(true), []);
  const stop = useCallback(() => setActive(false), []);

  return (
    <OnboardingContext.Provider value={{ active, start, stop }}>
      {children}
    </OnboardingContext.Provider>
  );
}

export function useOnboarding() {
  return useContext(OnboardingContext);
}
