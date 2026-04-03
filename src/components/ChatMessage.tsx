import { useState, useRef, useEffect, useCallback } from "react";
import { openImagePopup } from "../lib/openImagePopup";
import type { ChatMessage as ChatMessageData, ReactionMap, ReactionType } from "../hooks/useChat";
import { useMiniProfile } from "../contexts/MiniProfileContext";
import { useI18n } from "../i18n/I18nContext";
import { RichMessageContent } from "./RichMessageContent";

export function ReplyIcon({ size = 12 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="9 17 4 12 9 7" />
      <path d="M20 18v-2a4 4 0 0 0-4-4H4" />
    </svg>
  );
}

export function TranslateIcon({ size = 12 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 8l6 6" />
      <path d="M4 14l6-6 2-3" />
      <path d="M2 5h12" />
      <path d="M7 2h1" />
      <path d="M22 22l-5-10-5 10" />
      <path d="M14 18h6" />
    </svg>
  );
}

interface Props {
  message: ChatMessageData;
  isMe: boolean;
  showAvatar: boolean;
  showNickname: boolean;
  onDelete?: (id: string) => void;
  onReply?: (message: ChatMessageData) => void;
  onTranslate?: (message: ChatMessageData) => void;
  reactions?: ReactionMap;
  userId?: string;
  onReact?: (messageId: string, type: ReactionType) => void;
  translation?: string | null;
  translating?: boolean;
  knownNicknames?: Set<string>;
}

function formatTime(dateStr: string): string {
  const d = new Date(dateStr);
  const h = d.getHours();
  const m = d.getMinutes().toString().padStart(2, "0");
  const ampm = h < 12 ? "AM" : "PM";
  const h12 = h === 0 ? 12 : h > 12 ? h - 12 : h;
  return `${ampm} ${h12}:${m}`;
}

export function formatDateSeparator(dateStr: string, locale: string): string {
  const d = new Date(dateStr);
  if (locale.startsWith("ko")) {
    return `${d.getFullYear()}년 ${d.getMonth() + 1}월 ${d.getDate()}일`;
  }
  if (locale.startsWith("ja")) {
    return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
  }
  if (locale.startsWith("zh")) {
    return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
  }
  return d.toLocaleDateString(locale, { year: "numeric", month: "long", day: "numeric" });
}

const REACTION_EMOJIS: { type: ReactionType; emoji: string }[] = [
  { type: "like", emoji: "👍" },
  { type: "heart", emoji: "❤️" },
  { type: "dislike", emoji: "👎" },
];

