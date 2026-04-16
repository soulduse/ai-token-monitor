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
import { SettingsOverlay } from "./SettingsOverlay";

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
  } = useChat(userId, activated, visible);
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
  const [isDragging, setIsDragging] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const mentionRef = useRef<MentionAutocompleteRef>(null);
  const isAtBottomRef = useRef(true);
  const prevMessagesLenRef = useRef(0);
  const dragDepthRef = useRef(0);
  const [showNoAiKeyPopup, setShowNoAiKeyPopup] = useState(false);
  const [showSettingsToAi, setShowSettingsToAi] = useState(false);

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
  // Use rAF to ensure DOM layout is flushed after display:none→flex transition
  useEffect(() => {
    if (!loading && visible && scrollRef.current) {
      requestAnimationFrame(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
      });
    }
  }, [loading, visible]);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    isAtBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  }, []);

  // Translate reply: replaces input with translated text for user to review before sending
  const handleTranslateReply = useCallback(async () => {
    if (!hasAiKey || !hasAiModel) {
      setShowNoAiKeyPopup(true);
      return;
    }
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
  }, [hasAiKey, hasAiModel, input, replyingTo, translatingReply, invokeTranslateReply]);

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

  const acceptImageFile = useCallback((file: File | null | undefined) => {
    if (!file) return;
    // Must match the accept list enforced by Supabase Storage (see
    // `supabase/migrations/20260402_chat_images.sql` and `chatImageUpload.ts`).
    const ALLOWED_MIME = ["image/png", "image/jpeg", "image/gif", "image/webp"];
    if (!ALLOWED_MIME.includes(file.type)) {
      setImageError(t("chat.imageOnly"));
      setTimeout(() => setImageError(null), 3000);
      return;
    }
    setPendingImage((prev) => {
      if (prev) URL.revokeObjectURL(prev.preview);
      return { blob: file, preview: URL.createObjectURL(file) };
    });
    setImageError(null);
  }, [t]);

  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    for (const item of items) {
      if (item.type.startsWith("image/")) {
        e.preventDefault();
        acceptImageFile(item.getAsFile());
        return;
      }
    }
  }, [acceptImageFile]);

  const handleFileInputChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    acceptImageFile(file);
    // Reset so selecting the same file again fires change.
    e.target.value = "";
  }, [acceptImageFile]);

  const handleOpenFilePicker = useCallback(() => {
    if (uploadingImage || pendingImage || sending) return;
    fileInputRef.current?.click();
  }, [uploadingImage, pendingImage, sending]);

  // Drag & drop — gated on not having an image already.
  const handleDragEnter = useCallback((e: React.DragEvent) => {
    if (!e.dataTransfer.types?.includes("Files")) return;
    e.preventDefault();
    dragDepthRef.current += 1;
    if (!pendingImage && !uploadingImage) {
      setIsDragging(true);
    }
  }, [pendingImage, uploadingImage]);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    if (!e.dataTransfer.types?.includes("Files")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = pendingImage || uploadingImage ? "none" : "copy";
  }, [pendingImage, uploadingImage]);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    if (!e.dataTransfer.types?.includes("Files")) return;
    dragDepthRef.current = Math.max(0, dragDepthRef.current - 1);
    if (dragDepthRef.current === 0) {
      setIsDragging(false);
    }
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    if (!e.dataTransfer.types?.includes("Files")) return;
    e.preventDefault();
    dragDepthRef.current = 0;
    setIsDragging(false);
    if (pendingImage || uploadingImage) return;
    const file = e.dataTransfer.files?.[0];
    acceptImageFile(file);
  }, [pendingImage, uploadingImage, acceptImageFile]);

  // Safety net: if the user drags out of the window without a matching
  // dragleave reaching our container, reset the overlay to avoid it getting
  // stuck. Fires when the cursor leaves the document boundary.
  useEffect(() => {
    const handleDocLeave = (e: DragEvent) => {
      if (e.relatedTarget === null) {
        dragDepthRef.current = 0;
        setIsDragging(false);
      }
    };
    const handleDocDrop = () => {
      dragDepthRef.current = 0;
      setIsDragging(false);
    };
    document.addEventListener("dragleave", handleDocLeave);
    document.addEventListener("drop", handleDocDrop);
    return () => {
      document.removeEventListener("dragleave", handleDocLeave);
      document.removeEventListener("drop", handleDocDrop);
    };
  }, []);

  const handleRemovePendingImage = useCallback(() => {
    if (pendingImage) {
      URL.revokeObjectURL(pendingImage.preview);
      setPendingImage(null);
    }
  }, [pendingImage]);


  const handleReply = useCallback((message: ChatMessage) => {
    setReplyingTo(message);
    requestAnimationFrame(() => {
      inputRef.current?.focus();
    });
  }, []);

  const handleTranslate = useCallback((message: ChatMessage) => {
    if (!hasAiKey || !hasAiModel) {
      setShowNoAiKeyPopup(true);
      return;
    }
    translate(message.id, message.content);
  }, [hasAiKey, hasAiModel, translate]);

  const grouped = useMemo(() => groupMessages(messages, prefs.language ?? "en"), [messages, prefs.language]);

  return (
    <div
      onDragEnter={handleDragEnter}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      style={{
        display: "flex",
        flexDirection: "column",
        background: "var(--heat-0)",
        borderRadius: "var(--radius-lg)",
        overflow: "hidden",
        flex: 1,
        minHeight: 0,
        boxShadow: "var(--shadow-card)",
        position: "relative",
      }}
    >
      {/* Drag & drop overlay */}
      {isDragging && (
        <div style={{
          position: "absolute",
          inset: 0,
          zIndex: 20,
          background: "rgba(124, 92, 252, 0.12)",
          border: "2px dashed var(--accent-purple)",
          borderRadius: "var(--radius-lg)",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 8,
          pointerEvents: "none",
          backdropFilter: "blur(2px)",
        }}>
          <div style={{ fontSize: 28 }}>📎</div>
          <div style={{
            fontSize: 12,
            fontWeight: 700,
            color: "var(--accent-purple)",
          }}>
            {t("chat.dropHere")}
          </div>
        </div>
      )}

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
                onTranslate={handleTranslate}
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
          <input
            ref={fileInputRef}
            type="file"
            accept="image/png,image/jpeg,image/gif,image/webp"
            onChange={handleFileInputChange}
            style={{ display: "none" }}
          />
          <button
            type="button"
            onClick={handleOpenFilePicker}
            disabled={uploadingImage || !!pendingImage || sending}
            aria-label={t("chat.attachImage")}
            title={t("chat.attachImage")}
            style={{
              width: 32,
              height: 32,
              borderRadius: 16,
              border: "none",
              cursor: uploadingImage || pendingImage || sending ? "default" : "pointer",
              background: "transparent",
              color: uploadingImage || pendingImage || sending ? "var(--text-muted)" : "var(--text-secondary)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
              opacity: uploadingImage || pendingImage || sending ? 0.4 : 1,
              transition: "all 0.15s ease",
            }}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/>
            </svg>
          </button>
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

      {/* No AI Key popup */}
      {showNoAiKeyPopup && (
        <NoAiKeyPopup
          onClose={() => setShowNoAiKeyPopup(false)}
          onGoToSettings={() => {
            setShowNoAiKeyPopup(false);
            setShowSettingsToAi(true);
          }}
        />
      )}

      {/* Settings overlay opened from the no-AI-key popup */}
      <SettingsOverlay
        visible={showSettingsToAi}
        onClose={() => setShowSettingsToAi(false)}
        initialTab="ai"
        centered
      />

    </div>
  );
}

