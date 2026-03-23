import { createContext, useContext, useEffect, useState, useCallback, useRef } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { UserPreferences } from "../lib/types";

interface SettingsContextType {
  prefs: UserPreferences;
  updatePrefs: (partial: Partial<UserPreferences>) => void;
  ready: boolean;
}

const defaultPrefs: UserPreferences = {
  number_format: "compact",
  show_tray_cost: true,
  leaderboard_opted_in: false,
  theme: "github",
  color_mode: "system",
  language: "en",
};

const SettingsContext = createContext<SettingsContextType>({
  prefs: defaultPrefs,
  updatePrefs: () => {},
  ready: false,
});

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<UserPreferences>(defaultPrefs);
  const [ready, setReady] = useState(false);
  const skipNextPersist = useRef(true);

  useEffect(() => {
    invoke<UserPreferences>("get_preferences").then((p) => {
      setPrefs(p);
      // Skip the persist effect triggered by this setPrefs
      skipNextPersist.current = true;
      setReady(true);
    }).catch(() => {
      setReady(true);
    });
  }, []);

  // Apply theme to document
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", prefs.theme);
  }, [prefs.theme]);

  // Apply color mode (light/dark/system)
  useEffect(() => {
    const root = document.documentElement;
    const apply = (isDark: boolean) => {
      root.setAttribute("data-color-mode", isDark ? "dark" : "light");
    };

    if (prefs.color_mode === "dark") {
      apply(true);
      return;
    }
    if (prefs.color_mode === "light") {
      apply(false);
      return;
    }
    // system: follow OS preference
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    apply(mq.matches);
    const handler = (e: MediaQueryListEvent) => apply(e.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [prefs.color_mode]);

  // Persist to disk when prefs change
  useEffect(() => {
    if (skipNextPersist.current) {
      skipNextPersist.current = false;
      return;
    }
    if (!ready) return;
    invoke("set_preferences", { prefs }).catch(() => {});
  }, [prefs, ready]);

  const updatePrefs = useCallback((partial: Partial<UserPreferences>) => {
    if (!ready) return; // Block updates until loaded
    setPrefs((prev) => ({ ...prev, ...partial }));
  }, [ready]);

  return (
    <SettingsContext.Provider value={{ prefs, updatePrefs, ready }}>
      {children}
    </SettingsContext.Provider>
  );
}

export function useSettings() {
  return useContext(SettingsContext);
}