/** Combined reaction + action toolbar below each bubble. Fixed height to prevent layout shift. */
function BubbleToolbar({
  reactions,
  userId,
  messageId,
  onReact,
  hovered,
  isMe,
  onDelete,
  onReply,
  onTranslate,
  translating,
  align,
}: {
  reactions?: ReactionMap;
  userId?: string;
  messageId: string;
  onReact?: (messageId: string, type: ReactionType) => void;
  hovered: boolean;
  isMe: boolean;
  onDelete?: (id: string) => void;
  onReply?: () => void;
  onTranslate?: () => void;
  translating?: boolean;
  align: "left" | "right";
}) {
  const t = useI18n();

  const hasAnyReaction = reactions && (reactions.like.length > 0 || reactions.heart.length > 0 || reactions.dislike.length > 0);
  const showReactions = onReact && userId && (hasAnyReaction || hovered);
  const showActions = hovered && (onReply || onTranslate || (isMe && onDelete));

  if (!showReactions && !showActions) {
    return <div style={{ height: 20 }} />;
  }

  const actionBtnStyle = {
    background: "none",
    border: "none",
    cursor: "pointer",
    padding: "1px 4px",
    color: "var(--text-muted)",
    fontSize: 9,
    opacity: 0.6,
    lineHeight: 1,
    display: "flex",
    alignItems: "center",
    gap: 3,
  } as const;

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      gap: 6,
      height: 20,
      justifyContent: align === "right" ? "flex-end" : "flex-start",
      paddingLeft: align === "left" ? 40 : 0,
    }}>
      {/* Reactions */}
      {showReactions && REACTION_EMOJIS.map(({ type, emoji }) => {
        const count = reactions?.[type]?.length ?? 0;
        const isMine = reactions?.[type]?.includes(userId!) ?? false;
        if (count === 0 && !hovered) return null;
        return (
          <button
            key={type}
            onClick={() => onReact!(messageId, type)}
            style={{
              background: isMine ? "rgba(124, 92, 252, 0.15)" : "transparent",
              border: isMine ? "1px solid rgba(124, 92, 252, 0.3)" : "1px solid transparent",
              borderRadius: 10,
              padding: "1px 5px",
              fontSize: 10,
              cursor: "pointer",
              display: "flex",
              alignItems: "center",
              gap: 2,
              color: "var(--text-secondary)",
              lineHeight: 1.4,
            }}
          >
            <span style={{ fontSize: 9 }}>{emoji}</span>
            {count > 0 && <span style={{ fontSize: 8, fontWeight: 700 }}>{count}</span>}
          </button>
        );
      })}

      {/* Separator */}
      {showReactions && showActions && (
        <div style={{ width: 1, height: 12, background: "var(--heat-0)", opacity: 0.5 }} />
      )}

      {/* Action buttons */}
      {showActions && (
        <>
          {onTranslate && (
            <button
              onClick={onTranslate}
              disabled={translating}
              style={{ ...actionBtnStyle, cursor: translating ? "wait" : "pointer", opacity: translating ? 0.4 : 0.6 }}
            >
              <TranslateIcon size={10} />
              <span>{t("chat.translate")}</span>
            </button>
          )}
          {onReply && (
            <button onClick={onReply} style={actionBtnStyle}>
              <ReplyIcon size={10} />
              <span>{t("chat.reply")}</span>
            </button>
          )}
          {isMe && onDelete && (
            <button onClick={() => onDelete(messageId)} style={actionBtnStyle}>
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="3 6 5 6 21 6"/>
                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
              </svg>
              <span>{t("chat.delete")}</span>
            </button>
          )}
        </>
      )}
    </div>
  );
}

function ReplyPreview({ nickname, content }: { nickname: string; content: string }) {
  const truncated = content.length > 60 ? content.slice(0, 60) + "…" : content;
  return (
    <div style={{
      borderLeft: "2px solid rgba(124, 92, 252, 0.5)",
      paddingLeft: 8,
      marginBottom: 6,
      fontSize: 10,
      lineHeight: 1.4,
      opacity: 0.8,
    }}>
      <div style={{ fontWeight: 700, marginBottom: 1 }}>{nickname}</div>
      <div style={{ fontWeight: 400 }}>{truncated}</div>
    </div>
  );
}

function ContextMenu({
  x,
  y,
  onReply,
  onClose,
}: {
  x: number;
  y: number;
  onReply: () => void;
  onClose: () => void;
}) {
  const t = useI18n();
  const menuRef = useRef<HTMLDivElement>(null);
  const clampedX = Math.min(x, window.innerWidth - 140);
  const clampedY = Math.min(y, window.innerHeight - 60);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  return (
    <div
      ref={menuRef}
      style={{
        position: "fixed",
        left: clampedX,
        top: clampedY,
        background: "var(--bg-card)",
        borderRadius: 8,
        boxShadow: "0 4px 16px rgba(0,0,0,0.2)",
        border: "1px solid rgba(124, 92, 252, 0.15)",
        zIndex: 100,
        padding: 4,
        minWidth: 120,
      }}
    >
      <button
        onClick={() => { onReply(); onClose(); }}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          width: "100%",
          padding: "6px 10px",
          background: "none",
          border: "none",
          borderRadius: 6,
          cursor: "pointer",
          fontSize: 12,
          fontWeight: 600,
          color: "var(--text-primary)",
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = "var(--heat-0)")}
        onMouseLeave={(e) => (e.currentTarget.style.background = "none")}
      >
        <ReplyIcon />
        {t("chat.contextMenu.reply")}
      </button>
    </div>
  );
}

