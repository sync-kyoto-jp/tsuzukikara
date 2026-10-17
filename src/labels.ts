import type { MessageKey } from "./i18n";

type T = (key: MessageKey, params?: Record<string, string | number>) => string;

/** Short environment label for a source key such as `windows` or `wsl:Ubuntu`. */
export function sourceLabel(t: T, key: string): string {
  if (key === "windows") return t("source.windows");
  if (key.startsWith("wsl:")) return t("source.wsl", { distro: key.slice(4) });
  if (key.startsWith("custom:")) return t("source.custom");
  return key;
}

export function titleOrUntitled(t: T, title: string): string {
  return title.trim() ? title : t("session.untitled");
}
