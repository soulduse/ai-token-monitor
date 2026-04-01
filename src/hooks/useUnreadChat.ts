import { useEffect, useRef, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import type { RealtimeChannel } from "@supabase/supabase-js";

/**
 * Tracks unread chat message count while the chat tab is not active.
 * Listens to Supabase Realtime INSERT events on chat_messages.
 */
export function useUnreadChat(isChatActive: boolean, userId: string | null) {
  const [unreadCount, setUnreadCount] = useState(0);
  const channelRef = useRef<RealtimeChannel | null>(null);
  const channelCounterRef = useRef(0);

  // Reset when chat tab becomes active
  useEffect(() => {
    if (isChatActive) {
      setUnreadCount(0);
    }
  }, [isChatActive]);

  const setupChannel = useCallback(() => {
    if (!supabase || !userId) return;

    const channel = supabase
      .channel(`chat_unread_badge_${++channelCounterRef.current}`)
      .on("postgres_changes", {
        event: "INSERT",
        schema: "public",
        table: "chat_messages",
      }, (payload) => {
        const row = payload.new as { user_id: string };
        if (row.user_id === userId) return;
        setUnreadCount((prev) => prev + 1);
      })
      .subscribe();

    channelRef.current = channel;
  }, [userId]);

  // Subscribe to new messages
  useEffect(() => {
    if (!supabase || !userId) return;

    setupChannel();

    return () => {
      if (channelRef.current) {
        supabase.removeChannel(channelRef.current);
        channelRef.current = null;
      }
    };
  }, [userId, setupChannel]);

  const setupChannelRef = useRef(setupChannel);
  setupChannelRef.current = setupChannel;

  // Reconnect when window becomes visible again
  useEffect(() => {
    if (!supabase || !userId) return;

    const handleVisibility = () => {
      if (document.hidden) return;

      if (channelRef.current) {
        supabase.removeChannel(channelRef.current);
        channelRef.current = null;
      }
      setupChannelRef.current();
    };

    document.addEventListener("visibilitychange", handleVisibility);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [userId]);

  const resetUnread = useCallback(() => setUnreadCount(0), []);

  return { unreadCount: isChatActive ? 0 : unreadCount, resetUnread };
}
