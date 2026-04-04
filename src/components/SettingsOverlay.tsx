import { useState, useEffect, useCallback, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useSettings } from "../contexts/SettingsContext";
import { useAuth } from "../hooks/useAuth";
import { useI18n, LANGUAGE_OPTIONS } from "../i18n/I18nContext";
import { InfoTooltip } from "./InfoTooltip";
import type { Locale } from "../i18n/I18nContext";

type SettingsTab = "general" | "account" | "ai" | "webhooks";

interface Props {
  visible: boolean;
  onClose: () => void;
}

export function SettingsOverlay({ visible, onClose }: Props) {
  const { prefs, updatePrefs } = useSettings();
  const { user, profile, signIn, signOut, available: leaderboardAvailable } = useAuth();
  const [appVersion, setAppVersion] = useState("");
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
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
        width: 280,
        maxHeight: "calc(100vh - 80px)",
        display: "flex",
        flexDirection: "column",
        border: "1px solid rgba(124, 92, 252, 0.1)",
      }}>
        <div style={{
          fontSize: 11,
          fontWeight: 700,
          color: "var(--text-secondary)",
          textTransform: "uppercase",
          letterSpacing: "0.5px",
          marginBottom: 8,
        }}>
          {t("settings.title")}
        </div>

        {/* Tab bar */}
        <div style={{
          display: "flex",
          gap: 0,
          marginBottom: 10,
          borderBottom: "1px solid var(--heat-0)",
        }}>
          {(["general", "account", "ai", "webhooks"] as SettingsTab[]).map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              style={{
                flex: 1,
                padding: "6px 8px",
                fontSize: 10,
                fontWeight: 700,
                border: "none",
                borderBottom: activeTab === tab ? "2px solid var(--accent-purple)" : "2px solid transparent",
                cursor: "pointer",
                background: "transparent",
                color: activeTab === tab ? "var(--accent-purple)" : "var(--text-muted)",
                transition: "all 0.15s ease",
              }}
            >
              {t(`settings.tab.${tab}`)}
            </button>
          ))}
        </div>

        {/* Tab content */}
        <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
          {activeTab === "general" && (
            <GeneralTab prefs={prefs} updatePrefs={updatePrefs} />
          )}
          {activeTab === "account" && (
            <AccountTab
              prefs={prefs}
              updatePrefs={updatePrefs}
              user={user}
              profile={profile}
              signIn={signIn}
              signOut={signOut}
              leaderboardAvailable={leaderboardAvailable}
            />
          )}
          {activeTab === "ai" && (
            <AiTranslationSection
              aiKeys={prefs.ai_keys}
              aiModel={prefs.ai_model}
              onKeysChange={(keys) => updatePrefs({ ai_keys: keys })}
              onModelChange={(model) => updatePrefs({ ai_model: model })}
            />
          )}
          {activeTab === "webhooks" && (
            <WebhooksTab
              prefs={prefs}
              updatePrefs={updatePrefs}
            />
          )}
        </div>

        {/* Footer: version + quit */}
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

/* ========== General Tab ========== */