export function ChatMessageRow({
  message,
  isMe,
  showAvatar,
  showNickname,
  onDelete,
  onReply,
  onTranslate,
  reactions,
  userId,
  onReact,
  translation,
  translating,
  knownNicknames,
}: Props) {
  const { open: openMiniProfile } = useMiniProfile();
  const [hovered, setHovered] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const closeContextMenu = useCallback(() => setContextMenu(null), []);

  const handleProfileClick = useCallback(() => {
    openMiniProfile({
      user_id: message.user_id,
      nickname: message.nickname,
      avatar_url: message.avatar_url,
    });
  }, [openMiniProfile, message.user_id, message.nickname, message.avatar_url]);

  const handleContextMenu = (e: React.MouseEvent) => {
    if (onReply) {
      e.preventDefault();
      setContextMenu({ x: e.clientX, y: e.clientY });
    }
  };

  if (isMe) {
    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          marginTop: showNickname ? 12 : 3,
          paddingRight: 8,
          paddingLeft: 40,
        }}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        onContextMenu={handleContextMenu}
      >
        <div style={{ display: "flex", justifyContent: "flex-end", alignItems: "flex-end", gap: 4 }}>
          <span style={{ fontSize: 9, color: "var(--text-muted)", fontWeight: 500, whiteSpace: "nowrap" }}>
            {formatTime(message.created_at)}
          </span>

          {/* Bubble */}
          <div style={{
            background: "linear-gradient(135deg, var(--accent-purple), var(--accent-pink, #c084fc))",
            color: "#fff",
            padding: "8px 12px",
            borderRadius: "12px 12px 2px 12px",
            fontSize: 12,
            fontWeight: 500,
            lineHeight: 1.5,
            maxWidth: "75%",
            wordBreak: "break-word",
            whiteSpace: "pre-wrap",
          }}>
            {message.replied_message && (
              <ReplyPreview nickname={message.replied_message.nickname} content={message.replied_message.content} />
            )}
            {message.image_url && (
              <img
                src={message.image_url}
                alt=""
                onClick={(e) => { e.stopPropagation(); openImagePopup(message.image_url!); }}
                style={{
                  maxWidth: "100%",
                  maxHeight: 200,
                  borderRadius: 8,
                  cursor: "zoom-in",
                  marginBottom: message.content ? 6 : 0,
                  display: "block",
                }}
              />
            )}
            {message.content && !(message.image_url && message.content === "[image]") && <RichMessageContent content={message.content} isMe knownNicknames={knownNicknames} />}
            {translation && (
              <div style={{
                borderTop: "1px solid rgba(255,255,255,0.2)",
                marginTop: 6,
                paddingTop: 6,
                fontSize: 11,
                fontStyle: "italic",
                opacity: 0.9,
              }}>
                <RichMessageContent content={translation} isMe knownNicknames={knownNicknames} />
              </div>
            )}
          </div>
        </div>

        <BubbleToolbar
          reactions={reactions}
          userId={userId}
          messageId={message.id}
          onReact={onReact}
          hovered={hovered}
          isMe
          onDelete={onDelete}
          onReply={onReply ? () => onReply(message) : undefined}
          onTranslate={onTranslate ? () => onTranslate(message) : undefined}
          translating={translating}
          align="right"
        />

        {contextMenu && (
          <ContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            onReply={() => onReply?.(message)}
            onClose={closeContextMenu}
          />
        )}
      </div>
    );
  }

  return (
    <div
      style={{ marginTop: showNickname ? 12 : 3, paddingLeft: 8, paddingRight: 40 }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onContextMenu={handleContextMenu}
    >
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        {/* Avatar column */}
        <div style={{ width: 32, flexShrink: 0 }}>
          {showAvatar ? (
            message.avatar_url ? (
              <img
                src={message.avatar_url}
                alt=""
                style={{ width: 32, height: 32, borderRadius: 10, cursor: "pointer" }}
                onClick={handleProfileClick}
              />
            ) : (
              <div
                onClick={handleProfileClick}
                style={{
                  width: 32,
                  height: 32,
                  borderRadius: 10,
                  background: "var(--heat-1)",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  fontSize: 13,
                  fontWeight: 700,
                  color: "var(--accent-purple)",
                  cursor: "pointer",
                }}
              >
                {message.nickname.charAt(0).toUpperCase()}
              </div>
            )
          ) : null}
        </div>

        {/* Content column */}
        <div style={{ flex: 1, minWidth: 0 }}>
          {showNickname && (
            <div
              onClick={handleProfileClick}
              style={{
                fontSize: 11,
                fontWeight: 700,
                color: "var(--text-secondary)",
                marginBottom: 4,
                cursor: "pointer",
              }}
            >
              {message.nickname}
            </div>
          )}

          <div style={{ display: "flex", alignItems: "flex-end", gap: 4 }}>
            {/* Bubble */}
            <div style={{
              background: "var(--chat-bubble-other)",
              color: "var(--text-primary)",
              padding: "8px 12px",
              borderRadius: showNickname ? "2px 12px 12px 12px" : "12px 12px 12px 12px",
              fontSize: 12,
              fontWeight: 500,
              lineHeight: 1.5,
              maxWidth: "75%",
              wordBreak: "break-word",
              whiteSpace: "pre-wrap",
              boxShadow: "0 1px 3px rgba(0,0,0,0.1)",
            }}>
              {message.replied_message && (
                <ReplyPreview nickname={message.replied_message.nickname} content={message.replied_message.content} />
              )}
              {message.image_url && (
                <img
                  src={message.image_url}
                  alt=""
                  onClick={(e) => { e.stopPropagation(); openImagePopup(message.image_url!); }}
                  style={{
                    maxWidth: "100%",
                    maxHeight: 200,
                    borderRadius: 8,
                    cursor: "zoom-in",
                    marginBottom: message.content ? 6 : 0,
                    display: "block",
                  }}
                />
              )}
              {message.content && !(message.image_url && message.content === "[image]") && <RichMessageContent content={message.content} isMe={false} knownNicknames={knownNicknames} />}
              {translation && (
                <div style={{
                  borderTop: "1px solid var(--heat-1, rgba(0,0,0,0.08))",
                  marginTop: 6,
                  paddingTop: 6,
                  fontSize: 11,
                  fontStyle: "italic",
                  color: "var(--text-secondary)",
                }}>
                  <RichMessageContent content={translation} isMe={false} knownNicknames={knownNicknames} />
                </div>
              )}
            </div>

            {/* Time */}
            <span style={{ fontSize: 9, color: "var(--text-muted)", fontWeight: 500, whiteSpace: "nowrap", flexShrink: 0 }}>
              {formatTime(message.created_at)}
            </span>
          </div>

          <BubbleToolbar
            reactions={reactions}
            userId={userId}
            messageId={message.id}
            onReact={onReact}
            hovered={hovered}
            isMe={false}
            onReply={onReply ? () => onReply(message) : undefined}
            onTranslate={onTranslate ? () => onTranslate(message) : undefined}
            translating={translating}
            align="left"
          />
        </div>
      </div>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onReply={() => onReply?.(message)}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}

export function DateSeparator({ label }: { label: string }) {
  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      gap: 8,
      margin: "16px 12px 8px",
    }}>
      <div style={{ flex: 1, height: 1, background: "var(--heat-0)" }} />
      <span style={{ fontSize: 10, fontWeight: 600, color: "var(--text-muted)", whiteSpace: "nowrap" }}>
        {label}
      </span>
      <div style={{ flex: 1, height: 1, background: "var(--heat-0)" }} />
    </div>
  );
}
