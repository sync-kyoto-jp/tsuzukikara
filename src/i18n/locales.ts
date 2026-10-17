// Registry of UI languages. To add a language:
//   1. Copy locales/en.ts to locales/<code>.ts and translate every value
//      (TypeScript reports missing or unknown keys).
//   2. Import it below and add one entry to LOCALES.
import type { Messages } from "./locales/en";
import en from "./locales/en";
import es from "./locales/es";
import fr from "./locales/fr";
import ja from "./locales/ja";
import ko from "./locales/ko";

export interface Locale {
  /** BCP 47 language code, also used for date formatting. */
  code: string;
  /** Name shown in the language picker, written in the language itself. */
  nativeName: string;
  messages: Messages;
}

export const LOCALES: Locale[] = [
  { code: "en", nativeName: "English", messages: en },
  { code: "ja", nativeName: "日本語", messages: ja },
  { code: "fr", nativeName: "Français", messages: fr },
  { code: "es", nativeName: "Español", messages: es },
  { code: "ko", nativeName: "한국어", messages: ko },
];

export const FALLBACK_LOCALE = LOCALES[0];

export function findLocale(code: string | null | undefined): Locale | undefined {
  if (!code) return undefined;
  const lower = code.toLowerCase();
  return LOCALES.find((l) => l.code === lower) ?? LOCALES.find((l) => lower.startsWith(`${l.code}-`));
}

/** First supported language from the OS/browser preferences. */
export function systemLocale(): Locale {
  for (const lang of navigator.languages ?? [navigator.language]) {
    const found = findLocale(lang);
    if (found) return found;
  }
  return FALLBACK_LOCALE;
}