function GeneralTab({
  prefs,
  updatePrefs,
}: {
  prefs: ReturnType<typeof useSettings>["prefs"];
  updatePrefs: ReturnType<typeof useSettings>["updatePrefs"];
}) {
  return (
    <div>
      <SettingRow label={useI18n()("settings.theme")}>
        <ThemeSelector
          value={prefs.theme}
          onChange={(v) => updatePrefs({ theme: v })}
        />
      </SettingRow>

      <SettingRow label={useI18n()("settings.appearance")}>
        <ColorModeToggle
          value={prefs.color_mode}
          onChange={(v) => updatePrefs({ color_mode: v })}
        />
      </SettingRow>

      <SettingRow label={useI18n()("settings.language")}>
        <LanguageSelector
          value={prefs.language}
          onChange={(v) => updatePrefs({ language: v })}
        />
      </SettingRow>

      <SettingRow
        label={useI18n()("settings.numberFormat")}
        description={prefs.number_format === "compact" ? "377.0K" : "377,000"}
      >
        <ToggleButton
          options={["compact", "full"]}
          value={prefs.number_format}
          onChange={(v) => updatePrefs({ number_format: v as "compact" | "full" })}
        />
      </SettingRow>

      <SettingRow label={useI18n()("settings.menuBarCost")}>
        <ToggleSwitch
          checked={prefs.show_tray_cost}
          onChange={(v) => updatePrefs({ show_tray_cost: v })}
        />
      </SettingRow>

      <SettingRow label={useI18n()("settings.usageTracking")}>
        <ToggleSwitch
          checked={prefs.usage_tracking_enabled}
          onChange={(v) => updatePrefs({ usage_tracking_enabled: v })}
        />
      </SettingRow>

      <SettingRow label={useI18n()("settings.monthlySalary")}>
        <ToggleSwitch
          checked={prefs.salary_enabled}
          onChange={(v) => updatePrefs({ salary_enabled: v })}
        />
      </SettingRow>

      {prefs.salary_enabled && (
        <SettingRow label="" description="USD">
          <input
            type="number"
            min={0}
            step={100}
            value={prefs.monthly_salary ?? ""}
            placeholder="—"
            onChange={(e) => {
              const val = e.target.value;
              updatePrefs({ monthly_salary: val ? Number(val) : undefined });
            }}
            style={{
              width: 80,
              fontSize: 11,
              fontWeight: 600,
              padding: "3px 6px",
              borderRadius: 4,
              border: "1px solid var(--heat-1)",
              background: "var(--heat-0)",
              color: "var(--text-primary)",
              outline: "none",
              textAlign: "right",
            }}
          />
        </SettingRow>
      )}
    </div>
  );
}

/* ========== Account Tab ========== */

function AccountTab({
  prefs,
  updatePrefs,
  user,
  profile,
  signIn,
  signOut,
  leaderboardAvailable,
}: {
  prefs: ReturnType<typeof useSettings>["prefs"];
  updatePrefs: ReturnType<typeof useSettings>["updatePrefs"];
  user: ReturnType<typeof useAuth>["user"];
  profile: ReturnType<typeof useAuth>["profile"];
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
  leaderboardAvailable: boolean;
}) {
  const t = useI18n();

  return (
    <div>
      <ConfigDirsSection
        provider="claude"
        dirs={prefs.config_dirs}
        onChange={(dirs) => updatePrefs({ config_dirs: dirs })}
      />

      {prefs.include_codex && (
        <>
          <div style={{
            height: 1,
            background: "var(--heat-0)",
            margin: "8px 0",
          }} />
          <ConfigDirsSection
            provider="codex"
            dirs={prefs.codex_dirs}
            onChange={(dirs) => updatePrefs({ codex_dirs: dirs })}
          />
        </>
      )}

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
    </div>
  );
}

/* ========== Shared Components ========== */

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

type ConfigDirProvider = "claude" | "codex";

const DIR_CONFIG: Record<ConfigDirProvider, {
  titleKey: string;
  tooltipKey: string;
  detectCmd: string;
  validateCmd: string;
  defaultSubdir: string;
  invalidKey: string;
}> = {
  claude: {
    titleKey: "settings.configDirs",
    tooltipKey: "settings.configDirsTooltip",
    detectCmd: "detect_claude_dirs",
    validateCmd: "validate_claude_dir",
    defaultSubdir: ".claude",
    invalidKey: "settings.configDirsInvalid",
  },
  codex: {
    titleKey: "settings.codexConfigDirs",
    tooltipKey: "settings.codexConfigDirsTooltip",
    detectCmd: "detect_codex_dirs",
    validateCmd: "validate_codex_dir",
    defaultSubdir: ".codex",
    invalidKey: "settings.codexConfigDirsInvalid",
  },
};

