import { useState, useRef, useEffect, useCallback, useMemo } from "react";
import { useAuth } from "../hooks/useAuth";
import { useSettings } from "../contexts/SettingsContext";
import { useChat } from "../hooks/useChat";
import { useTranslate } from "../hooks/useTranslate";
import { useTypingIndicator } from "../hooks/useTypingIndicator";
import { ChatMessageRow, DateSeparator, formatDateSeparator, TranslateIcon } from "./ChatMessage";
import { MentionAutocomplete } from "./MentionAutocomplete";
import type { MentionAutocompleteRef } from "./MentionAutocomplete";
import { getAllCachedProfiles, getCachedProfile } from "../lib/profileCache";
import { uploadChatImage } from "../lib/chatImageUpload";
import { useI18n, LANGUAGE_NAMES } from "../i18n/I18nContext";
import type { ChatMessage } from "../hooks/useChat";

export function ChatRoom({ activated = true, visible = true }: { activated?: boolean; visible?: boolean }) {
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

  return <ChatContent userId={user.id} activated={activated} visible={visible} />;
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

function ChatContent({ userId, activated, visible }: { userId: string; activated: boolean; visible: boolean }) {
  const t = useI18n();
  const { prefs } = useSettings();
  const {
    messages, reactions, loading, sending, hasMore,
    sendMessage, deleteMessage, loadMore, toggleReaction,
  } = useChat(userId, activated);
  const langName = LANGUAGE_NAMES[prefs.language] ?? prefs.language;
  const { translations, translating, translate, translateReply: invokeTranslateReply } = useTranslate(langName);
  const hasAiKey = !!(prefs.ai_keys?.gemini || prefs.ai_keys?.openai || prefs.ai_keys?.anthropic);
  const hasAiModel = !!prefs.ai_model;
  const myNickname = useMemo(() => getCachedProfile(userId)?.nickname ?? null, [userId, messages.length]);
  const { typingUsers, sendTyping, stopTyping } = useTypingIndicator(userId, myNickname, activated);
  const [input, setInput] = useState("");
  const [rateLimited, setRateLimited] = useState(false);
  const [replyingTo, setReplyingTo] = useState<ChatMessage | null>(null);
  const [translatingReply, setTranslatingReply] = useState(false);
  const [mentionState, setMentionState] = useState<{ query: string; startIndex: number; anchorRect: DOMRect | null } | null>(null);
  const [pendingImage, setPendingImage] = useState<{ blob: Blob; preview: string } | null>(null);
  const [uploadingImage, setUploadingImage] = useState(false);
  const [imageError, setImageError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const mentionRef = useRef<MentionAutocompleteRef>(null);
  const isAtBottomRef = useRef(true);
  const prevMessagesLenRef = useRef(0);

  // Build known nicknames set from profile cache for mention highlighting
  const knownNicknames = useMemo(() => {
    const profiles = getAllCachedProfiles();
    const set = new Set<string>();
    for (const p of profiles.values()) {
      set.add(p.nickname.toLowerCase());
    }
    return set;
    // Re-compute when messages change (new profiles may have been cached)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [messages.length]);

  // Auto-scroll to bottom on new messages (only if user is at bottom)
  useEffect(() => {
    if (messages.length > prevMessagesLenRef.current && isAtBottomRef.current) {
      scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
    }
    prevMessagesLenRef.current = messages.length;
  }, [messages.length]);

  // Scroll to bottom on initial load or when tab becomes visible again
  useEffect(() => {
    if (!loading && visible && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [loading, visible]);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    isAtBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  }, []);

  // Translate reply: replaces input with translated text for user to review before sending
  const handleTranslateReply = useCallback(async () => {
    if (!input.trim() || !replyingTo || translatingReply) return;
    setTranslatingReply(true);
    try {
      const translated = await invokeTranslateReply(input, replyingTo.content);
      if (translated) {
        setInput(translated);
      }
    } finally {
      setTranslatingReply(false);
    }
  }, [input, replyingTo, translatingReply, invokeTranslateReply]);

  const handleSend = useCallback(async () => {
    if ((!input.trim() && !pendingImage) || sending || translatingReply || uploadingImage) return;

    let imageUrl: string | undefined;

    // Upload image if present
    if (pendingImage) {
      setUploadingImage(true);
      const uploadResult = await uploadChatImage(pendingImage.blob, userId);
      setUploadingImage(false);
      if (uploadResult.error) {
        setImageError(uploadResult.error);
        setTimeout(() => setImageError(null), 3000);
        URL.revokeObjectURL(pendingImage.preview);
        setPendingImage(null);
        return;
      }
      imageUrl = uploadResult.url;
    }

    const result = await sendMessage(input, replyingTo?.id, imageUrl);
    if (result.error === "rate_limited") {
      setRateLimited(true);
      setTimeout(() => setRateLimited(false), 3000);
      return;
    }
    if (!result.error) {
      if (pendingImage) {
        URL.revokeObjectURL(pendingImage.preview);
        setPendingImage(null);
      }
      setInput("");
      setReplyingTo(null);
      stopTyping();
      if (inputRef.current) inputRef.current.style.height = "auto";
      setTimeout(() => {
        scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
      }, 100);
    }
  }, [input, sending, sendMessage, replyingTo, translatingReply, pendingImage, uploadingImage, userId, stopTyping]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    // Delegate to mention autocomplete first
    if (mentionState && mentionRef.current?.handleKeyDown(e)) {
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
    if (e.key === "Escape" && replyingTo) {
      setReplyingTo(null);
    }
  }, [handleSend, replyingTo, mentionState]);

  const handleMentionSelect = useCallback((nickname: string) => {
    if (!mentionState) return;
    const before = input.slice(0, mentionState.startIndex);
    const after = input.slice(mentionState.startIndex + 1 + mentionState.query.length); // skip @query
    const newInput = `${before}@${nickname} ${after}`;
    setInput(newInput.slice(0, 500));
    setMentionState(null);
    inputRef.current?.focus();
  }, [mentionState, input]);

  const handleMentionClose = useCallback(() => {
    setMentionState(null);
  }, []);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    for (const item of items) {
      if (item.type.startsWith("image/")) {
        e.preventDefault();
        const blob = item.getAsFile();
        if (!blob) return;
        const preview = URL.createObjectURL(blob);
        setPendingImage({ blob, preview });
        return;
      }
    }
  }, []);

  const handleRemovePendingImage = useCallback(() => {
    if (pendingImage) {
      URL.revokeObjectURL(pendingImage.preview);
      setPendingImage(null);
    }
  }, [pendingImage]);


  const handleReply = useCallback((message: ChatMessage) => {
    setReplyingTo(message);
  }, []);

  const handleTranslate = useCallback((message: ChatMessage) => {
    if (!hasAiKey || !hasAiModel) return;
    translate(message.id, message.content);
  }, [hasAiKey, hasAiModel, translate]);

  const grouped = useMemo(() => groupMessages(messages, prefs.language ?? "en"), [messages, prefs.language]);

  return (
    <div style={{
      display: "flex",
      flexDirection: "column",
      background: "var(--heat-0)",
      borderRadius: "var(--radius-lg)",
      overflow: "hidden",
      flex: 1,
      minHeight: 0,
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
                onReply={handleReply}
                onTranslate={hasAiKey && hasAiModel ? handleTranslate : undefined}
                reactions={reactions.get(item.message.id)}
                userId={userId}
                onReact={toggleReaction}
                translation={translations[item.message.id] ?? null}
                translating={translating.has(item.message.id)}
                knownNicknames={knownNicknames}
              />
            );
          })
        )}
      </div>

      {/* Mention autocomplete */}
      {mentionState && (
        <MentionAutocomplete
          ref={mentionRef}
          query={mentionState.query}
          onSelect={handleMentionSelect}
          onClose={handleMentionClose}
          anchorRect={mentionState.anchorRect}
          currentUserId={userId}
        />
      )}

      {/* Typing indicator */}
      {typingUsers.length > 0 && (
        <div style={{
          padding: "3px 14px",
          fontSize: 11,
          fontWeight: 600,
          color: "var(--text-secondary)",
          background: "var(--bg-card)",
          borderTop: "1px solid var(--heat-1, rgba(0,0,0,0.06))",
          display: "flex",
          alignItems: "center",
          gap: 4,
        }}>
          <TypingDots />
          <span>
            {typingUsers.length === 1
              ? t("chat.typing.one", { name: typingUsers[0].nickname })
              : typingUsers.length === 2
                ? t("chat.typing.two", { name1: typingUsers[0].nickname, name2: typingUsers[1].nickname })
                : t("chat.typing.many", { name: typingUsers[0].nickname, count: String(typingUsers.length - 1) })}
          </span>
        </div>
      )}

      {/* Input bar (with optional reply preview integrated) */}
      <div style={{
        borderTop: typingUsers.length > 0 ? "none" : "1px solid var(--heat-1, rgba(0,0,0,0.06))",
        background: "var(--bg-card)",
        padding: "8px 10px",
      }}>
        {/* Pending image preview */}
        {pendingImage && (
          <div style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 6,
            padding: "6px 10px",
            marginBottom: 6,
            borderRadius: 10,
            background: "rgba(124, 92, 252, 0.08)",
            borderLeft: "3px solid var(--accent-purple)",
          }}>
            <img
              src={pendingImage.preview}
              alt=""
              style={{
                maxWidth: 120,
                maxHeight: 80,
                borderRadius: 6,
                objectFit: "cover",
              }}
            />
            <div style={{ flex: 1, minWidth: 0, fontSize: 10, color: "var(--text-secondary)", fontWeight: 600, paddingTop: 2 }}>
              {uploadingImage ? t("chat.uploading") : t("chat.imageReady")}
            </div>
            <button
              onClick={handleRemovePendingImage}
              disabled={uploadingImage}
              style={{
                background: "none",
                border: "none",
                cursor: uploadingImage ? "default" : "pointer",
                padding: 2,
                color: "var(--text-muted)",
                fontSize: 13,
                lineHeight: 1,
                flexShrink: 0,
              }}
            >
              ✕
            </button>
          </div>
        )}

        {/* Reply preview */}
        {replyingTo && (
          <div style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "6px 10px",
            marginBottom: 6,
            borderRadius: 10,
            background: "rgba(124, 92, 252, 0.08)",
            borderLeft: "3px solid var(--accent-purple)",
            fontSize: 11,
          }}>
            <div style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              <span style={{ fontWeight: 700, color: "var(--accent-purple)", marginRight: 4 }}>
                {replyingTo.nickname}
              </span>
              <span style={{ color: "var(--text-secondary)", fontWeight: 500 }}>
                {replyingTo.content.length > 50 ? replyingTo.content.slice(0, 50) + "…" : replyingTo.content}
              </span>
            </div>
            {hasAiKey && hasAiModel && (
              <button
                onClick={handleTranslateReply}
                disabled={!input.trim() || translatingReply}
                style={{
                  background: translatingReply ? "rgba(124, 92, 252, 0.2)" : "rgba(124, 92, 252, 0.06)",
                  border: "1px solid rgba(124, 92, 252, 0.15)",
                  borderRadius: 10,
                  cursor: !input.trim() || translatingReply ? "default" : "pointer",
                  padding: "3px 8px",
                  fontSize: 9,
                  color: input.trim() ? "var(--accent-purple)" : "var(--text-muted)",
                  fontWeight: 700,
                  display: "flex",
                  alignItems: "center",
                  gap: 3,
                  flexShrink: 0,
                  opacity: !input.trim() ? 0.5 : 1,
                  transition: "all 0.15s ease",
                }}
              >
                <TranslateIcon size={10} />
                <span>{translatingReply ? t("chat.translating") : t("chat.translateReply")}</span>
              </button>
            )}
            <button
              onClick={() => { setReplyingTo(null); }}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: 2,
                color: "var(--text-muted)",
                fontSize: 13,
                lineHeight: 1,
                flexShrink: 0,
              }}
            >
              ✕
            </button>
          </div>
        )}

        <div style={{ display: "flex", alignItems: "flex-end", gap: 8 }}>
          <textarea
            ref={inputRef}
            value={input}
            onChange={(e) => {
              const val = e.target.value.slice(0, 500);
              setInput(val);
              sendTyping();

              // Mention detection
              const cursor = e.target.selectionStart ?? val.length;
              const textBefore = val.slice(0, cursor);
              const atMatch = textBefore.match(/(^|[\s\n])@(\S*)$/);
              if (atMatch) {
                const startIndex = textBefore.length - atMatch[2].length - 1; // position of @
                const anchorRect = e.target.getBoundingClientRect();
                setMentionState({ query: atMatch[2], startIndex, anchorRect });
              } else {
                setMentionState(null);
              }
            }}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            placeholder={replyingTo ? t("chat.replyPlaceholder") : t("chat.placeholder")}
            disabled={sending || uploadingImage}
            rows={1}
            onInput={(e) => {
              const el = e.currentTarget;
              el.style.height = "auto";
              el.style.height = Math.min(el.scrollHeight, 80) + "px";
            }}
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
              resize: "none",
              lineHeight: 1.4,
              fontFamily: "inherit",
              overflow: "hidden",
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
          disabled={(!input.trim() && !pendingImage) || sending || uploadingImage}
          style={{
            width: 32,
            height: 32,
            borderRadius: 16,
            border: "none",
            cursor: (input.trim() || pendingImage) && !sending && !uploadingImage ? "pointer" : "default",
            background: (input.trim() || pendingImage)
              ? "linear-gradient(135deg, var(--accent-purple), var(--accent-pink, #c084fc))"
              : "var(--heat-1)",
            color: (input.trim() || pendingImage) ? "#fff" : "var(--text-muted)",
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
      </div>

      {/* Rate limit / image error warning */}
      {(rateLimited || imageError) && (
        <div style={{
          textAlign: "center",
          padding: "4px 0",
          fontSize: 10,
          fontWeight: 600,
          color: "var(--accent-pink, #ef4444)",
          background: "var(--bg-card)",
        }}>
          {rateLimited ? t("chat.rateLimited") : imageError}
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

/** Animated typing dots (CSS keyframes via inline style) */
function TypingDots() {
  return (
    <span style={{ display: "inline-flex", gap: 2, alignItems: "center" }}>
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          style={{
            width: 4,
            height: 4,
            borderRadius: "50%",
            background: "var(--accent-purple)",
            opacity: 0.5,
            animation: `typingDot 1.2s ease-in-out ${i * 0.2}s infinite`,
          }}
        />
      ))}
    </span>
  );
}
