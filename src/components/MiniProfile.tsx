import { useState, useEffect, useMemo, useRef } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useMiniProfile } from "../contexts/MiniProfileContext";
import { useUserActivity } from "../hooks/useUserActivity";
import { useProfileLinks } from "../hooks/useProfileLinks";
import { computeStreaks } from "../lib/statsHelpers";
import { Heatmap } from "./Heatmap";
import { useI18n } from "../i18n/I18nContext";
import type { DailyUsage } from "../lib/types";

interface Props {
  localDaily?: DailyUsage[];
  currentUserId: string | null;
}

export function MiniProfile({ localDaily, currentUserId }: Props) {
  const { target, close } = useMiniProfile();
  const isMe = target?.user_id === currentUserId;
  const { daily: remoteDaily, loading: remoteLoading, error } = useUserActivity(isMe ? null : target?.user_id ?? null);
  const { links, fetching, addLink, removeLink, canAdd } = useProfileLinks(target?.user_id ?? null, isMe);
  const t = useI18n();
  const [streakTooltip, setStreakTooltip] = useState(false);
  const [addingLink, setAddingLink] = useState(false);
  const [linkInput, setLinkInput] = useState("");
  const [linkError, setLinkError] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const daily = isMe && localDaily ? localDaily : remoteDaily;
  const loading = isMe ? false : remoteLoading;

  const streak = useMemo(() => {
    if (daily.length === 0) return null;
    return computeStreaks(daily);
  }, [daily]);

  // ESC to close (capture phase to prevent App's hide_window)
  useEffect(() => {
    if (!target) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (addingLink) {
          setAddingLink(false);
          setLinkInput("");
          setLinkError(false);
        } else {
          close();
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [target, close, addingLink]);

  // Reset state when target changes
  useEffect(() => {
    setStreakTooltip(false);
    setAddingLink(false);
    setLinkInput("");
    setLinkError(false);
  }, [target]);

  // Focus input when adding
  useEffect(() => {
    if (addingLink) inputRef.current?.focus();
  }, [addingLink]);

  const handleAddLink = async () => {
    const url = linkInput.trim();
    if (!url) return;

    // Auto-add https:// if no protocol
    const fullUrl = url.match(/^https?:\/\//) ? url : `https://${url}`;

    setLinkError(false);
    const success = await addLink(fullUrl);
    if (success) {
      setAddingLink(false);
      setLinkInput("");
    } else {
      setLinkError(true);
    }
  };

  if (!target) return null;

  return (
    <div
      onClick={close}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1000,
        background: "rgba(0, 0, 0, 0.6)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        animation: "miniProfileFadeIn 0.15s ease",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 320,
          maxHeight: "90vh",
          overflowY: "auto",
          background: "var(--bg-card)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "0 20px 60px rgba(0, 0, 0, 0.5)",
          overflow: "hidden",
          animation: "miniProfileScaleIn 0.15s ease",
        }}
      >
        {/* Avatar + Nickname */}
        <div style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          padding: "24px 20px 12px",
          gap: 8,
        }}>
          {target.avatar_url ? (
            <img
              src={target.avatar_url}
              alt=""
              referrerPolicy="no-referrer"
              style={{
                width: 72,
                height: 72,
                borderRadius: "50%",
                border: "3px solid var(--accent-purple)",
              }}
            />
          ) : (
            <div style={{
              width: 72,
              height: 72,
              borderRadius: "50%",
              background: "var(--heat-1)",
              border: "3px solid var(--accent-purple)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 28,
              fontWeight: 800,
              color: "var(--accent-purple)",
            }}>
              {target.nickname.charAt(0).toUpperCase()}
            </div>
          )}
          <div style={{
            fontSize: 16,
            fontWeight: 800,
            color: "var(--text-primary)",
          }}>
            {target.nickname}
          </div>

          {/* Streak badge with tooltip */}
          {!loading && streak && streak.currentStreak > 0 && (
            <div
              style={{ position: "relative" }}
              onMouseEnter={() => setStreakTooltip(true)}
              onMouseLeave={() => setStreakTooltip(false)}
            >
              <div style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "4px 12px",
                borderRadius: 20,
                background: "rgba(124, 92, 252, 0.12)",
                cursor: "default",
              }}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent-purple)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z"/>
                </svg>
                <span style={{
                  fontSize: 12,
                  fontWeight: 700,
                  color: "var(--accent-purple)",
                }}>
                  {streak.currentStreak} {t("activity.days")}
                </span>
              </div>
              {streakTooltip && (
                <div style={{
                  position: "absolute",
                  top: "100%",
                  left: "50%",
                  transform: "translateX(-50%)",
                  marginTop: 6,
                  padding: "6px 10px",
                  background: "var(--bg-primary)",
                  borderRadius: "var(--radius-sm)",
                  fontSize: 10,
                  fontWeight: 600,
                  color: "var(--text-secondary)",
                  whiteSpace: "nowrap",
                  boxShadow: "0 4px 12px rgba(0, 0, 0, 0.4)",
                  zIndex: 10,
                  pointerEvents: "none",
                }}>
                  {t("miniProfile.streakTooltip")}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Links section */}
        {(links.length > 0 || canAdd) && (
          <div style={{ padding: "0 16px 12px" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              {links.map((link, i) => (
                <LinkRow
                  key={`${link.url}-${i}`}
                  link={link}
                  isMe={isMe}
                  onRemove={() => removeLink(i)}
                />
              ))}

              {/* Add link UI */}
              {canAdd && !addingLink && (
                <button
                  onClick={() => setAddingLink(true)}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 6,
                    padding: "6px 10px",
                    borderRadius: "var(--radius-sm)",
                    border: "1px dashed rgba(255, 255, 255, 0.12)",
                    background: "transparent",
                    color: "var(--text-secondary)",
                    fontSize: 11,
                    fontWeight: 600,
                    cursor: "pointer",
                    width: "100%",
                    justifyContent: "center",
                    transition: "border-color 0.15s ease, color 0.15s ease",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.borderColor = "var(--accent-purple)";
                    e.currentTarget.style.color = "var(--accent-purple)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.borderColor = "rgba(255, 255, 255, 0.12)";
                    e.currentTarget.style.color = "var(--text-secondary)";
                  }}
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
                    <line x1="12" y1="5" x2="12" y2="19"/>
                    <line x1="5" y1="12" x2="19" y2="12"/>
                  </svg>
                  {t("miniProfile.addLink")}
                </button>
              )}

              {addingLink && (
                <div style={{
                  display: "flex",
                  gap: 4,
                  alignItems: "center",
                }}>
                  <input
                    ref={inputRef}
                    type="text"
                    value={linkInput}
                    onChange={(e) => { setLinkInput(e.target.value); setLinkError(false); }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") { e.preventDefault(); handleAddLink(); }
                    }}
                    placeholder="https://..."
                    disabled={fetching}
                    style={{
                      flex: 1,
                      padding: "6px 8px",
                      borderRadius: "var(--radius-sm)",
                      border: linkError
                        ? "1px solid rgba(255, 80, 80, 0.5)"
                        : "1px solid rgba(255, 255, 255, 0.15)",
                      background: "var(--bg-primary)",
                      color: "var(--text-primary)",
                      fontSize: 11,
                      outline: "none",
                    }}
                  />
                  <button
                    onClick={handleAddLink}
                    disabled={fetching || !linkInput.trim()}
                    style={{
                      padding: "6px 10px",
                      borderRadius: "var(--radius-sm)",
                      border: "none",
                      background: "var(--accent-purple)",
                      color: "#fff",
                      fontSize: 10,
                      fontWeight: 700,
                      cursor: fetching ? "wait" : "pointer",
                      opacity: fetching || !linkInput.trim() ? 0.5 : 1,
                      whiteSpace: "nowrap",
                    }}
                  >
                    {fetching ? "..." : t("miniProfile.add")}
                  </button>
                </div>
              )}
            </div>
          </div>
        )}

        {/* Heatmap */}
        <div style={{ padding: "0 12px 12px" }}>
          {loading ? (
            <div style={{
              background: "var(--bg-card)",
              borderRadius: "var(--radius-lg)",
              padding: 16,
              boxShadow: "var(--shadow-card)",
            }}>
              <div style={{
                fontSize: 11,
                fontWeight: 700,
                color: "var(--text-secondary)",
                textTransform: "uppercase",
                letterSpacing: "0.5px",
                marginBottom: 10,
              }}>
                {t("activity.weeks", { weeks: 8 })}
              </div>
              <div style={{
                height: 120,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}>
                <div style={{
                  width: 20,
                  height: 20,
                  border: "2px solid var(--text-secondary)",
                  borderTopColor: "var(--accent-purple)",
                  borderRadius: "50%",
                  animation: "miniProfileSpin 0.8s linear infinite",
                }} />
              </div>
            </div>
          ) : error ? (
            <div style={{
              background: "var(--bg-card)",
              borderRadius: "var(--radius-lg)",
              padding: 16,
              boxShadow: "var(--shadow-card)",
              textAlign: "center",
            }}>
              <div style={{
                fontSize: 11,
                color: "var(--text-secondary)",
                fontWeight: 600,
              }}>
                {t("miniProfile.loadError")}
              </div>
            </div>
          ) : (
            <Heatmap daily={daily} weeks={8} />
          )}
        </div>

        {/* GitHub button */}
        <div style={{ padding: "0 12px 16px" }}>
          <button
            onClick={() => openUrl(`https://github.com/${encodeURIComponent(target.nickname)}`)}
            style={{
              width: "100%",
              padding: "10px 16px",
              borderRadius: "var(--radius-sm)",
              border: "1px solid rgba(255, 255, 255, 0.1)",
              background: "var(--bg-primary)",
              color: "var(--text-primary)",
              fontSize: 12,
              fontWeight: 700,
              cursor: "pointer",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 8,
              transition: "background 0.15s ease",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "rgba(255, 255, 255, 0.08)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "var(--bg-primary)";
            }}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
            </svg>
            {t("miniProfile.viewGithub")}
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.5 }}>
              <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
              <polyline points="15 3 21 3 21 9"/>
              <line x1="10" y1="14" x2="21" y2="3"/>
            </svg>
          </button>
        </div>
      </div>

    </div>
  );
}

