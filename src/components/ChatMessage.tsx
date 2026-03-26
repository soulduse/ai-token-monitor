import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ChatMessage as ChatMessageData } from "../hooks/useChat";
import { useI18n } from "../i18n/I18nContext";

interface Props {
  message: ChatMessageData;
  isMe: boolean;
  showAvatar: boolean;  // false when same user sends consecutive messages
  showNickname: boolean;
  onDelete?: (id: string) => void;
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

export function ChatMessageRow({ message, isMe, showAvatar, showNickname, onDelete }: Props) {
  const [hovered, setHovered] = useState(false);
  const t = useI18n();

  if (isMe) {
    return (
      <div
        style={{
          display: "flex",
          justifyContent: "flex-end",
          alignItems: "flex-end",
          gap: 4,
          marginTop: showNickname ? 12 : 3,
          paddingRight: 8,
          paddingLeft: 40,
        }}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
      >
        {/* Delete button + time on left */}
        <div style={{ display: "flex", alignItems: "flex-end", gap: 4, flexShrink: 0 }}>
          {hovered && onDelete && (
            <button
              onClick={() => onDelete(message.id)}
              title={t("chat.delete")}
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                padding: 2,
                color: "var(--text-muted)",
                fontSize: 10,
                opacity: 0.6,
                lineHeight: 1,
              }}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="3 6 5 6 21 6"/>
                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
              </svg>
            </button>
          )}
          <span style={{ fontSize: 9, color: "var(--text-muted)", fontWeight: 500, whiteSpace: "nowrap" }}>
            {formatTime(message.created_at)}
          </span>
        </div>

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
          {message.content}
        </div>
      </div>
    );
  }

  // Other user's message (left-aligned)
  return (
    <div style={{ marginTop: showNickname ? 12 : 3, paddingLeft: 8, paddingRight: 40 }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        {/* Avatar column (fixed width for alignment) */}
        <div style={{ width: 32, flexShrink: 0 }}>
          {showAvatar ? (
            message.avatar_url ? (
              <img
                src={message.avatar_url}
                alt=""
                style={{ width: 32, height: 32, borderRadius: 10, cursor: "pointer" }}
                onClick={() => openUrl(`https://github.com/${message.nickname}`)}
              />
            ) : (
              <div
                onClick={() => openUrl(`https://github.com/${message.nickname}`)}
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
              onClick={() => openUrl(`https://github.com/${message.nickname}`)}
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
              background: "var(--bg-card)",
              color: "var(--text-primary)",
              padding: "8px 12px",
              borderRadius: showNickname ? "2px 12px 12px 12px" : "12px 12px 12px 12px",
              fontSize: 12,
              fontWeight: 500,
              lineHeight: 1.5,
              maxWidth: "75%",
              wordBreak: "break-word",
              whiteSpace: "pre-wrap",
              boxShadow: "0 1px 2px rgba(0,0,0,0.05)",
            }}>
              {message.content}
            </div>

            {/* Time */}
            <span style={{ fontSize: 9, color: "var(--text-muted)", fontWeight: 500, whiteSpace: "nowrap", flexShrink: 0 }}>
              {formatTime(message.created_at)}
            </span>
          </div>
        </div>
      </div>
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
