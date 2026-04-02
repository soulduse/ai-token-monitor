import { useState, useEffect, useCallback, useImperativeHandle, forwardRef } from "react";
import { getAllCachedProfiles } from "../lib/profileCache";

interface Props {
  query: string;
  onSelect: (nickname: string) => void;
  onClose: () => void;
  anchorRect: DOMRect | null;
  currentUserId: string;
}

export interface MentionAutocompleteRef {
  handleKeyDown: (e: React.KeyboardEvent) => boolean;
}

interface Suggestion {
  uid: string;
  nickname: string;
  avatar_url: string | null;
}

const MAX_SUGGESTIONS = 5;

export const MentionAutocomplete = forwardRef<MentionAutocompleteRef, Props>(
  function MentionAutocomplete({ query, onSelect, onClose, anchorRect, currentUserId }, ref) {
    const [selectedIndex, setSelectedIndex] = useState(0);
    const [suggestions, setSuggestions] = useState<Suggestion[]>([]);

    // Filter profiles by query
    useEffect(() => {
      const profiles = getAllCachedProfiles();
      const q = query.toLowerCase();
      const filtered: Suggestion[] = [];

      for (const [uid, profile] of profiles) {
        if (uid === currentUserId) continue;
        if (profile.nickname.toLowerCase().startsWith(q)) {
          filtered.push({ uid, nickname: profile.nickname, avatar_url: profile.avatar_url });
        }
        if (filtered.length >= MAX_SUGGESTIONS) break;
      }

      // If no prefix match, try includes
      if (filtered.length === 0 && q.length > 0) {
        for (const [uid, profile] of profiles) {
          if (uid === currentUserId) continue;
          if (profile.nickname.toLowerCase().includes(q)) {
            filtered.push({ uid, nickname: profile.nickname, avatar_url: profile.avatar_url });
          }
          if (filtered.length >= MAX_SUGGESTIONS) break;
        }
      }

      // Still no match (e.g. Korean input) — show all profiles as fallback
      if (filtered.length === 0 && q.length > 0) {
        for (const [uid, profile] of profiles) {
          if (uid === currentUserId) continue;
          filtered.push({ uid, nickname: profile.nickname, avatar_url: profile.avatar_url });
          if (filtered.length >= MAX_SUGGESTIONS) break;
        }
      }

      setSuggestions(filtered);
      setSelectedIndex(0);
    }, [query, currentUserId, onClose]);

    const selectCurrent = useCallback(() => {
      if (suggestions.length > 0 && selectedIndex < suggestions.length) {
        onSelect(suggestions[selectedIndex].nickname);
      }
    }, [suggestions, selectedIndex, onSelect]);

    // Expose keyboard handler to parent
    useImperativeHandle(ref, () => ({
      handleKeyDown(e: React.KeyboardEvent): boolean {
        if (suggestions.length === 0) return false;

        if (e.key === "ArrowDown") {
          e.preventDefault();
          setSelectedIndex((prev) => (prev + 1) % suggestions.length);
          return true;
        }
        if (e.key === "ArrowUp") {
          e.preventDefault();
          setSelectedIndex((prev) => (prev - 1 + suggestions.length) % suggestions.length);
          return true;
        }
        if (e.key === "Enter" || e.key === "Tab") {
          e.preventDefault();
          selectCurrent();
          return true;
        }
        if (e.key === "Escape") {
          e.preventDefault();
          onClose();
          return true;
        }
        return false;
      },
    }), [suggestions, selectCurrent, onClose]);

    if (suggestions.length === 0 || !anchorRect) return null;

    return (
      <>
        {/* Invisible backdrop to absorb outside clicks */}
        <div
          onClick={(e) => { e.stopPropagation(); e.preventDefault(); onClose(); }}
          style={{ position: "fixed", inset: 0, zIndex: 199 }}
        />
        <div style={{
          position: "fixed",
          left: anchorRect.left,
          bottom: window.innerHeight - anchorRect.top + 4,
          background: "var(--bg-card)",
          borderRadius: 10,
          boxShadow: "0 4px 16px rgba(0,0,0,0.2)",
          border: "1px solid rgba(124, 92, 252, 0.15)",
          zIndex: 200,
          padding: 4,
          minWidth: 180,
          maxWidth: 260,
        }}>
        {suggestions.map((s, i) => (
          <button
            key={s.uid}
            onClick={() => onSelect(s.nickname)}
            onMouseEnter={() => setSelectedIndex(i)}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              width: "100%",
              padding: "6px 10px",
              background: i === selectedIndex ? "rgba(124, 92, 252, 0.1)" : "none",
              border: "none",
              borderRadius: 6,
              cursor: "pointer",
              fontSize: 12,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {s.avatar_url ? (
              <img
                src={s.avatar_url}
                alt=""
                style={{ width: 24, height: 24, borderRadius: 8, flexShrink: 0 }}
              />
            ) : (
              <div style={{
                width: 24,
                height: 24,
                borderRadius: 8,
                background: "var(--heat-1)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: 11,
                fontWeight: 700,
                color: "var(--accent-purple)",
                flexShrink: 0,
              }}>
                {s.nickname.charAt(0).toUpperCase()}
              </div>
            )}
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {s.nickname}
            </span>
          </button>
        ))}
      </div>
      </>
    );
  },
);
