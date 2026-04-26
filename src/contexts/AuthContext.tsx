import { createContext, useContext, useEffect, useState, useCallback, useRef } from "react";
import type { ReactNode } from "react";
import { supabase } from "../lib/supabase";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { User } from "@supabase/supabase-js";

const DEEP_LINK_CALLBACK = "ai-token-monitor://auth/callback";
const AUTH_TIMEOUT_MS = 120_000;

function isProduction(): boolean {
  // Tauri v2: macOS/Linux → "tauri:", Windows → "http:" with host "tauri.localhost"
  const { protocol, hostname } = window.location;
  if (protocol !== "http:") return true;
  return hostname === "tauri.localhost";
}

interface OAuthFallback {
  url: string;
  reason: "open-failed" | "timeout";
}

interface AuthContextType {
  user: User | null;
  profile: { nickname: string; avatar_url: string | null } | null;
  loading: boolean;
  available: boolean;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
  oauthFallback: OAuthFallback | null;
  dismissOauthFallback: () => void;
}

const AuthContext = createContext<AuthContextType>({
  user: null,
  profile: null,
  loading: true,
  available: false,
  signIn: async () => {},
  signOut: async () => {},
  oauthFallback: null,
  dismissOauthFallback: () => {},
});

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [profile, setProfile] = useState<{ nickname: string; avatar_url: string | null } | null>(null);
  const [loading, setLoading] = useState(true);
  const [oauthFallback, setOauthFallback] = useState<OAuthFallback | null>(null);
  const available = supabase !== null;
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const dismissOauthFallback = useCallback(() => {
    setOauthFallback(null);
  }, []);

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

  // Shared handler: extract code from deep-link URL and exchange for session.
  // Uses a Set to prevent duplicate processing of the same auth code.
  const processedCodes = useRef(new Set<string>());
  const handleDeepLinkUrl = useCallback(async (url: string) => {
    if (!supabase || !url.startsWith(DEEP_LINK_CALLBACK)) return;
    let parsed: URL;
    try {
      parsed = new URL(url);
    } catch {
      console.warn("[OAuth] Malformed deep-link URL, ignoring:", url);
      return;
    }
    const code = parsed.searchParams.get("code");
    if (!code) {
      console.warn("[OAuth] Deep-link received but no code param:", url);
      return;
    }
    if (processedCodes.current.has(code)) {
      console.log("[OAuth] Code already processed, skipping:", code);
      return;
    }
    processedCodes.current.add(code);
    try {
      await supabase.auth.exchangeCodeForSession(code);
      await invoke("show_window");
    } catch (err) {
      console.error("[OAuth] Session exchange failed:", err);
      processedCodes.current.delete(code);
    } finally {
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      setLoading(false);
    }
  }, []);

  // Production: listen for deep link OAuth callback (macOS + Windows)
  useEffect(() => {
    if (!isProduction() || !supabase) return;

    let cancelled = false;
    let unlistenDeepLink: (() => void) | undefined;
    let unlistenEvent: (() => void) | undefined;

    // Path 1: deep-link plugin (works on macOS, may not fire on Windows)
    onOpenUrl(async (urls: string[]) => {
      for (const url of urls) {
        await handleDeepLinkUrl(url);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenDeepLink = fn;
    });

    // Path 2: Rust single-instance emit (Windows fallback when Path 1 does not fire)
    listen<string>("deep-link-auth", async (event) => {
      console.log("[OAuth] Received deep-link-auth event:", event.payload);
      await handleDeepLinkUrl(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenEvent = fn;
    });

    // Path 3: cold start (Windows) — check for URL stored before frontend mounted.
    // Delayed slightly to let Path 1/2 listeners register first.
    const pendingTimer = setTimeout(() => {
      invoke<string | null>("get_pending_deep_link").then(async (url) => {
        if (url) {
          console.log("[OAuth] Pending deep-link found:", url);
          await handleDeepLinkUrl(url);
        }
      });
    }, 100);

    return () => {
      cancelled = true;
      clearTimeout(pendingTimer);
      unlistenDeepLink?.();
      unlistenEvent?.();
    };
  }, [handleDeepLinkUrl]);

  // Cleanup auth timeout on unmount
  useEffect(() => {
    return () => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, []);

  // Once authenticated, drop any lingering fallback prompt
  useEffect(() => {
    if (user) setOauthFallback(null);
  }, [user]);

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
    setOauthFallback(null);
    setLoading(true);

    if (!isProduction()) {
      // Dev mode: implicit flow via localhost
      await supabase.auth.signInWithOAuth({
        provider: "github",
        options: { redirectTo: window.location.origin },
      });
      return;
    }

    // Production (macOS + Windows): PKCE + deep link
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

    const authUrl = data.url;

    try {
      await openUrl(authUrl);
    } catch (err) {
      // Browser failed to open (multi-agent env, missing default handler, etc.)
      // Surface URL so user can paste it into a browser manually.
      console.error("[OAuth] openUrl failed, surfacing fallback:", err);
      setOauthFallback({ url: authUrl, reason: "open-failed" });
      setLoading(false);
      return;
    }

    // Timeout: surface fallback URL if deep-link callback never arrives
    // (e.g. another app instance hijacked the ai-token-monitor:// scheme).
    timeoutRef.current = setTimeout(() => {
      setOauthFallback((prev) => prev ?? { url: authUrl, reason: "timeout" });
      setLoading(false);
    }, AUTH_TIMEOUT_MS);
  }, []);

  const signOut = useCallback(async () => {
    if (!supabase) return;
    setLoading(true);
    await supabase.auth.signOut();
    processedCodes.current.clear();
    setUser(null);
    setProfile(null);
    setLoading(false);
  }, []);

  return (
    <AuthContext.Provider value={{ user, profile, loading, available, signIn, signOut, oauthFallback, dismissOauthFallback }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
