// Мини-i18n без зависимостей: t("key") + {placeholders}. Спека §8.
// Языки: en (по умолчанию), ru, uk. Fallback-цепочка: current → en → ключ.
import ru from "./ru.json";
import en from "./en.json";
import uk from "./uk.json";
import errors from "./errors.json";
import type { ErrorPayload } from "../api/types";

type Dict = Record<string, string>;
type ErrorEntry = Record<"ru" | "en" | "uk", string>;

const dicts: Record<string, Dict> = { ru: ru as Dict, en: en as Dict, uk: uk as Dict };
const errorDict = errors as unknown as Record<string, ErrorEntry>;
let current = "en";

export function setLanguage(lang: string) {
  if (dicts[lang]) current = lang;
}

export function t(key: string, params?: Record<string, string | number>): string {
  let text = dicts[current][key] ?? dicts.en[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.replace(`{${k}}`, String(v));
    }
  }
  return text;
}

/** Ошибка ядра → локализованный текст по текущему языку; message — fallback. */
export function errorText(e: ErrorPayload): string {
  const entry = errorDict[e.code];
  const localized = entry ? (entry[current as keyof ErrorEntry] ?? entry.en ?? entry.ru) : undefined;
  const base = localized ?? e.message;
  return e.hint ? `${base}. ${e.hint}` : base;
}

/**
 * Ошибка из промиса/IPC в текст для UI (D13). Payload ядра `{code, message, hint?}`
 * проходит через errorText; голые Error/строки показываем как есть; вместо
 * `[object Object]` — честная «внутренняя ошибка» из errors.json.
 */
export function apiErrorText(e: unknown): string {
  if (typeof e === "string" && e) return e;
  const payload = e as { code?: unknown; message?: unknown; hint?: unknown } | null | undefined;
  const message =
    typeof payload?.message === "string" && payload.message ? payload.message : undefined;
  if (typeof payload?.code === "string" && payload.code) {
    return errorText({
      code: payload.code,
      message: message ?? payload.code,
      hint: typeof payload?.hint === "string" && payload.hint ? payload.hint : undefined,
    });
  }
  if (message) return message;
  if (e instanceof Error && e.message) return e.message;
  return errorText({ code: "internal", message: String(e) });
}
