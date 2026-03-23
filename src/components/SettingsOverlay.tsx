import { useState, useEffect, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "../contexts/SettingsContext";
import { useAuth } from "../hooks/useAuth";
import { useI18n, LANGUAGE_OPTIONS } from "../i18n/I18nContext";
import type { Locale } from "../i18n/I18nContext";

interface Props {
  visible: boolean;
  onClose: () => void;
}

export function SettingsOverlay({ visible, onClose }: Props) {
  const { prefs, updatePrefs } = useSettings();
  const { user, profile, signIn, signOut, available: leaderboardAvailable } = useAuth();
  const [appVersion, setAppVersion] = useState("");
  const t = useI18n();

  useEffect(() => {
    getVersion().then(setAppVersion);
  }, []);

  if (!visible) return null;

  return (
    <>
      <div
        onClick={onClose}
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 50,
        }}
      />
      <div style={{
        position: "absolute",
        top: 48,
        right: 16,
        background: "var(--bg-card)",
        borderRadius: "var(--radius-md)",
        boxShadow: "0 8px 24px rgba(0,0,0,0.15)",
        padding: 12,
        zIndex: 51,
        minWidth: 220,
        border: "1px solid rgba(124, 92, 252, 0.1)",
      }}>
        <div style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-secondary)",
          textTransform: "uppercase",
          letterSpacing: "0.5px",
          marginBottom: 10,
        }}>
          {t("settings.title")}
        </div>

        {/* Theme selector */}
        <SettingRow label={t("settings.theme")}>
          <ThemeSelector
            value={prefs.theme}
            onChange={(v) => updatePrefs({ theme: v })}
          />
        </SettingRow>

        <SettingRow label={t("settings.appearance")}>
          <ColorModeToggle
            value={prefs.color_mode}
            onChange={(v) => updatePrefs({ color_mode: v })}
          />
        </SettingRow>

        <SettingRow label={t("settings.language")}>
          <LanguageSelector
            value={prefs.language}
            onChange={(v) => updatePrefs({ language: v })}
          />
        </SettingRow>

        <SettingRow
          label={t("settings.numberFormat")}
          description={prefs.number_format === "compact" ? "377.0K" : "377,000"}
        >
          <ToggleButton
            options={["compact", "full"]}
            value={prefs.number_format}
            onChange={(v) => updatePrefs({ number_format: v as "compact" | "full" })}
          />
        </SettingRow>

        <SettingRow label={t("settings.menuBarCost")}>
          <ToggleSwitch
            checked={prefs.show_tray_cost}
            onChange={(v) => updatePrefs({ show_tray_cost: v })}
          />
        </SettingRow>

        {/* Leaderboard section */}
        {leaderboardAvailable && (
          <>
            <div style={{
              height: 1,
              background: "var(--heat-0)",
              margin: "8px 0",
            }} />

            <div style={{
              fontSize: 11,
              fontWeight: 700,
              color: "var(--text-secondary)",
              textTransform: "uppercase",
              letterSpacing: "0.5px",
              marginBottom: 8,
            }}>
              {t("settings.leaderboard")}
            </div>

            <SettingRow label={t("settings.shareUsageData")}>
              <ToggleSwitch
                checked={prefs.leaderboard_opted_in}
                onChange={(v) => updatePrefs({ leaderboard_opted_in: v })}
              />
            </SettingRow>

            {user ? (
              <div style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "6px 0",
              }}>
                <div style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                }}>
                  {profile?.avatar_url && (
                    <img
                      src={profile.avatar_url}
                      alt=""
                      style={{ width: 18, height: 18, borderRadius: 9 }}
                    />
                  )}
                  <span style={{ fontSize: 11, fontWeight: 600, color: "var(--text-primary)" }}>
                    {profile?.nickname ?? t("settings.signedIn")}
                  </span>
                </div>
                <button
                  onClick={signOut}
                  style={{
                    fontSize: 10,
                    fontWeight: 600,
                    padding: "3px 8px",
                    borderRadius: 4,
                    border: "none",
                    cursor: "pointer",
                    background: "var(--heat-0)",
                    color: "var(--text-secondary)",
                  }}
                >
                  {t("settings.signOut")}
                </button>
              </div>
            ) : (
              <button
                onClick={signIn}
                style={{
                  width: "100%",
                  padding: "6px 0",
                  fontSize: 11,
                  fontWeight: 600,
                  border: "none",
                  borderRadius: 4,
                  cursor: "pointer",
                  background: "#24292e",
                  color: "#fff",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  gap: 6,
                  marginTop: 4,
                }}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/>
                </svg>
                {t("settings.signInGithub")}
              </button>
            )}
          </>
        )}

        {/* Quit */}
        <div style={{
          height: 1,
          background: "var(--heat-0)",
          margin: "8px 0",
        }} />

        <div style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}>
          <span style={{
            fontSize: 10,
            color: "var(--text-muted)",
            fontWeight: 500,
          }}>
            v{appVersion}
          </span>
          <button
            onClick={() => invoke("quit_app")}
            style={{
              fontSize: 11,
              fontWeight: 600,
              padding: "4px 12px",
              borderRadius: 6,
              border: "none",
              cursor: "pointer",
              background: "rgba(239, 68, 68, 0.1)",
              color: "#ef4444",
              transition: "background 0.15s ease",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "rgba(239, 68, 68, 0.2)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "rgba(239, 68, 68, 0.1)";
            }}
          >
            {t("settings.quit")}
          </button>
        </div>
      </div>
    </>
  );
}

