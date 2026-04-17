import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

type SourceKey =
  | "include_claude"
  | "include_codex"
  | "include_opencode"
  | "include_kimi"
  | "include_glm";

interface SourceDef {
  key: SourceKey;
  label: string;
  available: boolean;
}

export function SourceSelector() {
  const { prefs, updatePrefs } = useSettings();
  const t = useI18n();
  const [codexAvailable, setCodexAvailable] = useState(false);
  const [opencodeAvailable, setOpencodeAvailable] = useState(false);
  const [kimiAvailable, setKimiAvailable] = useState(false);
  const [glmAvailable, setGlmAvailable] = useState(false);
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<boolean>("is_codex_available")
      .then(setCodexAvailable)
      .catch(() => setCodexAvailable(false));
    invoke<boolean>("is_opencode_available")
      .then(setOpencodeAvailable)
      .catch(() => setOpencodeAvailable(false));
    invoke<boolean>("is_kimi_available")
      .then(setKimiAvailable)
      .catch(() => setKimiAvailable(false));
    invoke<boolean>("is_glm_available")
      .then(setGlmAvailable)
      .catch(() => setGlmAvailable(false));
  }, []);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const sources: SourceDef[] = useMemo(() => [
    { key: "include_claude", label: t("sources.claude"), available: true },
    { key: "include_codex", label: t("sources.codex"), available: codexAvailable },
    { key: "include_opencode", label: t("sources.opencode"), available: opencodeAvailable },
    { key: "include_kimi", label: t("sources.kimi"), available: kimiAvailable },
    { key: "include_glm", label: t("sources.glm"), available: glmAvailable },
  ], [t, codexAvailable, opencodeAvailable, kimiAvailable, glmAvailable]);

  const visibleSources = sources.filter((s) => s.available);
  const totalCount = visibleSources.length;
  const activeCount = visibleSources.reduce((n, s) => n + (prefs[s.key] ? 1 : 0), 0);

  // Hide entirely if only Claude is available (no multi-source UX needed)
  if (totalCount <= 1) return null;

  const toggleSource = (key: SourceKey) => {
    const nextValue = !prefs[key];
    // Must keep at least one source active among visible/available ones
    const currentActiveVisible = visibleSources.reduce(
      (n, s) => n + (prefs[s.key] ? 1 : 0),
      0,
    );
    if (!nextValue && currentActiveVisible === 1 && prefs[key]) return;
    updatePrefs({ [key]: nextValue });
  };

  const summary = t("sources.summary.count", {
    active: String(activeCount),
    total: String(totalCount),
  });

  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      gap: 10,
      marginTop: -4,
    }}>
      <div style={{
        fontSize: 10,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
      }}>
        {t("sources.title")}
      </div>

      <div ref={menuRef} style={{ position: "relative" }}>
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={t("sources.open")}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "4px 10px",
            borderRadius: 999,
            border: open ? "1px solid var(--accent-purple)" : "1px solid var(--heat-2)",
            background: "var(--bg-card)",
            color: "var(--text-primary)",
            fontSize: 11,
            fontWeight: 700,
            cursor: "pointer",
            transition: "border-color 0.15s ease",
          }}
        >
          <span>{summary}</span>
          <svg
            width="10"
            height="10"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="3"
            strokeLinecap="round"
            strokeLinejoin="round"
            style={{
              transform: open ? "rotate(180deg)" : "none",
              transition: "transform 0.15s ease",
            }}
            aria-hidden="true"
          >
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>

        {open && (
          <div
            role="menu"
            style={{
              position: "absolute",
              top: "calc(100% + 6px)",
              right: 0,
              minWidth: 180,
              padding: 4,
              background: "var(--bg-card)",
              borderRadius: 10,
              border: "1px solid rgba(128,128,128,0.15)",
              boxShadow: "0 12px 32px rgba(0,0,0,0.18), 0 2px 8px rgba(0,0,0,0.08)",
              zIndex: 60,
              transformOrigin: "top right",
              animation: "headerMenuPop 0.16s cubic-bezier(.2,.9,.2,1) both",
            }}
          >
            {visibleSources.map((s) => {
              const checked = !!prefs[s.key];
              const activeVisible = visibleSources.reduce(
                (n, v) => n + (prefs[v.key] ? 1 : 0),
                0,
              );
              const locked = checked && activeVisible === 1;
              return (
                <button
                  key={s.key}
                  role="menuitemcheckbox"
                  aria-checked={checked}
                  onClick={() => toggleSource(s.key)}
                  disabled={locked}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    width: "100%",
                    padding: "6px 8px",
                    background: "none",
                    border: "none",
                    borderRadius: 6,
                    cursor: locked ? "default" : "pointer",
                    color: "var(--text-primary)",
                    fontSize: 12,
                    fontWeight: 600,
                    textAlign: "left",
                    opacity: locked ? 0.7 : 1,
                  }}
                >
                  <span style={{
                    width: 14,
                    height: 14,
                    borderRadius: 3,
                    border: checked ? "1px solid var(--accent-purple)" : "1px solid var(--heat-2)",
                    background: checked ? "var(--accent-purple)" : "transparent",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    color: "#fff",
                    flexShrink: 0,
                  }}>
                    {checked ? (
                      <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                        <polyline points="20 6 9 17 4 12" />
                      </svg>
                    ) : null}
                  </span>
                  <span>{s.label}</span>
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
