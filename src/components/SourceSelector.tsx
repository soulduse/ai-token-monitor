import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../contexts/SettingsContext";
import { useI18n } from "../i18n/I18nContext";

export function SourceSelector() {
  const { prefs, updatePrefs } = useSettings();
  const t = useI18n();
  const [codexAvailable, setCodexAvailable] = useState(false);

  useEffect(() => {
    invoke<boolean>("is_codex_available")
      .then(setCodexAvailable)
      .catch(() => setCodexAvailable(false));
  }, []);

  // Hide entirely if Codex is not installed
  if (!codexAvailable) return null;

  const toggleSource = (key: "include_claude" | "include_codex") => {
    const nextValue = !prefs[key];
    const activeCount = Number(prefs.include_claude) + Number(prefs.include_codex);
    // Must keep at least one source active
    if (!nextValue && activeCount === 1) return;
    updatePrefs({ [key]: nextValue });
  };

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

      <div style={{ display: "flex", gap: 8 }}>
        <SourceToggle
          label={t("sources.claude")}
          checked={prefs.include_claude}
          locked={!prefs.include_codex}
          onClick={() => toggleSource("include_claude")}
        />
        <SourceToggle
          label={t("sources.codex")}
          checked={prefs.include_codex}
          locked={!prefs.include_claude}
          onClick={() => toggleSource("include_codex")}
        />
      </div>
    </div>
  );
}

function SourceToggle({
  label,
  checked,
  locked,
  onClick,
}: {
  label: string;
  checked: boolean;
  locked: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={checked}
      onClick={onClick}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "4px 10px",
        borderRadius: 999,
        border: checked ? "1px solid var(--heat-2)" : "1px solid transparent",
        background: checked ? "var(--bg-card)" : "var(--heat-0)",
        color: checked ? "var(--text-primary)" : "var(--text-secondary)",
        fontSize: 11,
        fontWeight: 700,
        cursor: locked ? "default" : "pointer",
        opacity: locked && checked ? 0.88 : 1,
        boxShadow: checked ? "0 1px 4px rgba(0,0,0,0.06)" : "none",
        transition: "all 0.15s ease",
      }}
    >
      <span style={{
        width: 12,
        height: 12,
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
          <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        ) : null}
      </span>
      {label}
    </button>
  );
}
