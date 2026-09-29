type Translate = (key: string, params?: Record<string, string | number>) => string;

const MAX_RAW_LENGTH = 200;

/**
 * Turn a translate_text / translate_reply error string into a user-facing
 * message. The patterns mirror the literal errors in ai_translate.rs and
 * cli_translate.rs; anything unrecognised is shown raw (truncated) so the
 * user still sees what the CLI or API actually said.
 */
export function describeTranslateError(raw: string, t: Translate): string {
  if (raw.includes("Codex CLI is too old")) return t("chat.translateError.codexOutdated");
  if (raw.includes("Codex CLI is not logged in")) return t("chat.translateError.codexNotLoggedIn");
  const codexModel = raw.match(/^Codex CLI cannot use the model (.+)\.$/);
  if (codexModel) return t("chat.translateError.codexModel", { model: codexModel[1] });
  if (raw.includes("too old for translation")) return t("chat.translateError.cliOutdated");
  if (raw.includes("No gemini, claude or codex CLI found")) return t("chat.translateError.noCli");
  const notFound = raw.match(/^(\w+) CLI not found/);
  if (notFound) return t("chat.translateError.cliNotFound", { cli: notFound[1] });
  if (raw.includes("timed out")) return t("chat.translateError.timeout");
  if (raw.includes("API key not set") || raw.includes("No AI keys configured")) {
    return t("chat.translateError.noApiKey");
  }
  if (raw.includes("No AI model selected")) return t("chat.translateError.noModel");
  if (/HTTP 40[13]\b/.test(raw)) return t("chat.translateError.authFailed");
  if (/HTTP 429\b/.test(raw)) return t("chat.translateError.rateLimited");
  if (raw.includes("Text too long")) return t("chat.translateError.tooLong");
  const detail = raw.length > MAX_RAW_LENGTH ? `${raw.slice(0, MAX_RAW_LENGTH)}…` : raw;
  return t("chat.translateError.generic", { message: detail });
}