function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div style={{
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "6px 0",
    }}>
      <div>
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-primary)" }}>{label}</div>
        {description && (
          <div style={{ fontSize: 10, color: "var(--text-secondary)" }}>{description}</div>
        )}
      </div>
      {children}
    </div>
  );
}

function ToggleButton({
  options,
  value,
  onChange,
}: {
  options: string[];
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div style={{
      display: "flex",
      background: "var(--heat-0)",
      borderRadius: 6,
      padding: 2,
    }}>
      {options.map((opt) => (
        <button
          key={opt}
          onClick={() => onChange(opt)}
          style={{
            fontSize: 10,
            fontWeight: 600,
            padding: "3px 8px",
            borderRadius: 4,
            border: "none",
            cursor: "pointer",
            background: value === opt ? "var(--accent-purple)" : "transparent",
            color: value === opt ? "#fff" : "var(--text-secondary)",
            transition: "all 0.15s ease",
          }}
        >
          {opt === "compact" ? "K/M" : "Full"}
        </button>
      ))}
    </div>
  );
}

function ToggleSwitch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div
      onClick={() => onChange(!checked)}
      style={{
        width: 36,
        height: 20,
        borderRadius: 10,
        background: checked ? "var(--accent-purple)" : "var(--heat-0)",
        cursor: "pointer",
        position: "relative",
        transition: "background 0.2s ease",
        flexShrink: 0,
      }}
    >
      <div style={{
        width: 16,
        height: 16,
        borderRadius: 8,
        background: "#fff",
        position: "absolute",
        top: 2,
        left: checked ? 18 : 2,
        transition: "left 0.2s ease",
        boxShadow: "0 1px 3px rgba(0,0,0,0.2)",
      }} />
    </div>
  );
}

const THEMES: { id: "github" | "purple" | "ocean" | "sunset"; label: string; colors: [string, string] }[] = [
  { id: "github", label: "GitHub", colors: ["#2da44e", "#216e39"] },
  { id: "purple", label: "Purple", colors: ["#7C5CFC", "#B5A0EF"] },
  { id: "ocean", label: "Ocean", colors: ["#0284c7", "#38bdf8"] },
  { id: "sunset", label: "Sunset", colors: ["#d97706", "#f59e0b"] },
];

function ThemeSelector({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: "github" | "purple" | "ocean" | "sunset") => void;
}) {
  return (
    <div style={{ display: "flex", gap: 4 }}>
      {THEMES.map((t) => (
        <div
          key={t.id}
          onClick={() => onChange(t.id)}
          title={t.label}
          style={{
            width: 20,
            height: 20,
            borderRadius: 10,
            background: `linear-gradient(135deg, ${t.colors[0]}, ${t.colors[1]})`,
            cursor: "pointer",
            border: value === t.id ? "2px solid var(--text-primary)" : "2px solid transparent",
            transition: "border 0.15s ease",
            flexShrink: 0,
          }}
        />
      ))}
    </div>
  );
}

const COLOR_MODE_IDS: ("system" | "light" | "dark")[] = ["system", "light", "dark"];
const COLOR_MODE_KEYS: Record<string, string> = {
  system: "settings.auto",
  light: "settings.light",
  dark: "settings.dark",
};

function ColorModeToggle({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: "system" | "light" | "dark") => void;
}) {
  const t = useI18n();
  return (
    <div style={{
      display: "flex",
      background: "var(--heat-0)",
      borderRadius: 6,
      padding: 2,
    }}>
      {COLOR_MODE_IDS.map((id) => (
        <button
          key={id}
          onClick={() => onChange(id)}
          style={{
            fontSize: 10,
            fontWeight: 600,
            padding: "3px 8px",
            borderRadius: 4,
            border: "none",
            cursor: "pointer",
            background: value === id ? "var(--accent-purple)" : "transparent",
            color: value === id ? "#fff" : "var(--text-secondary)",
            transition: "all 0.15s ease",
          }}
        >
          {t(COLOR_MODE_KEYS[id])}
        </button>
      ))}
    </div>
  );
}

function LanguageSelector({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: Locale) => void;
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value as Locale)}
      style={{
        fontSize: 10,
        fontWeight: 600,
        padding: "3px 6px",
        borderRadius: 4,
        border: "1px solid var(--heat-1)",
        cursor: "pointer",
        background: "var(--heat-0)",
        color: "var(--text-primary)",
        outline: "none",
      }}
    >
      {LANGUAGE_OPTIONS.map((lang) => (
        <option key={lang.id} value={lang.id}>
          {lang.label}
        </option>
      ))}
    </select>
  );
}
