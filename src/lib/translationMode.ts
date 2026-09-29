import type { UserPreferences } from "./types";

type TranslationPrefs = Pick<UserPreferences, "ai_keys" | "ai_model" | "translation_provider">;

/** A translation model key + selected model — what the API path needs. */
export function hasApiTranslation(prefs: TranslationPrefs): boolean {
  const keys = prefs.ai_keys;
  const hasKey = !!(keys?.gemini || keys?.openai || keys?.anthropic || keys?.kiro);
  return hasKey && !!prefs.ai_model;
}

/**
 * Mirrors `UserPreferences::translates_with_cli` in the backend: an explicit
 * choice always wins; with none saved, a configured API key + model keeps the
 * API path, and otherwise a detected CLI is used automatically.
 */
export function translatesWithCli(prefs: TranslationPrefs, cliDetected: boolean): boolean {
  if (prefs.translation_provider) return prefs.translation_provider === "cli";
  return !hasApiTranslation(prefs) && cliDetected;
}