function ConfigDirsSection({
  provider = "claude",
  dirs,
  onChange,
}: {
  provider?: ConfigDirProvider;
  dirs: string[];
  onChange: (dirs: string[]) => void;
}) {
  const t = useI18n();
  const cfg = DIR_CONFIG[provider];
  const [detecting, setDetecting] = useState(false);
  const [message, setMessage] = useState("");

  const showMessage = useCallback((msg: string) => {
    setMessage(msg);
    setTimeout(() => setMessage(""), 3000);
  }, []);

  const handleAutoDetect = useCallback(async () => {
    setDetecting(true);
    try {
      const found = await invoke<string[]>(cfg.detectCmd);
      const newDirs = found.filter((d) => !dirs.includes(d));
      if (newDirs.length > 0) {
        onChange([...dirs, ...newDirs]);
        showMessage(t("settings.configDirsFound", { count: String(newDirs.length) }));
      } else {
        showMessage(t("settings.configDirsNotFound"));
      }
    } catch {
      showMessage(t("settings.configDirsNotFound"));
    } finally {
      setDetecting(false);
    }
  }, [dirs, onChange, showMessage, t, cfg.detectCmd]);

  const handleAddFolder = useCallback(async () => {
    try {
      await invoke("set_dialog_open", { open: true });
      const home = await invoke<string>("get_home_dir");
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: home ? `${home}/${cfg.defaultSubdir}` : undefined,
      });
      if (!selected) return;
      const homePath = selected.replace(/^\/Users\/[^/]+/, "~");
      if (dirs.includes(homePath) || dirs.includes(selected)) return;
      const valid = await invoke<boolean>(cfg.validateCmd, { path: homePath });
      if (valid) {
        onChange([...dirs, homePath]);
      } else {
        showMessage(t(cfg.invalidKey));
      }
    } finally {
      await invoke("set_dialog_open", { open: false });
    }
  }, [dirs, onChange, showMessage, t, cfg.validateCmd, cfg.defaultSubdir, cfg.invalidKey]);

  const handleRemove = useCallback((dir: string) => {
    onChange(dirs.filter((d) => d !== dir));
  }, [dirs, onChange]);

  return (
    <div>
      <div style={{
        display: "flex",
        alignItems: "center",
        gap: 4,
        fontSize: 11,
        fontWeight: 700,
        color: "var(--text-secondary)",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
        marginBottom: 8,
      }}>
        {t(cfg.titleKey)}
        <InfoTooltip>{t(cfg.tooltipKey)}</InfoTooltip>
      </div>

      <div style={{
        maxHeight: 120,
        overflowY: "auto",
        marginBottom: 6,
      }}>
        {dirs.map((dir, i) => (
          <div
            key={dir}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: "3px 0",
              fontSize: 11,
              color: "var(--text-primary)",
            }}
          >
            <span style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              flex: 1,
              fontWeight: 500,
            }}>
              {dir}
            </span>
            {i === 0 ? (
              <span style={{
                fontSize: 9,
                fontWeight: 600,
                color: "var(--text-muted)",
                marginLeft: 4,
                flexShrink: 0,
              }}>
                ({t("settings.configDirsPrimary")})
              </span>
            ) : (
              <button
                onClick={() => handleRemove(dir)}
                style={{
                  fontSize: 10,
                  fontWeight: 700,
                  width: 18,
                  height: 18,
                  borderRadius: 4,
                  border: "none",
                  cursor: "pointer",
                  background: "transparent",
                  color: "var(--text-muted)",
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flexShrink: 0,
                  marginLeft: 4,
                  transition: "color 0.15s ease",
                }}
                onMouseEnter={(e) => { e.currentTarget.style.color = "#ef4444"; }}
                onMouseLeave={(e) => { e.currentTarget.style.color = "var(--text-muted)"; }}
              >
                x
              </button>
            )}
          </div>
        ))}
      </div>

      <div style={{ display: "flex", gap: 4 }}>
        <button
          onClick={handleAutoDetect}
          disabled={detecting}
          style={{
            fontSize: 10,
            fontWeight: 600,
            padding: "4px 8px",
            borderRadius: 4,
            border: "none",
            cursor: detecting ? "wait" : "pointer",
            background: "var(--heat-0)",
            color: "var(--text-secondary)",
            opacity: detecting ? 0.6 : 1,
            transition: "background 0.15s ease",
          }}
          onMouseEnter={(e) => { if (!detecting) e.currentTarget.style.background = "var(--heat-1)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.background = "var(--heat-0)"; }}
        >
          {detecting ? "..." : t("settings.configDirsAutoDetect")}
        </button>
        <button
          onClick={handleAddFolder}
          style={{
            fontSize: 10,
            fontWeight: 600,
            padding: "4px 8px",
            borderRadius: 4,
            border: "none",
            cursor: "pointer",
            background: "var(--heat-0)",
            color: "var(--text-secondary)",
            transition: "background 0.15s ease",
          }}
          onMouseEnter={(e) => { e.currentTarget.style.background = "var(--heat-1)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.background = "var(--heat-0)"; }}
        >
          + {t("settings.configDirsAdd")}
        </button>
      </div>

      {message && (
        <div style={{
          fontSize: 10,
          color: "var(--text-muted)",
          marginTop: 4,
          fontWeight: 500,
        }}>
          {message}
        </div>
      )}
    </div>
  );
}

/* ========== AI Translation Tab ========== */

interface AiProvider {
  id: string;
  i18nKey: string;
  models: { id: string; label: string }[];
}

const AI_PROVIDERS: AiProvider[] = [
  {
    id: "gemini",
    i18nKey: "settings.aiKeyGemini",
    models: [
      { id: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
      { id: "gemini-2.5-flash-lite", label: "Gemini 2.5 Flash Lite" },
      { id: "gemini-2.5-pro", label: "Gemini 2.5 Pro" },
      { id: "gemini-3-flash-preview", label: "Gemini 3 Flash" },
      { id: "gemini-3.1-flash-lite-preview", label: "Gemini 3.1 Flash Lite" },
      { id: "gemini-3.1-pro-preview", label: "Gemini 3.1 Pro" },
    ],
  },
  {
    id: "openai",
    i18nKey: "settings.aiKeyOpenAI",
    models: [
      { id: "gpt-5-mini", label: "GPT-5 Mini" },
      { id: "gpt-5.4", label: "GPT-5.4" },
      { id: "gpt-5.4-mini", label: "GPT-5.4 Mini" },
      { id: "gpt-5.4-nano", label: "GPT-5.4 Nano" },
    ],
  },
  {
    id: "anthropic",
    i18nKey: "settings.aiKeyAnthropic",
    models: [
      { id: "claude-haiku-4-5-20251001", label: "Claude Haiku 4.5" },
      { id: "claude-sonnet-4-6", label: "Claude Sonnet 4.6" },
    ],
  },
];

function AiTranslationSection({
  aiKeys,
  aiModel,
  onKeysChange,
  onModelChange,
}: {
  aiKeys?: { gemini?: string; openai?: string; anthropic?: string };
  aiModel?: string;
  onKeysChange: (keys: { gemini?: string; openai?: string; anthropic?: string }) => void;
  onModelChange: (model: string | undefined) => void;
}) {
  const t = useI18n();
  const keys = aiKeys ?? {};

  const availableModels = AI_PROVIDERS.flatMap((p) => {
    const key = keys[p.id as keyof typeof keys];
    return key ? p.models : [];
  });

  const handleKeyChange = (provider: string, value: string) => {
    const trimmed = value.trim();
    onKeysChange({ ...keys, [provider]: trimmed || undefined });
  };

  const keyInputStyle = {
    width: "100%",
    fontSize: 10,
    fontWeight: 500,
    padding: "4px 8px",
    borderRadius: 4,
    border: "1px solid var(--heat-1)",
    background: "var(--heat-0)",
    color: "var(--text-primary)",
    outline: "none",
    fontFamily: "monospace",
  } as const;

  return (
    <div>
      <div style={{
        fontSize: 12,
        color: "var(--text-secondary)",
        marginBottom: 12,
        lineHeight: 1.6,
        padding: "10px 12px",
        background: "rgba(124, 92, 252, 0.05)",
        borderRadius: 8,
        border: "1px solid rgba(124, 92, 252, 0.1)",
      }}>
        <div style={{ fontSize: 13, fontWeight: 700, marginBottom: 6, color: "var(--text-primary)" }}>{t("settings.aiDescription")}</div>
        <div style={{ fontSize: 12, opacity: 0.85, whiteSpace: "pre-line", lineHeight: 1.6 }}>{t("settings.aiFeatures")}</div>
        <div style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          marginTop: 8,
          fontSize: 11,
          opacity: 0.7,
        }}>
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
            <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
            <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
          </svg>
          <span>{t("settings.aiKeySecure")}</span>
        </div>
      </div>

      {AI_PROVIDERS.map((provider) => {
        const key = keys[provider.id as keyof typeof keys] ?? "";
        return (
          <div key={provider.id} style={{ marginBottom: 6 }}>
            <div style={{ fontSize: 10, fontWeight: 600, color: "var(--text-secondary)", marginBottom: 2 }}>
              {t(provider.i18nKey)}
            </div>
            <input
              type="password"
              value={key}
              onChange={(e) => handleKeyChange(provider.id, e.target.value)}
              placeholder={t("settings.aiKeyPlaceholder")}
              style={keyInputStyle}
            />
          </div>
        );
      })}

      {availableModels.length > 0 ? (
        <SettingRow label={t("settings.aiModel")}>
          <select
            value={aiModel ?? ""}
            onChange={(e) => onModelChange(e.target.value || undefined)}
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
              maxWidth: 140,
            }}
          >
            <option value="">—</option>
            {availableModels.map((m) => (
              <option key={m.id} value={m.id}>{m.label}</option>
            ))}
          </select>
        </SettingRow>
      ) : (
        <div style={{ fontSize: 10, color: "var(--text-muted)", fontWeight: 500, marginTop: 4 }}>
          {t("settings.aiNoKeys")}
        </div>
      )}
    </div>
  );
}

