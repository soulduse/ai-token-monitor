import { useEffect, useRef, useCallback } from "react";
import { supabase } from "../lib/supabase";
import { getOrFetchProfile } from "../lib/profileCache";
import { subscribeChatChannel, type ChatMessageInsertPayload } from "../realtime/chatChannel";
import type { ProfileData } from "../lib/profileCache";

let notificationModule: typeof import("@tauri-apps/plugin-notification") | null = null;
let permissionChecked = false;
let permissionReady = false;

async function ensureNotificationPermission() {
  if (permissionChecked) return;
  permissionChecked = true;
  try {
    if (!notificationModule) {
      notificationModule = await import("@tauri-apps/plugin-notification");
    }
    let granted = await notificationModule.isPermissionGranted();
    if (!granted) {
      const result = await notificationModule.requestPermission();
      granted = result === "granted";
    }
    permissionReady = granted;
  } catch {
    // Notification not available (e.g. dev mode)
  }
}

async function fireNotification(title: string, body: string) {
  if (!permissionReady || !notificationModule) return;
  try {
    notificationModule.sendNotification({ title, body });
  } catch {
    // Silently ignore
  }
}

interface Params {
  isChatActive: boolean;
  currentNickname: string | null;
  currentUserId: string | null;
}

/**
 * Listens for new chat messages via the unified channel manager and fires
 * OS notifications when:
 * 1. Someone @mentions the current user
 * 2. Someone replies to the current user's message
 *
 * Only fires when the chat tab is not active.
 */
export function useChatNotification({ isChatActive, currentNickname, currentUserId }: Params) {
  const isChatActiveRef = useRef(isChatActive);
  isChatActiveRef.current = isChatActive;
  const currentNicknameRef = useRef(currentNickname);
  currentNicknameRef.current = currentNickname;
  const currentUserIdRef = useRef(currentUserId);
  currentUserIdRef.current = currentUserId;

  // Request notification permission once
  useEffect(() => {
    if (currentUserId) {
      ensureNotificationPermission();
    }
  }, [currentUserId]);

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

  const checkMention = useCallback((content: string): boolean => {
    const nick = currentNicknameRef.current;
    if (!nick) return false;
    const re = new RegExp(`@${nick.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![a-zA-Z0-9_-])`, "i");
    return re.test(content);
  }, []);

  const checkReplyToMe = useCallback(async (replyTo: string | null): Promise<boolean> => {
    if (!replyTo || !supabase) return false;
    const uid = currentUserIdRef.current;
    if (!uid) return false;

    const { data } = await supabase
      .from("chat_messages")
      .select("user_id")
      .eq("id", replyTo)
      .single();

    return data?.user_id === uid;
  }, []);

  const fetchProfileRef = useRef(fetchProfile);
  fetchProfileRef.current = fetchProfile;

  useEffect(() => {
    if (!supabase || !currentUserId) return;

    return subscribeChatChannel({
      onMessageInsert: async (row: ChatMessageInsertPayload) => {
        try {
          // Skip own messages
          if (row.user_id === currentUserIdRef.current) return;
          // Skip if chat tab is active
          if (isChatActiveRef.current) return;

          const isMention = checkMention(row.content);
          const isReply = !isMention && (await checkReplyToMe(row.reply_to ?? null));

          if (!isMention && !isReply) return;

          const senderProfile = await fetchProfileRef.current(row.user_id);
          const body = row.content.length > 100
            ? row.content.slice(0, 100) + "..."
            : row.content;

          fireNotification(
            isMention ? `@${senderProfile.nickname}` : senderProfile.nickname,
            body,
          );
        } catch {
          // Silently ignore notification errors
        }
      },
    });
  }, [currentUserId, checkMention, checkReplyToMe]);
}
