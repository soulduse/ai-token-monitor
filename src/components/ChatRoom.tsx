import { useState, useRef, useEffect, useCallback } from "react";
import { useAuth } from "../hooks/useAuth";
import { useSettings } from "../contexts/SettingsContext";
import { useChat } from "../hooks/useChat";
import { ChatMessageRow, DateSeparator, formatDateSeparator } from "./ChatMessage";
import { useI18n } from "../i18n/I18nContext";
import type { ChatMessage } from "../hooks/useChat";

export function ChatRoom() {
  const { user, loading: authLoading, signIn, available } = useAuth();
  const { prefs } = useSettings();
  const t = useI18n();

  if (!available) {
    return (
      <div style={{
        background: "var(--bg-card)",
        borderRadius: "var(--radius-lg)",
        padding: 24,
        boxShadow: "var(--shadow-card)",
        textAlign: "center",
      }}>
        <div style={{ fontSize: 32, marginBottom: 8 }}>💬</div>
        <div style={{ fontSize: 14, fontWeight: 700, color: "var(--text-primary)", marginBottom: 4 }}>
          {t("leaderboard.comingSoon")}
        </div>
        <div style={{ fontSize: 11, color: "var(--text-secondary)", fontWeight: 600 }}>
          {t("leaderboard.notConfigured")}
        </div>
      </div>
    );
  }

  if (!user || !prefs.leaderboard_opted_in) {
    return <ChatCTA onSignIn={signIn} loading={authLoading} hasUser={!!user} />;
  }

  return <ChatContent userId={user.id} />;
}

function ChatCTA({
  onSignIn,
  loading,
  hasUser,
}: {
  onSignIn: () => void;
  loading: boolean;
  hasUser: boolean;
}) {
  const { updatePrefs } = useSettings();
  const t = useI18n();

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 24,
      boxShadow: "var(--shadow-card)",
      textAlign: "center",
      display: "flex",
      flexDirection: "column",
      alignItems: "center",
      gap: 12,
    }}>
      <div style={{ fontSize: 40 }}>💬</div>
      <div style={{ fontSize: 16, fontWeight: 800, color: "var(--text-primary)" }}>
        {t("chat.join")}
      </div>
      <div style={{
        fontSize: 12,
        color: "var(--text-secondary)",
        fontWeight: 600,
        maxWidth: 260,
        lineHeight: 1.5,
      }}>
        {t("chat.description")}
      </div>

      {hasUser ? (
        <button
          onClick={() => updatePrefs({ leaderboard_opted_in: true })}
          style={{
            padding: "10px 24px",
            fontSize: 13,
            fontWeight: 700,
            border: "none",
            borderRadius: "var(--radius-sm)",
            cursor: "pointer",
            background: "linear-gradient(135deg, var(--accent-purple), var(--accent-pink, #c084fc))",
            color: "#fff",
          }}
        >
          {t("leaderboard.enable")}
        </button>
      ) : (
        <button
          onClick={onSignIn}
          disabled={loading}
          style={{
            padding: "10px 24px",
            fontSize: 13,
            fontWeight: 700,
            border: "none",
            borderRadius: "var(--radius-sm)",
            cursor: loading ? "wait" : "pointer",
            background: "#24292e",
            color: "#fff",
            display: "flex",
            alignItems: "center",
            gap: 8,
            opacity: loading ? 0.7 : 1,
          }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/>
          </svg>
          {t("leaderboard.signIn")}
        </button>
      )}
    </div>
  );
}

