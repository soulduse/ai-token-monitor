import { useEffect, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import { subscribeChatChannel, type ChatMessageInsertPayload } from "../realtime/chatChannel";

/**
 * Tracks unread chat message count while the chat tab is not active.
 * Uses the unified chat channel manager (no dedicated Realtime channel).
 */
export function useUnreadChat(isChatActive: boolean, userId: string | null) {
  const [unreadCount, setUnreadCount] = useState(0);

  // Reset when chat tab becomes active
  useEffect(() => {
    if (isChatActive) {
      setUnreadCount(0);
    }
  }, [isChatActive]);

  // Subscribe to the unified channel; ignore own messages.
  useEffect(() => {
    if (!supabase || !userId) return;
    return subscribeChatChannel({
      onMessageInsert: (row: ChatMessageInsertPayload) => {
        if (row.user_id === userId) return;
        setUnreadCount((prev) => prev + 1);
      },
    });
  }, [userId]);

  const resetUnread = useCallback(() => setUnreadCount(0), []);

  return { unreadCount: isChatActive ? 0 : unreadCount, resetUnread };
}
