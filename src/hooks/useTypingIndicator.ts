import { useEffect, useRef, useState, useCallback } from "react";
import { supabase } from "../lib/supabase";
import type { RealtimeChannel } from "@supabase/supabase-js";

interface TypingUser {
  userId: string;
  nickname: string;
}

const TYPING_TIMEOUT = 3000; // 3s after last keystroke → stop showing
const THROTTLE_MS = 1500; // don't send presence updates more often than this

export function useTypingIndicator(userId: string | null, nickname: string | null, enabled: boolean) {
  const [typingUsers, setTypingUsers] = useState<TypingUser[]>([]);
  const channelRef = useRef<RealtimeChannel | null>(null);
  const lastSentRef = useRef(0);
  const stopTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!supabase || !userId || !nickname || !enabled) return;

    const channel = supabase.channel("chat_typing", {
      config: { presence: { key: userId } },
    });

    channel
      .on("presence", { event: "sync" }, () => {
        const state = channel.presenceState<{ userId: string; nickname: string; typing: boolean }>();
        const users: TypingUser[] = [];
        for (const [, presences] of Object.entries(state)) {
          for (const p of presences) {
            if (p.userId !== userId && p.typing) {
              users.push({ userId: p.userId, nickname: p.nickname });
            }
          }
        }
        setTypingUsers(users);
      })
      .subscribe(async (status) => {
        if (status === "SUBSCRIBED") {
          await channel.track({ userId, nickname, typing: false });
        }
      });

    channelRef.current = channel;

    return () => {
      if (stopTimerRef.current) clearTimeout(stopTimerRef.current);
      if (channelRef.current) {
        supabase.removeChannel(channelRef.current);
        channelRef.current = null;
      }
    };
  }, [userId, nickname, enabled]);

  const sendTyping = useCallback(() => {
    if (!channelRef.current || !userId || !nickname) return;

    const now = Date.now();

    // Reset auto-stop timer on every keystroke
    if (stopTimerRef.current) clearTimeout(stopTimerRef.current);
    stopTimerRef.current = setTimeout(() => {
      channelRef.current?.track({ userId, nickname, typing: false });
    }, TYPING_TIMEOUT);

    // Throttle presence updates
    if (now - lastSentRef.current < THROTTLE_MS) return;

    lastSentRef.current = now;
    channelRef.current.track({ userId, nickname, typing: true });
  }, [userId, nickname]);

  const stopTyping = useCallback(() => {
    const channel = channelRef.current;
    if (!channel || !userId || !nickname) return;
    if (stopTimerRef.current) clearTimeout(stopTimerRef.current);
    channel.track({ userId, nickname, typing: false });
  }, [userId, nickname]);

  return { typingUsers, sendTyping, stopTyping };
}
