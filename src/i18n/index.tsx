import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { api, isAppError } from "../api";
import { FALLBACK_LOCALE, findLocale, Locale, systemLocale } from "./locales";
import type { MessageKey } from "./locales/en";

export type { MessageKey };
type Params = Record<string, string | number>;

interface I18n {
  locale: Locale;
  /** Saved preference; `null` follows the system language. */
  preference: string | null;
  setPreference: (code: string | null) => Promise<void>;
  t: (key: MessageKey, params?: Params) => string;
  /** Translates a backend error (`{ code, detail }`) or any thrown value. */
  te: (error: unknown) => string;
  formatDate: (iso: string | null) => string;
}

const I18nContext = createContext<I18n | null>(null);

function interpolate(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (m, name: string) => (name in params ? String(params[name]) : m));
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [preference, setPref] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    api
      .getSettings()
      .then((s) => setPref(s.language ?? null))
      .catch(() => setPref(null))
      .finally(() => setReady(true));
  }, []);

  const locale = useMemo(() => findLocale(preference) ?? systemLocale(), [preference]);

  useEffect(() => {
    document.documentElement.lang = locale.code;
  }, [locale]);

  const setPreference = useCallback(async (code: string | null) => {
    await api.setLanguage(code);
    setPref(code);
  }, []);

  const t = useCallback(
    (key: MessageKey, params?: Params) =>
      interpolate(locale.messages[key] ?? FALLBACK_LOCALE.messages[key] ?? key, params),
    [locale],
  );

  const te = useCallback(
    (error: unknown) => {
      if (isAppError(error)) {
        const key = `error.${error.code}` as MessageKey;
        const template = locale.messages[key] ?? FALLBACK_LOCALE.messages[key];
        if (template) return interpolate(template, { detail: error.detail ?? "" });
        return error.detail ? `${error.code}: ${error.detail}` : error.code;
      }
      if (typeof error === "string") return error;
      if (error instanceof Error) return error.message;
      return JSON.stringify(error);
    },
    [locale],
  );

  const formatDate = useCallback(
    (iso: string | null) => {
      if (!iso) return "—";
      const d = new Date(iso);
      if (Number.isNaN(d.getTime())) return iso;
      return d.toLocaleString(locale.code, {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      });
    },
    [locale],
  );

  const value = useMemo(
    () => ({ locale, preference, setPreference, t, te, formatDate }),
    [locale, preference, setPreference, t, te, formatDate],
  );

  // Avoid a flash of the wrong language before the saved preference is loaded.
  if (!ready) return null;
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18n {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n must be used inside I18nProvider");
  return ctx;
}

