import type { UserPreferences } from "./types";

type TranslationPrefs = Pick<UserPreferences, "ai_keys" | "ai_model" | "translation_provider">;

export interface CliTool {
  name: string;
  available: boolean;
}

/**
 * CLIs used without an explicit choice (backend `DEFAULT_CLI_NAMES`). Codex's
 * tools can only be removed by pinning its model metadata, so it runs only
 * when picked in Settings.
 */
const DEFAULT_CLIS = ["gemini", "claude"];

/** The CLI translation runs through when none is chosen — first detected default. */
export function defaultCli(tools: CliTool[]): string | undefined {
  return tools.find((tool) => tool.available && DEFAULT_CLIS.includes(tool.name))?.name;
}

/** A translation model key + selected model — what the API path needs. */
export function hasApiTranslation(prefs: TranslationPrefs): boolean {
  const keys = prefs.ai_keys;
  const hasKey = !!(keys?.gemini || keys?.openai || keys?.anthropic || keys?.kiro);
  return hasKey && !!prefs.ai_model;
}

/**
 * Mirrors `UserPreferences::translates_with_cli` in the backend: an explicit
 * choice always wins; with none saved, a configured API key + model keeps the
 * API path, and otherwise a detected default CLI (see `defaultCli`) is used.
 */
export function translatesWithCli(prefs: TranslationPrefs, defaultCliDetected: boolean): boolean {
  if (prefs.translation_provider) return prefs.translation_provider === "cli";
  return !hasApiTranslation(prefs) && defaultCliDetected;
}