/* ========== Webhooks Tab ========== */

function WebhooksTab({
  prefs,
  updatePrefs,
}: {
  prefs: ReturnType<typeof useSettings>["prefs"];
  updatePrefs: ReturnType<typeof useSettings>["updatePrefs"];
}) {
  const t = useI18n();
  const keys = prefs.ai_keys ?? {};
  const config = prefs.webhook_config ?? {
    discord_enabled: false,
    slack_enabled: false,
    telegram_enabled: false,
    thresholds: [50, 80, 90],
    notify_on_reset: false,
    monitored_windows: {
      five_hour: true,
      seven_day: true,
      seven_day_sonnet: false,
      seven_day_opus: false,
      extra_usage: false,
    },
  };

  const [testing, setTesting] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<{ platform: string; ok: boolean; msg: string } | null>(null);

  const updateConfig = (partial: Partial<typeof config>) => {
    updatePrefs({ webhook_config: { ...config, ...partial } });
  };

  const updateKeys = (partial: Partial<typeof keys>) => {
    updatePrefs({ ai_keys: { ...keys, ...partial } });
  };

  const handleTest = async (platform: string) => {
    setTesting(platform);
    setTestResult(null);
    try {
      const msg = await invoke<string>("test_webhook", { platform });
      setTestResult({ platform, ok: true, msg });
    } catch (e) {
      setTestResult({ platform, ok: false, msg: String(e) });
    } finally {
      setTesting(null);
      setTimeout(() => setTestResult(null), 4000);
    }
  };

  const keyInputStyle = {
    width: "100%",
    fontSize: 10,
    fontWeight: 500,
    padding: "4px 8px",
    borderRadius: 4,
    border: "1px solid var(--heat-1)",
    background: "var(--heat-0)",
    color: "var(--text-primary)",
    outline: "none",
    fontFamily: "monospace",
  } as const;

  const testBtnStyle = (platform: string) => ({
    fontSize: 9,
    fontWeight: 600 as const,
    padding: "3px 8px",
    borderRadius: 4,
    border: "none",
    cursor: testing === platform ? "wait" as const : "pointer" as const,
    background: "var(--accent-purple)",
    color: "#fff",
    opacity: testing === platform ? 0.6 : 1,
    flexShrink: 0 as const,
    transition: "opacity 0.15s ease",
  });

  return (
    <div>
      {/* Description */}
      <div style={{
        fontSize: 12,
        color: "var(--text-secondary)",
        marginBottom: 12,
        lineHeight: 1.6,
        padding: "10px 12px",
        background: "rgba(124, 92, 252, 0.05)",
        borderRadius: 8,
        border: "1px solid rgba(124, 92, 252, 0.1)",
      }}>
        <div style={{ fontSize: 13, fontWeight: 700, marginBottom: 6, color: "var(--text-primary)" }}>
          {t("settings.webhookTitle")}
        </div>
        <div style={{ fontSize: 12, opacity: 0.85, whiteSpace: "pre-line", lineHeight: 1.6 }}>
          {t("settings.webhookDescription")}
        </div>
        <div style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          marginTop: 8,
          fontSize: 11,
          opacity: 0.7,
        }}>
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
            <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
            <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
          </svg>
          <span>{t("settings.webhookSecure")}</span>
        </div>
      </div>

      {/* Discord */}
      <WebhookPlatformSection
        label="Discord"
        guide={t("settings.webhookGuideDiscord")}
        enabled={config.discord_enabled}
        onToggle={(v) => updateConfig({ discord_enabled: v })}
        urlValue={keys.webhook_discord_url ?? ""}
        urlPlaceholder="https://discord.com/api/webhooks/..."
        onUrlChange={(v) => updateKeys({ webhook_discord_url: v || undefined })}
        onTest={() => handleTest("discord")}
        testing={testing === "discord"}
        testResult={testResult?.platform === "discord" ? testResult : null}
        keyInputStyle={keyInputStyle}
        testBtnStyle={testBtnStyle("discord")}
      />

      {/* Slack */}
      <WebhookPlatformSection
        label="Slack"
        guide={t("settings.webhookGuideSlack")}
        enabled={config.slack_enabled}
        onToggle={(v) => updateConfig({ slack_enabled: v })}
        urlValue={keys.webhook_slack_url ?? ""}
        urlPlaceholder="https://hooks.slack.com/services/..."
        onUrlChange={(v) => updateKeys({ webhook_slack_url: v || undefined })}
        onTest={() => handleTest("slack")}
        testing={testing === "slack"}
        testResult={testResult?.platform === "slack" ? testResult : null}
        keyInputStyle={keyInputStyle}
        testBtnStyle={testBtnStyle("slack")}
      />

      {/* Telegram */}
      <div style={{ marginBottom: 10 }}>
        <div style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 4,
        }}>
          <span style={{ fontSize: 11, fontWeight: 700, color: "var(--text-primary)" }}>Telegram</span>
          <ToggleSwitch
            checked={config.telegram_enabled}
            onChange={(v) => updateConfig({ telegram_enabled: v })}
          />
        </div>
        <div style={{ fontSize: 9, color: "var(--text-muted)", marginBottom: 4, lineHeight: 1.4 }}>
          {t("settings.webhookGuideTelegram")}
        </div>
        <div style={{ marginBottom: 3 }}>
          <div style={{ fontSize: 9, fontWeight: 600, color: "var(--text-secondary)", marginBottom: 1 }}>Bot Token</div>
          <input
            type="password"
            value={keys.webhook_telegram_bot_token ?? ""}
            onChange={(e) => updateKeys({ webhook_telegram_bot_token: e.target.value.trim() || undefined })}
            placeholder="123456:ABC-DEF..."
            style={keyInputStyle}
          />
        </div>
        <div style={{ marginBottom: 3 }}>
          <div style={{ fontSize: 9, fontWeight: 600, color: "var(--text-secondary)", marginBottom: 1 }}>Chat ID</div>
          <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
            <input
              type="text"
              value={keys.webhook_telegram_chat_id ?? ""}
              onChange={(e) => updateKeys({ webhook_telegram_chat_id: e.target.value.trim() || undefined })}
              placeholder="-1001234567890"
              style={keyInputStyle}
            />
            <button
              onClick={() => handleTest("telegram")}
              disabled={testing === "telegram"}
              style={testBtnStyle("telegram")}
            >
              {testing === "telegram" ? "..." : t("settings.webhookTest")}
            </button>
          </div>
        </div>
        {testResult?.platform === "telegram" && (
          <div style={{
            fontSize: 10,
            color: testResult.ok ? "#22c55e" : "#ef4444",
            fontWeight: 500,
            marginTop: 2,
          }}>
            {testResult.msg}
          </div>
        )}
      </div>

      {/* Divider */}
      <div style={{ height: 1, background: "var(--heat-0)", margin: "10px 0" }} />

      {/* Alert Settings */}
      <div style={{ fontSize: 11, fontWeight: 700, color: "var(--text-secondary)", textTransform: "uppercase", letterSpacing: "0.5px", marginBottom: 8 }}>
        {t("settings.webhookAlertSettings")}
      </div>

      {/* Thresholds */}
      <div style={{ marginBottom: 8 }}>
        <div style={{ fontSize: 10, fontWeight: 600, color: "var(--text-secondary)", marginBottom: 4 }}>
          {t("settings.webhookThresholds")}
        </div>
        <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
          {config.thresholds.map((threshold, i) => (
            <div key={i} style={{
              display: "flex",
              alignItems: "center",
              background: "var(--heat-0)",
              borderRadius: 4,
              padding: "2px 4px",
              gap: 2,
            }}>
              <span style={{ fontSize: 10, fontWeight: 600, color: "var(--text-primary)" }}>{threshold}%</span>
              <button
                onClick={() => {
                  const next = config.thresholds.filter((_, j) => j !== i);
                  updateConfig({ thresholds: next.length > 0 ? next : [50] });
                }}
                style={{
                  fontSize: 9,
                  fontWeight: 700,
                  border: "none",
                  background: "transparent",
                  cursor: "pointer",
                  color: "var(--text-muted)",
                  padding: "0 2px",
                }}
              >
                x
              </button>
            </div>
          ))}
          <button
            onClick={() => {
              const existing = new Set(config.thresholds);
              const candidates = [25, 50, 75, 80, 90, 95];
              const next = candidates.find((c) => !existing.has(c));
              if (next !== undefined) {
                updateConfig({ thresholds: [...config.thresholds, next].sort((a, b) => a - b) });
              }
            }}
            style={{
              fontSize: 10,
              fontWeight: 600,
              padding: "2px 6px",
              borderRadius: 4,
              border: "1px dashed var(--heat-1)",
              background: "transparent",
              cursor: "pointer",
              color: "var(--text-muted)",
            }}
          >
            +
          </button>
        </div>
      </div>

      {/* Monitored Windows */}
      <div style={{ marginBottom: 8 }}>
        <div style={{ fontSize: 10, fontWeight: 600, color: "var(--text-secondary)", marginBottom: 4 }}>
          {t("settings.webhookWindows")}
        </div>
        {([
          ["five_hour", t("usageAlert.session")] as const,
          ["seven_day", t("usageAlert.weekly")] as const,
          ["seven_day_sonnet", "Weekly Sonnet"] as const,
          ["seven_day_opus", "Weekly Opus"] as const,
          ["extra_usage", t("usageAlert.extraUsage")] as const,
        ]).map(([key, label]) => (
          <div key={key} style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "3px 0",
          }}>
            <span style={{ fontSize: 11, fontWeight: 500, color: "var(--text-primary)" }}>{label}</span>
            <ToggleSwitch
              checked={config.monitored_windows[key]}
              onChange={(v) =>
                updateConfig({
                  monitored_windows: { ...config.monitored_windows, [key]: v },
                })
              }
            />
          </div>
        ))}
      </div>

      {/* Notify on reset */}
      <SettingRow label={t("settings.webhookNotifyReset")}>
        <ToggleSwitch
          checked={config.notify_on_reset}
          onChange={(v) => updateConfig({ notify_on_reset: v })}
        />
      </SettingRow>
    </div>
  );
}

