// Мини-i18n без зависимостей: t("key") + {placeholders}. Спека §8.
import ru from "./ru.json";
import en from "./en.json";
import errors from "./errors.json";
import type { ErrorPayload } from "../api/types";

type Dict = Record<string, string>;

const dicts: Record<string, Dict> = { ru: ru as Dict, en: en as Dict };
let current = "ru";

export function setLanguage(lang: string) {
  if (dicts[lang]) current = lang;
}

export function t(key: string, params?: Record<string, string | number>): string {
  let text = dicts[current][key] ?? dicts.ru[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.replace(`{${k}}`, String(v));
    }
  }
  return text;
}

/** Ошибка ядра → локализованный текст: код в errors.json, message — fallback. */
export function errorText(e: ErrorPayload): string {
  const localized = (errors as Dict)[e.code];
  const base = localized ? `${localized}` : e.message;
  return e.hint ? `${base}. ${e.hint}` : base;
}
