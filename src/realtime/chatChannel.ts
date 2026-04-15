import { supabase } from "../lib/supabase";
import type { RealtimeChannel } from "@supabase/supabase-js";

export type ChatMessageInsertPayload = {
  id: string;
  user_id: string;
  content: string;
  created_at: string;
  reply_to?: string | null;
  image_url?: string | null;
};

export type ChatMessageDeletePayload = { id: string };

export type ChatReactionPayload = {
  message_id: string;
  user_id: string;
  reaction_type: string;
};

export interface ChatChannelHandlers {
  onMessageInsert?: (row: ChatMessageInsertPayload) => void;
  onMessageDelete?: (row: ChatMessageDeletePayload) => void;
  onReactionInsert?: (row: ChatReactionPayload) => void;
  onReactionDelete?: (row: ChatReactionPayload) => void;
  onReconnect?: () => void;
}

const subscribers = new Set<ChatChannelHandlers>();
let channel: RealtimeChannel | null = null;
let userId: string | null = null;
let activated = false;
let visibilityHandlerInstalled = false;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

function shouldBeActive(): boolean {
  return Boolean(supabase) && activated && !!userId && !document.hidden;
}

function emit<K extends keyof ChatChannelHandlers>(
  event: K,
  arg: Parameters<NonNullable<ChatChannelHandlers[K]>>[0],
) {
  for (const sub of subscribers) {
    const handler = sub[event] as
      | ((arg: Parameters<NonNullable<ChatChannelHandlers[K]>>[0]) => void)
      | undefined;
    if (handler) {
      try {
        handler(arg);
      } catch {
        // ignore handler errors
      }
    }
  }
}

function emitReconnect() {
  for (const sub of subscribers) {
    try {
      sub.onReconnect?.();
    } catch {
      // ignore
    }
  }
}

function teardown() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  if (channel && supabase) {
    supabase.removeChannel(channel);
  }
  channel = null;
}

function setup(notifyReconnect: boolean) {
  if (!supabase || !shouldBeActive()) return;
  teardown();

  const ch = supabase
    .channel("chat_realtime_unified")
    .on(
      "postgres_changes",
      { event: "INSERT", schema: "public", table: "chat_messages" },
      (payload) => emit("onMessageInsert", payload.new as ChatMessageInsertPayload),
    )
    .on(
      "postgres_changes",
      { event: "DELETE", schema: "public", table: "chat_messages" },
      (payload) => emit("onMessageDelete", payload.old as ChatMessageDeletePayload),
    )
    .on(
      "postgres_changes",
      { event: "INSERT", schema: "public", table: "chat_reactions" },
      (payload) => emit("onReactionInsert", payload.new as ChatReactionPayload),
    )
    .on(
      "postgres_changes",
      { event: "DELETE", schema: "public", table: "chat_reactions" },
      (payload) => emit("onReactionDelete", payload.old as ChatReactionPayload),
    )
    .subscribe((status) => {
      // Guard against stale callbacks from a previous channel that was
      // already torn down — without this, late TIMED_OUT events from the
      // old channel can trigger an endless reconnect loop.
      if (channel !== ch) return;
      if (status === "TIMED_OUT" || status === "CHANNEL_ERROR") {
        if (reconnectTimer) clearTimeout(reconnectTimer);
        reconnectTimer = setTimeout(() => {
          reconnectTimer = null;
          if (shouldBeActive()) {
            setup(true);
          }
        }, 1000);
      }
    });

  channel = ch;
  if (notifyReconnect) emitReconnect();
}

function refresh(notifyReconnect: boolean) {
  if (shouldBeActive()) {
    setup(notifyReconnect);
  } else {
    teardown();
  }
}

function installVisibilityHandler() {
  if (visibilityHandlerInstalled) return;
  visibilityHandlerInstalled = true;
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      teardown();
    } else if (shouldBeActive()) {
      setup(true);
    }
  });
}

export function setChatChannelUser(nextUserId: string | null) {
  if (userId === nextUserId) return;
  userId = nextUserId;
  refresh(true);
}

export function activateChatChannel(active: boolean) {
  if (activated === active) return;
  activated = active;
  refresh(true);
}

export function subscribeChatChannel(handlers: ChatChannelHandlers): () => void {
  installVisibilityHandler();
  subscribers.add(handlers);
  if (subscribers.size === 1) {
    refresh(false);
  }
  return () => {
    subscribers.delete(handlers);
    if (subscribers.size === 0) {
      teardown();
    }
  };
}