function WebhookPlatformSection({
  label,
  guide,
  enabled,
  onToggle,
  urlValue,
  urlPlaceholder,
  onUrlChange,
  onTest,
  testing,
  testResult,
  keyInputStyle,
  testBtnStyle,
}: {
  label: string;
  guide?: string;
  enabled: boolean;
  onToggle: (v: boolean) => void;
  urlValue: string;
  urlPlaceholder: string;
  onUrlChange: (v: string) => void;
  onTest: () => void;
  testing: boolean;
  testResult: { ok: boolean; msg: string } | null;
  keyInputStyle: React.CSSProperties;
  testBtnStyle: React.CSSProperties;
}) {
  const t = useI18n();
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: 4,
      }}>
        <span style={{ fontSize: 11, fontWeight: 700, color: "var(--text-primary)" }}>{label}</span>
        <ToggleSwitch checked={enabled} onChange={onToggle} />
      </div>
      {guide && (
        <div style={{ fontSize: 9, color: "var(--text-muted)", marginBottom: 4, lineHeight: 1.4 }}>
          {guide}
        </div>
      )}
      <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
        <input
          type="password"
          value={urlValue}
          onChange={(e) => onUrlChange(e.target.value.trim())}
          placeholder={urlPlaceholder}
          style={keyInputStyle}
        />
        <button
          onClick={onTest}
          disabled={testing}
          style={testBtnStyle}
        >
          {testing ? "..." : t("settings.webhookTest")}
        </button>
      </div>
      {testResult && (
        <div style={{
          fontSize: 10,
          color: testResult.ok ? "#22c55e" : "#ef4444",
          fontWeight: 500,
          marginTop: 2,
        }}>
          {testResult.msg}
        </div>
      )}
    </div>
  );
}
