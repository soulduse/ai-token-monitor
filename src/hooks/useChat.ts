import { useEffect, useRef, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import { getCachedProfile, setCachedProfile, getOrFetchProfile } from "../lib/profileCache";
import type { ProfileData } from "../lib/profileCache";
import type { RealtimeChannel } from "@supabase/supabase-js";

export type ReactionType = "like" | "heart" | "dislike";

export interface ChatMessage {
  id: string;
  user_id: string;
  content: string;
  created_at: string;
  nickname: string;
  avatar_url: string | null;
  reply_to: string | null;
  replied_message: { nickname: string; content: string } | null;
}

export interface ReactionMap {
  like: string[];
  heart: string[];
  dislike: string[];
}

const PAGE_SIZE = 50;
const COOLDOWN_MS = 2000;

const emptyReactions = (): ReactionMap => ({ like: [], heart: [], dislike: [] });

export function useChat(userId: string | null, enabled: boolean = true) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [reactions, setReactions] = useState<Map<string, ReactionMap>>(new Map());
  const [loading, setLoading] = useState(enabled);
  const [sending, setSending] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const reactionsRef = useRef(reactions);
  reactionsRef.current = reactions;
  const channelRef = useRef<RealtimeChannel | null>(null);
  const lastSendRef = useRef(0);
  const loadingMoreRef = useRef(false);
  const lastSeenAtRef = useRef<string | null>(null);
  const channelCounterRef = useRef(0);

  // Cache profile from a message (shared module-level cache)
  const cacheProfile = useCallback((msg: ChatMessage) => {
    if (!getCachedProfile(msg.user_id)) {
      setCachedProfile(msg.user_id, {
        nickname: msg.nickname,
        avatar_url: msg.avatar_url,
      });
    }
  }, []);

  // Fetch profile for a user_id (shared module-level cache with in-flight dedup)
  const fetchProfile = useCallback(async (uid: string): Promise<ProfileData> => {
    return getOrFetchProfile(uid, async () => {
      if (!supabase) return { nickname: "Unknown", avatar_url: null };

      const { data } = await supabase
        .from("profiles")
        .select("nickname, avatar_url")
        .eq("id", uid)
        .single();

      return {
        nickname: data?.nickname ?? "Unknown",
        avatar_url: data?.avatar_url ?? null,
      };
    });
  }, []);

  // Parse raw DB row into ChatMessage
  const parseRow = useCallback((row: {
    id: string;
    user_id: string;
    content: string;
    created_at: string;
    reply_to?: string | null;
    profiles?: { nickname: string; avatar_url: string | null } | { nickname: string; avatar_url: string | null }[];
  }): ChatMessage => {
    const profile = Array.isArray(row.profiles) ? row.profiles[0] : row.profiles;
    return {
      id: row.id,
      user_id: row.user_id,
      content: row.content,
      created_at: row.created_at,
      reply_to: row.reply_to ?? null,
      replied_message: null,
      nickname: profile?.nickname ?? "Unknown",
      avatar_url: profile?.avatar_url ?? null,
    };
  }, []);

  // Fetch reactions for a set of message IDs and merge into state
  const fetchReactions = useCallback(async (messageIds: string[]) => {
    if (!supabase || messageIds.length === 0) return;

    const { data } = await supabase
      .from("chat_reactions")
      .select("message_id, user_id, reaction_type")
      .in("message_id", messageIds);

    if (!data) return;

    const map = new Map<string, ReactionMap>();
    for (const row of data) {
      const type = row.reaction_type as ReactionType;
      if (!map.has(row.message_id)) {
        map.set(row.message_id, emptyReactions());
      }
      const entry = map.get(row.message_id)!;
      if (!entry[type].includes(row.user_id)) {
        entry[type].push(row.user_id);
      }
    }

    setReactions((prev) => {
      const next = new Map(prev);
      for (const [mid, r] of map) {
        next.set(mid, r);
      }
      return next;
    });
  }, []);

  // Fetch replied message info for messages with reply_to
  const enrichReplies = useCallback(async (msgs: ChatMessage[]) => {
    if (!supabase) return msgs;

    const replyIds = msgs.filter((m) => m.reply_to).map((m) => m.reply_to!);
    if (replyIds.length === 0) return msgs;

    const uniqueIds = [...new Set(replyIds)];
    const { data } = await supabase
      .from("chat_messages")
      .select("id, content, profiles(nickname)")
      .in("id", uniqueIds);

    if (!data) return msgs;

    const replyMap = new Map<string, { nickname: string; content: string }>();
    for (const row of data as { id: string; content: string; profiles?: { nickname: string } | { nickname: string }[] }[]) {
      const profile = Array.isArray(row.profiles) ? row.profiles[0] : row.profiles;
      replyMap.set(row.id, {
        nickname: profile?.nickname ?? "Unknown",
        content: row.content,
      });
    }

    return msgs.map((m) => {
      if (m.reply_to && replyMap.has(m.reply_to)) {
        return { ...m, replied_message: replyMap.get(m.reply_to)! };
      }
      return m;
    });
  }, []);

  // Stable refs for realtime handlers (avoid subscription recreation)
  const fetchProfileRef = useRef(fetchProfile);
  fetchProfileRef.current = fetchProfile;
  const enrichRepliesRef = useRef(enrichReplies);
  enrichRepliesRef.current = enrichReplies;

  // Initial fetch
  useEffect(() => {
    if (!supabase || !userId || !enabled) return;

    let cancelled = false;
    (async () => {
      setLoading(true);
      const { data } = await supabase
        .from("chat_messages")
        .select("id, user_id, content, created_at, reply_to, profiles(nickname, avatar_url)")
        .order("created_at", { ascending: false })
        .limit(PAGE_SIZE);

      if (cancelled) return;

      if (data) {
        let msgs = (data as typeof data).map(parseRow).reverse();
        msgs.forEach(cacheProfile);
        const ids = msgs.map((m) => m.id);
        const [enriched] = await Promise.all([enrichReplies(msgs), fetchReactions(ids)]);
        if (cancelled) return;
        setMessages(enriched);
        setHasMore(data.length === PAGE_SIZE);
        if (enriched.length > 0) {
          lastSeenAtRef.current = enriched[enriched.length - 1].created_at;
        }
      }
      setLoading(false);
    })();

    return () => { cancelled = true; };
  }, [userId, enabled, parseRow, cacheProfile, enrichReplies, fetchReactions]);

  // Setup realtime channel (extracted for reuse on reconnect)
  const setupChannel = useCallback(() => {
    if (!supabase) return;

    const channel = supabase
      .channel(`chat_realtime_${++channelCounterRef.current}`)
      .on("postgres_changes", {
        event: "INSERT",
        schema: "public",
        table: "chat_messages",
      }, async (payload) => {
        try {
          const row = payload.new as { id: string; user_id: string; content: string; created_at: string; reply_to?: string | null };
          const profile = await fetchProfileRef.current(row.user_id);
          let msg: ChatMessage = {
            ...row,
            reply_to: row.reply_to ?? null,
            replied_message: null,
            nickname: profile.nickname,
            avatar_url: profile.avatar_url,
          };
          if (msg.reply_to) {
            const enriched = await enrichRepliesRef.current([msg]);
            msg = enriched[0];
          }
          setMessages((prev) => {
            if (prev.some((m) => m.id === msg.id)) return prev;
            return [...prev, msg];
          });
          lastSeenAtRef.current = msg.created_at;
        } catch {
          // Silently handled — profileCache returns fallback data on failure
        }
      })
      .on("postgres_changes", {
        event: "DELETE",
        schema: "public",
        table: "chat_messages",
      }, (payload) => {
        const deletedId = (payload.old as { id: string }).id;
        setMessages((prev) => prev.filter((m) => m.id !== deletedId));
        setReactions((prev) => {
          const next = new Map(prev);
          next.delete(deletedId);
          return next;
        });
      })
      // Reactions realtime
      .on("postgres_changes", {
        event: "INSERT",
        schema: "public",
        table: "chat_reactions",
      }, (payload) => {
        const row = payload.new as { message_id: string; user_id: string; reaction_type: string };
        const type = row.reaction_type as ReactionType;
        setReactions((prev) => {
          const next = new Map(prev);
          const entry = next.get(row.message_id) ?? emptyReactions();
          if (!entry[type].includes(row.user_id)) {
            next.set(row.message_id, {
              ...entry,
              [type]: [...entry[type], row.user_id],
            });
          }
          return next;
        });
      })
      .on("postgres_changes", {
        event: "DELETE",
        schema: "public",
        table: "chat_reactions",
      }, (payload) => {
        const row = payload.old as { message_id: string; user_id: string; reaction_type: string };
        const type = row.reaction_type as ReactionType;
        setReactions((prev) => {
          const next = new Map(prev);
          const entry = next.get(row.message_id);
          if (entry) {
            next.set(row.message_id, {
              ...entry,
              [type]: entry[type].filter((uid) => uid !== row.user_id),
            });
          }
          return next;
        });
      })
      .subscribe();

    channelRef.current = channel;
  }, []);

  // Fetch messages missed while window was hidden
  const catchUpMessages = useCallback(async () => {
    if (!supabase || !lastSeenAtRef.current) return;

    const { data } = await supabase
      .from("chat_messages")
      .select("id, user_id, content, created_at, reply_to, profiles(nickname, avatar_url)")
      .gt("created_at", lastSeenAtRef.current)
      .order("created_at", { ascending: true })
      .limit(PAGE_SIZE);

    if (!data || data.length === 0) return;

    let msgs = (data as typeof data).map(parseRow);
    msgs.forEach(cacheProfile);
    const ids = msgs.map((m) => m.id);
    const [enriched] = await Promise.all([enrichReplies(msgs), fetchReactions(ids)]);

    setMessages((prev) => {
      const existingIds = new Set(prev.map((m) => m.id));
      const newMsgs = enriched.filter((m) => !existingIds.has(m.id));
      if (newMsgs.length === 0) return prev;
      return [...prev, ...newMsgs];
    });

    const latest = enriched[enriched.length - 1];
    if (latest && latest.created_at > (lastSeenAtRef.current ?? "")) {
      lastSeenAtRef.current = latest.created_at;
    }
  }, [parseRow, cacheProfile, enrichReplies, fetchReactions]);

  // Realtime subscription for messages + reactions
  useEffect(() => {
    if (!supabase || !userId || !enabled) return;

    setupChannel();

    return () => {
      if (channelRef.current) {
        supabase.removeChannel(channelRef.current);
        channelRef.current = null;
      }
    };
  }, [userId, enabled, setupChannel]);

  // Stable refs for visibility handler (avoid effect re-registration)
  const setupChannelRef = useRef(setupChannel);
  setupChannelRef.current = setupChannel;
  const catchUpMessagesRef = useRef(catchUpMessages);
  catchUpMessagesRef.current = catchUpMessages;

  // Reconnect realtime channel when window becomes visible again
  // (macOS throttles WebSocket heartbeats in hidden windows, causing disconnection)
  useEffect(() => {
    if (!supabase || !userId || !enabled) return;

    const handleVisibility = () => {
      if (document.hidden) return;

      if (channelRef.current) {
        supabase.removeChannel(channelRef.current);
        channelRef.current = null;
      }

      // Subscribe first so live events are captured,
      // then catch up on missed messages (dedup handles overlap)
      setupChannelRef.current();
      catchUpMessagesRef.current();
    };

    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [userId, enabled]);

  // Toggle reaction (uses ref to avoid stale closure)
  const toggleReaction = useCallback(async (messageId: string, type: ReactionType) => {
    if (!supabase || !userId) return;

    const entry = reactionsRef.current.get(messageId) ?? emptyReactions();
    const hasReacted = entry[type].includes(userId);

    if (hasReacted) {
      await supabase
        .from("chat_reactions")
        .delete()
        .eq("message_id", messageId)
        .eq("user_id", userId)
        .eq("reaction_type", type);
    } else {
      await supabase
        .from("chat_reactions")
        .insert({ message_id: messageId, user_id: userId, reaction_type: type });
    }
  }, [userId]);

  // Send message (with optional reply)
  const sendMessage = useCallback(async (content: string, replyTo?: string): Promise<{ error?: string }> => {
    if (!supabase || !userId) return { error: "Not authenticated" };

    const trimmed = content.trim();
    if (!trimmed || trimmed.length > 500) return { error: "Invalid message" };

    // Client-side cooldown
    const now = Date.now();
    if (now - lastSendRef.current < COOLDOWN_MS) {
      return { error: "rate_limited" };
    }

    setSending(true);

    const insertData: { user_id: string; content: string; reply_to?: string } = {
      user_id: userId,
      content: trimmed,
    };
    if (replyTo) insertData.reply_to = replyTo;

    const { error } = await supabase.from("chat_messages").insert(insertData);

    setSending(false);

    if (error) {
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
        .select("id, user_id, content, created_at, reply_to, profiles(nickname, avatar_url)")
        .lt("created_at", oldest.created_at)
        .order("created_at", { ascending: false })
        .limit(PAGE_SIZE);

      if (data) {
        let older = (data as typeof data).map(parseRow).reverse();
        older.forEach(cacheProfile);
        const ids = older.map((m) => m.id);
        const [enriched] = await Promise.all([enrichReplies(older), fetchReactions(ids)]);
        setMessages((prev) => [...enriched, ...prev]);
        setHasMore(data.length === PAGE_SIZE);
      }
    } finally {
      loadingMoreRef.current = false;
    }
  }, [hasMore, messages, parseRow, cacheProfile, enrichReplies, fetchReactions]);

  return { messages, reactions, loading, sending, hasMore, sendMessage, deleteMessage, loadMore, toggleReaction };
}
