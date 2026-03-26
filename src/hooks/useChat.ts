import { useEffect, useRef, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import type { RealtimeChannel } from "@supabase/supabase-js";

export interface ChatMessage {
  id: string;
  user_id: string;
  content: string;
  created_at: string;
  nickname: string;
  avatar_url: string | null;
}

interface ProfileCache {
  nickname: string;
  avatar_url: string | null;
}

const PAGE_SIZE = 50;
const COOLDOWN_MS = 2000;

export function useChat(userId: string | null) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const [sending, setSending] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const profileCache = useRef<Map<string, ProfileCache>>(new Map());
  const channelRef = useRef<RealtimeChannel | null>(null);
  const lastSendRef = useRef(0);
  const loadingMoreRef = useRef(false);

  // Cache profile from a message
  const cacheProfile = useCallback((msg: ChatMessage) => {
    if (!profileCache.current.has(msg.user_id)) {
      profileCache.current.set(msg.user_id, {
        nickname: msg.nickname,
        avatar_url: msg.avatar_url,
      });
    }
  }, []);

  // Fetch profile for a user_id
  const fetchProfile = useCallback(async (uid: string): Promise<ProfileCache> => {
    const cached = profileCache.current.get(uid);
    if (cached) return cached;

    if (!supabase) return { nickname: "Unknown", avatar_url: null };

    const { data } = await supabase
      .from("profiles")
      .select("nickname, avatar_url")
      .eq("id", uid)
      .single();

    const profile: ProfileCache = {
      nickname: data?.nickname ?? "Unknown",
      avatar_url: data?.avatar_url ?? null,
    };
    profileCache.current.set(uid, profile);
    return profile;
  }, []);

  // Parse raw DB row into ChatMessage
  const parseRow = useCallback((row: {
    id: string;
    user_id: string;
    content: string;
    created_at: string;
    profiles?: { nickname: string; avatar_url: string | null } | { nickname: string; avatar_url: string | null }[];
  }): ChatMessage => {
    const profile = Array.isArray(row.profiles) ? row.profiles[0] : row.profiles;
    return {
      id: row.id,
      user_id: row.user_id,
      content: row.content,
      created_at: row.created_at,
      nickname: profile?.nickname ?? "Unknown",
      avatar_url: profile?.avatar_url ?? null,
    };
  }, []);

  // Initial fetch
  useEffect(() => {
    if (!supabase || !userId) return;

    let cancelled = false;
    (async () => {
      setLoading(true);
      const { data } = await supabase
        .from("chat_messages")
        .select("id, user_id, content, created_at, profiles(nickname, avatar_url)")
        .order("created_at", { ascending: false })
        .limit(PAGE_SIZE);

      if (cancelled) return;

      if (data) {
        const msgs = (data as typeof data).map(parseRow).reverse();
        msgs.forEach(cacheProfile);
        setMessages(msgs);
        setHasMore(data.length === PAGE_SIZE);
      }
      setLoading(false);
    })();

    return () => { cancelled = true; };
  }, [userId, parseRow, cacheProfile]);

  // Realtime subscription
  useEffect(() => {
    if (!supabase || !userId) return;

    const channel = supabase
      .channel("chat_messages_realtime")
      .on("postgres_changes", {
        event: "INSERT",
        schema: "public",
        table: "chat_messages",
      }, async (payload) => {
        const row = payload.new as { id: string; user_id: string; content: string; created_at: string };
        const profile = await fetchProfile(row.user_id);
        const msg: ChatMessage = {
          ...row,
          nickname: profile.nickname,
          avatar_url: profile.avatar_url,
        };
        setMessages((prev) => {
          // Avoid duplicates
          if (prev.some((m) => m.id === msg.id)) return prev;
          return [...prev, msg];
        });
      })
      .on("postgres_changes", {
        event: "DELETE",
        schema: "public",
        table: "chat_messages",
      }, (payload) => {
        const deletedId = (payload.old as { id: string }).id;
        setMessages((prev) => prev.filter((m) => m.id !== deletedId));
      })
      .subscribe();

    channelRef.current = channel;

    return () => {
      channel.unsubscribe();
      channelRef.current = null;
    };
  }, [userId, fetchProfile]);

  // Send message
  const sendMessage = useCallback(async (content: string): Promise<{ error?: string }> => {
    if (!supabase || !userId) return { error: "Not authenticated" };

    const trimmed = content.trim();
    if (!trimmed || trimmed.length > 500) return { error: "Invalid message" };

    // Client-side cooldown
    const now = Date.now();
    if (now - lastSendRef.current < COOLDOWN_MS) {
      return { error: "rate_limited" };
    }

    setSending(true);

    const { error } = await supabase.from("chat_messages").insert({
      user_id: userId,
      content: trimmed,
    });

    setSending(false);

    if (error) {
      // DB trigger raises 'Rate limit exceeded' — PostgREST wraps it in details/hint/message
      const errStr = `${error.message} ${error.details ?? ""} ${error.hint ?? ""}`;
      if (error.code === "P0001" || errStr.includes("Rate limit")) return { error: "rate_limited" };
      return { error: error.message };
    }

    lastSendRef.current = now;
    return {};
  }, [userId]);

  // Delete message (optimistic: Realtime DELETE event removes from UI)
  const deleteMessage = useCallback(async (messageId: string): Promise<{ error?: string }> => {
    if (!supabase) return { error: "Not available" };
    const { error } = await supabase.from("chat_messages").delete().eq("id", messageId);
    if (error) return { error: error.message };
    return {};
  }, []);

  // Load more (older messages) with in-flight guard
  const loadMore = useCallback(async () => {
    if (!supabase || !hasMore || messages.length === 0 || loadingMoreRef.current) return;

    loadingMoreRef.current = true;
    try {
      const oldest = messages[0];
      const { data } = await supabase
        .from("chat_messages")
        .select("id, user_id, content, created_at, profiles(nickname, avatar_url)")
        .lt("created_at", oldest.created_at)
        .order("created_at", { ascending: false })
        .limit(PAGE_SIZE);

      if (data) {
        const older = (data as typeof data).map(parseRow).reverse();
        older.forEach(cacheProfile);
        setMessages((prev) => [...older, ...prev]);
        setHasMore(data.length === PAGE_SIZE);
      }
    } finally {
      loadingMoreRef.current = false;
    }
  }, [hasMore, messages, parseRow, cacheProfile]);

  return { messages, loading, sending, hasMore, sendMessage, deleteMessage, loadMore };
}
