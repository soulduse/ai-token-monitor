import { createContext, useContext, useEffect, useState, useCallback, useRef } from "react";
import type { ReactNode } from "react";
import { supabase } from "../lib/supabase";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import type { User } from "@supabase/supabase-js";

const DEEP_LINK_CALLBACK = "ai-token-monitor://auth/callback";
const AUTH_TIMEOUT_MS = 120_000;

function isWindowsProduction(): boolean {
  if (window.location.protocol === "http:") return false;
  return /windows/i.test(navigator.userAgent);
}

interface AuthContextType {
  user: User | null;
  profile: { nickname: string; avatar_url: string | null } | null;
  loading: boolean;
  available: boolean;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
}

const AuthContext = createContext<AuthContextType>({
  user: null,
  profile: null,
  loading: true,
  available: false,
  signIn: async () => {},
  signOut: async () => {},
});

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [profile, setProfile] = useState<{ nickname: string; avatar_url: string | null } | null>(null);
  const [loading, setLoading] = useState(true);
  const available = supabase !== null;
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!supabase) {
      setLoading(false);
      return;
    }

    supabase.auth.getSession().then(({ data: { session } }) => {
      const u = session?.user ?? null;
      setUser(u);
      setLoading(false);
      if (u) upsertProfile(u);
    });

    const { data: { subscription } } = supabase.auth.onAuthStateChange((_event, session) => {
      const u = session?.user ?? null;
      setUser(u);
      if (u) {
        upsertProfile(u);
      } else {
        setProfile(null);
      }
    });

    return () => subscription.unsubscribe();
  }, []);

  // Windows production: listen for deep link OAuth callback
  useEffect(() => {
    if (!isWindowsProduction() || !supabase) return;

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    onOpenUrl(async (urls: string[]) => {
      for (const url of urls) {
        if (!url.startsWith(DEEP_LINK_CALLBACK)) continue;
        const code = new URL(url).searchParams.get("code");
        if (!code) {
          console.warn("[OAuth] Deep-link received but no code param:", url);
          continue;
        }
        try {
          await supabase.auth.exchangeCodeForSession(code);
          // OAuth 성공 — single-instance 콜백에서 스킵한 윈도우 표시를 여기서 수행
          await invoke("show_window");
        } catch (err) {
          console.error("[OAuth] Session exchange failed:", err);
        } finally {
          if (timeoutRef.current) clearTimeout(timeoutRef.current);
          setLoading(false);
        }
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Cleanup auth timeout on unmount
  useEffect(() => {
    return () => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, []);

  const upsertProfile = useCallback(async (u: User) => {
    if (!supabase) return;

    const nickname = u.user_metadata?.user_name
      || u.user_metadata?.preferred_username
      || u.email?.split("@")[0]
      || "Anonymous";
    const avatar_url = u.user_metadata?.avatar_url || null;

    await supabase.from("profiles").upsert({
      id: u.id,
      nickname,
      avatar_url,
    }, { onConflict: "id" });

    setProfile({ nickname, avatar_url });
  }, []);

  const signIn = useCallback(async () => {
    if (!supabase) return;
    if (timeoutRef.current) clearTimeout(timeoutRef.current);
    setLoading(true);

    if (!isWindowsProduction()) {
      // macOS production + dev mode: existing implicit flow
      await supabase.auth.signInWithOAuth({
        provider: "github",
        options: { redirectTo: window.location.origin },
      });
      return;
    }

    // Windows production: PKCE + deep link
    const { data, error } = await supabase.auth.signInWithOAuth({
      provider: "github",
      options: {
        skipBrowserRedirect: true,
        redirectTo: DEEP_LINK_CALLBACK,
      },
    });

    if (error || !data.url) {
      console.error("OAuth error:", error);
      setLoading(false);
      return;
    }

    await openUrl(data.url);

    // Timeout: reset loading if no callback received
    timeoutRef.current = setTimeout(() => setLoading(false), AUTH_TIMEOUT_MS);
  }, []);

  const signOut = useCallback(async () => {
    if (!supabase) return;
    setLoading(true);
    await supabase.auth.signOut();
    setUser(null);
    setProfile(null);
    setLoading(false);
  }, []);

  return (
    <AuthContext.Provider value={{ user, profile, loading, available, signIn, signOut }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