function NoAiKeyPopup({ onClose, onGoToSettings }: { onClose: () => void; onGoToSettings: () => void }) {
  const t = useI18n();

  useEffect(() => {
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    document.addEventListener("keydown", handleEsc, true);
    return () => document.removeEventListener("keydown", handleEsc, true);
  }, [onClose]);

  return (
    <div
      onClick={onClose}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 999,
        background: "rgba(0, 0, 0, 0.4)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-primary)",
          borderRadius: 12,
          padding: "20px 24px",
          maxWidth: 280,
          boxShadow: "0 8px 32px rgba(0,0,0,0.3)",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 12,
          textAlign: "center",
        }}
      >
        <TranslateIcon size={24} />
        <p style={{ fontSize: 12, color: "var(--text-primary)", fontWeight: 600, margin: 0, lineHeight: 1.5 }}>
          {t("chat.noAiKey")}
        </p>
        <button
          onClick={onGoToSettings}
          style={{
            background: "linear-gradient(135deg, var(--accent-purple), var(--accent-pink, #c084fc))",
            color: "#fff",
            border: "none",
            borderRadius: 8,
            padding: "8px 20px",
            fontSize: 11,
            fontWeight: 700,
            cursor: "pointer",
            transition: "opacity 0.15s ease",
          }}
        >
          {t("chat.noAiKey.goToSettings")}
        </button>
      </div>
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