/* ---- Link Row ---- */

interface LinkRowProps {
  link: { url: string; title: string | null; favicon_url: string | null };
  isMe: boolean;
  onRemove: () => void;
}

function LinkRow({ link, isMe, onRemove }: LinkRowProps) {
  const [hovered, setHovered] = useState(false);
  const [faviconError, setFaviconError] = useState(false);

  const displayTitle = link.title || new URL(link.url).hostname;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "6px 10px",
        borderRadius: "var(--radius-sm)",
        background: hovered ? "rgba(255, 255, 255, 0.04)" : "transparent",
        transition: "background 0.12s ease",
        cursor: "pointer",
      }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onClick={() => openUrl(link.url)}
    >
      {/* Favicon */}
      {link.favicon_url && !faviconError ? (
        <img
          src={link.favicon_url}
          alt=""
          referrerPolicy="no-referrer"
          onError={() => setFaviconError(true)}
          style={{ width: 14, height: 14, borderRadius: 2, flexShrink: 0 }}
        />
      ) : (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--text-secondary)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
          <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/>
          <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>
        </svg>
      )}

      {/* Title */}
      <span style={{
        flex: 1,
        fontSize: 11,
        fontWeight: 600,
        color: "var(--text-primary)",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      }}>
        {displayTitle}
      </span>

      {/* External link icon */}
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0, opacity: hovered ? 0.7 : 0.3 }}>
        <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
        <polyline points="15 3 21 3 21 9"/>
        <line x1="10" y1="14" x2="21" y2="3"/>
      </svg>

      {/* Delete button (own profile only) */}
      {isMe && hovered && (
        <button
          onClick={(e) => { e.stopPropagation(); onRemove(); }}
          style={{
            width: 18,
            height: 18,
            borderRadius: "50%",
            border: "none",
            background: "rgba(255, 80, 80, 0.15)",
            color: "rgba(255, 80, 80, 0.8)",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            padding: 0,
          }}
        >
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
            <line x1="18" y1="6" x2="6" y2="18"/>
            <line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      )}
    </div>
  );
}