function ChatContent({ userId }: { userId: string }) {
  const t = useI18n();
  const { prefs } = useSettings();
  const { messages, loading, sending, hasMore, sendMessage, deleteMessage, loadMore } = useChat(userId);
  const [input, setInput] = useState("");
  const [rateLimited, setRateLimited] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const prevMessagesLenRef = useRef(0);

  // Auto-scroll to bottom on new messages (only if user is at bottom)
  useEffect(() => {
    if (messages.length > prevMessagesLenRef.current && isAtBottomRef.current) {
      scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
    }
    prevMessagesLenRef.current = messages.length;
  }, [messages.length]);

  // Initial scroll to bottom
  useEffect(() => {
    if (!loading && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [loading]);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    isAtBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  }, []);

  const handleSend = useCallback(async () => {
    if (!input.trim() || sending) return;

    const result = await sendMessage(input);
    if (result.error === "rate_limited") {
      setRateLimited(true);
      setTimeout(() => setRateLimited(false), 3000);
      return;
    }
    if (!result.error) {
      setInput("");
      // Scroll to bottom after sending
      setTimeout(() => {
        scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
      }, 100);
    }
  }, [input, sending, sendMessage]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }, [handleSend]);

  // Group messages for KakaoTalk-style rendering
  const grouped = groupMessages(messages, prefs.language ?? "en");

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      background: "var(--heat-0)",
      borderRadius: "var(--radius-lg)",
      overflow: "hidden",
      height: 420,
      boxShadow: "var(--shadow-card)",
    }}>
      {/* Messages area */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        style={{
          flex: 1,
          overflowY: "auto",
          paddingTop: 8,
          paddingBottom: 8,
        }}
      >
        {/* Load more button */}
        {hasMore && !loading && (
          <div style={{ textAlign: "center", padding: "8px 0" }}>
            <button
              onClick={loadMore}
              style={{
                background: "none",
                border: "none",
                color: "var(--text-secondary)",
                fontSize: 11,
                fontWeight: 600,
                cursor: "pointer",
                padding: "4px 12px",
              }}
            >
              {t("chat.loadMore")}
            </button>
          </div>
        )}

        {loading ? (
          <div style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            color: "var(--text-secondary)",
            fontSize: 12,
            fontWeight: 600,
          }}>
            {t("chat.loading")}
          </div>
        ) : messages.length === 0 ? (
          <div style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            color: "var(--text-secondary)",
            fontSize: 12,
            fontWeight: 600,
            gap: 8,
          }}>
            <div style={{ fontSize: 32 }}>💬</div>
            {t("chat.empty")}
          </div>
        ) : (
          grouped.map((item) => {
            if (item.type === "date") {
              return <DateSeparator key={`date-${item.label}`} label={item.label} />;
            }
            return (
              <ChatMessageRow
                key={item.message.id}
                message={item.message}
                isMe={item.message.user_id === userId}
                showAvatar={item.showAvatar}
                showNickname={item.showNickname}
                onDelete={item.message.user_id === userId ? deleteMessage : undefined}
              />
            );
          })
        )}
      </div>

      {/* Input bar */}
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "8px 10px",
        borderTop: "1px solid var(--heat-1, rgba(0,0,0,0.06))",
        background: "var(--bg-card)",
      }}>
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value.slice(0, 500))}
          onKeyDown={handleKeyDown}
          placeholder={t("chat.placeholder")}
          disabled={sending}
          style={{
            flex: 1,
            padding: "8px 14px",
            fontSize: 12,
            fontWeight: 500,
            border: "1px solid var(--heat-1, rgba(0,0,0,0.08))",
            borderRadius: 18,
            background: "var(--heat-0)",
            color: "var(--text-primary)",
            outline: "none",
          }}
        />

        {/* Character count (show when > 450) */}
        {input.length > 450 && (
          <span style={{
            fontSize: 9,
            fontWeight: 600,
            color: input.length >= 500 ? "var(--accent-pink, #ef4444)" : "var(--text-muted)",
            flexShrink: 0,
          }}>
            {input.length}/500
          </span>
        )}

        {/* Send button */}
        <button
          onClick={handleSend}
          disabled={!input.trim() || sending}
          style={{
            width: 32,
            height: 32,
            borderRadius: 16,
            border: "none",
            cursor: input.trim() && !sending ? "pointer" : "default",
            background: input.trim()
              ? "linear-gradient(135deg, var(--accent-purple), var(--accent-pink, #c084fc))"
              : "var(--heat-1)",
            color: input.trim() ? "#fff" : "var(--text-muted)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            transition: "all 0.15s ease",
          }}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"/>
          </svg>
        </button>
      </div>

      {/* Rate limit warning */}
      {rateLimited && (
        <div style={{
          textAlign: "center",
          padding: "4px 0",
          fontSize: 10,
          fontWeight: 600,
          color: "var(--accent-pink, #ef4444)",
          background: "var(--bg-card)",
        }}>
          {t("chat.rateLimited")}
        </div>
      )}
    </div>
  );
}

type GroupedItem =
  | { type: "date"; label: string }
  | { type: "message"; message: ChatMessage; showAvatar: boolean; showNickname: boolean };

function groupMessages(messages: ChatMessage[], locale: string): GroupedItem[] {
  const items: GroupedItem[] = [];
  let lastDate = "";
  let lastUserId = "";

  for (const msg of messages) {
    const msgDate = new Date(msg.created_at).toLocaleDateString();

    if (msgDate !== lastDate) {
      items.push({ type: "date", label: formatDateSeparator(msg.created_at, locale) });
      lastDate = msgDate;
      lastUserId = "";
    }

    const isSameUser = msg.user_id === lastUserId;
    items.push({
      type: "message",
      message: msg,
      showAvatar: !isSameUser,
      showNickname: !isSameUser,
    });

    lastUserId = msg.user_id;
  }

  return items;
}
