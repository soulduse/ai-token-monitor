import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

/** `id` changes on every failure, so a repeated identical error still restarts the auto-hide timer. */
export interface TranslateError {
  id: number;
  message: string;
}

let nextErrorId = 0;

export function useTranslate(targetLanguage: string) {
  const [translations, setTranslations] = useState<Record<string, string>>({});
  const [translating, setTranslating] = useState<Set<string>>(new Set());
  const [error, setError] = useState<TranslateError | null>(null);

  // Cached translations are in the old language after a language switch.
  useEffect(() => {
    setTranslations({});
  }, [targetLanguage]);

  const clearError = useCallback(() => setError(null), []);

  const reportError = useCallback((err: unknown) => {
    const message = typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
    setError({ id: ++nextErrorId, message });
  }, []);

  const translateText = useCallback(async (text: string, targetLang: string, sourceLang?: string): Promise<string | null> => {
    setError(null);
    try {
      return await invoke<string>("translate_text", {
        text,
        targetLanguage: targetLang,
        sourceLanguage: sourceLang ?? null,
      });
    } catch (err) {
      console.error("Translation failed:", err);
      reportError(err);
      return null;
    }
  }, [reportError]);

  const translateReply = useCallback(async (text: string, originalMessage: string): Promise<string | null> => {
    setError(null);
    try {
      return await invoke<string>("translate_reply", {
        text,
        originalMessage,
      });
    } catch (err) {
      console.error("Reply translation failed:", err);
      reportError(err);
      return null;
    }
  }, [reportError]);

  const translate = useCallback(async (messageId: string, text: string): Promise<string | null> => {
    // Each call spends API credit or CLI subscription quota — reuse the result.
    const cached = translations[messageId];
    if (cached) return cached;
    setTranslating((prev) => new Set(prev).add(messageId));
    try {
      const result = await translateText(text, targetLanguage);
      if (result) {
        setTranslations((prev) => ({ ...prev, [messageId]: result }));
      }
      return result;
    } finally {
      setTranslating((prev) => {
        const next = new Set(prev);
        next.delete(messageId);
        return next;
      });
    }
  }, [targetLanguage, translateText, translations]);

  return { translations, translating, translate, translateText, translateReply, error, clearError };
}
