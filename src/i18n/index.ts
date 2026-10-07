// Мини-i18n без зависимостей: t("key") + {placeholders}. Спека §8.
// Языки: en (по умолчанию), ru, uk, pl. Fallback-цепочка: current → en → ключ.
import ru from "./ru.json";
import en from "./en.json";
import uk from "./uk.json";
import pl from "./pl.json";
import errors from "./errors.json";
import type { ErrorPayload } from "../api/types";

type Dict = Record<string, string>;
type ErrorEntry = Record<"ru" | "en" | "uk" | "pl", string>;

const dicts: Record<string, Dict> = {
  ru: ru as Dict,
  en: en as Dict,
  uk: uk as Dict,
  pl: pl as Dict,
};
const errorDict = errors as unknown as Record<string, Record<string, string>>;
// LOC#2: секция подсказок (hint-коды ядра → тексты по языку).
const hintDict = (errors as unknown as { hints?: Record<string, Record<string, string>> }).hints;

/**
 * A11/D30: язык можно задать ключом localStorage "lang" — он перекрывает
 * settings.language при старте (та же механика, что у тем: theme.ts/STORAGE_KEY).
 * Нужно для локале-независимых приёмочных скриптов.
 */
const LANG_STORAGE_KEY = "lang";

function isLang(v: string): boolean {
  return Object.prototype.hasOwnProperty.call(dicts, v);
}

function storedLanguage(): string | null {
  try {
    const v = localStorage.getItem(LANG_STORAGE_KEY);
    return v && isLang(v) ? v : null;
  } catch {
    return null; // приватный режим — выбор языка не переживёт перезапуск
  }
}

/** Язык уже выбран явно (localStorage)? Тогда settings его не перебивает. */
export function hasStoredLanguage(): boolean {
  return storedLanguage() !== null;
}

let current = storedLanguage() ?? "en";

// D64: <html lang> в index.html зашит «ru» и сам не обновляется — синхронно
// приводим его к фактическому языку до первой отрисовки, иначе скринридеры
// озвучивают интерфейс по-русски при любом выборе языка.
if (typeof document !== "undefined") document.documentElement.lang = current;

export function setLanguage(lang: string) {
  if (!isLang(lang)) return;
  current = lang;
  if (typeof document !== "undefined") document.documentElement.lang = lang;
  try {
    localStorage.setItem(LANG_STORAGE_KEY, lang);
  } catch {
    /* приватный режим — язык просто не переживёт перезапуск */
  }
}

/** Текущий код языка (ru/en/uk) — для toLocaleString и форматов дат (A38). */
export function currentLanguage(): string {
  return current;
}

/** A58: подстановка {key} — во ВСЕ вхождения, без спецсимволов $ в replace. */
function interpolate(text: string, params?: Record<string, string | number>): string {
  if (!params) return text;
  let out = text;
  for (const [k, v] of Object.entries(params)) {
    out = out.split(`{${k}}`).join(String(v));
  }
  return out;
}

function lookup(key: string): string | undefined {
  return dicts[current][key] ?? dicts.en[key];
}

export function t(key: string, params?: Record<string, string | number>): string {
  const text = lookup(key);
  return interpolate(text ?? key, params);
}

/**
 * A40: плюрализация числительных. Ключ — база, форма выбирается правилами
 * языка. ru/uk — славянские правила (1/21 → one, 2–4 кроме 12–14 → few,
 * иначе many); pl — свои: one ТОЛЬКО для точной единицы (n=1; «21 pobranie»
 * — many, а не one), 2–4 кроме 12–14 → few, иначе many; en — one/other.
 * Набор форм `one/few/many/other` есть во всех словарях (паритет ключей);
 * недостижимые для языка формы служат только этому паритету.
 * {n} подставляется всегда.
 */
function pluralCandidates(lang: string, n: number): string[] {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (lang === "ru" || lang === "uk") {
    const primary =
      mod10 === 1 && mod100 !== 11
        ? "one"
        : mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)
          ? "few"
          : "many";
    return [primary, "many", "few", "one", "other"];
  }
  if (lang === "pl") {
    const primary =
      n === 1
        ? "one"
        : mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)
          ? "few"
          : "many";
    return [primary, "many", "few", "one", "other"];
  }
  return [n === 1 ? "one" : "other", "other", "many", "one"];
}

export function plural(key: string, n: number, params?: Record<string, string | number>): string {
  const withN = { n, ...params };
  for (const form of pluralCandidates(current, n)) {
    const text = lookup(`${key}.${form}`);
    if (text !== undefined) return interpolate(text, withN);
  }
  return t(key, withN);
}

/** Ошибка ядра → локализованный текст по текущему языку; message — fallback. */
export function errorText(e: ErrorPayload): string {
  const entry = errorDict[e.code];
  const localized = entry ? (entry[current as keyof ErrorEntry] ?? entry.en ?? entry.ru) : undefined;
  // «Сеть» без деталей бесполезна: реальная причина (статус/тело ответа
  // Microsoft) приходит в message — показываем её (жалоба владельца 2026-09-27).
  // Когда причина есть, вместо длинной обёртки с советом берём короткую
  // network_detail («Проблема с сетью: <причина>»), иначе текст задваивался.
  const short = errorDict.network_detail?.[current as keyof ErrorEntry];
  // Причина из ядра: пустая или равная коду детализации не несёт.
  // D64: сырой message ядра (кириллица) подклеиваем ТОЛЬКО в ru-интерфейсе —
  // en/uk/pl получают локализованный текст по коду без русской каши в хвосте.
  const ruMessage = current === "ru" && e.message && e.message !== e.code ? e.message : undefined;
  const base =
    e.code === "network" && ruMessage
      ? `${short ?? localized ?? e.code}: ${ruMessage}`
      : // invalid_input без деталей бесполезен так же, как network: что именно
        // не прошло валидацию — в message. Симметрично network, но короче:
        // «локализованный текст (детали)».
        e.code === "invalid_input" && ruMessage
        ? `${localized ?? e.code} (${ruMessage})`
        : (localized ?? e.message);
  // LOC#2: ядро отдаёт КОД подсказки (hintCode) — текст по словарю, иначе
  // en/uk/pl приклеивался бы русский совет из ядра.
  const hint = e.hintCode
    ? hintDict?.[e.hintCode]?.[current]
    : undefined;
  return hint ? `${base}. ${hint}` : base;
}

/**
 * Ошибка из промиса/IPC в текст для UI (D13). Payload ядра `{code, message, hint?}`
 * проходит через errorText; голые Error/строки показываем как есть; вместо
 * `[object Object]` — честная «внутренняя ошибка» из errors.json.
 */
export function apiErrorText(e: unknown): string {
  if (typeof e === "string" && e) return e;
  const payload = e as { code?: unknown; message?: unknown; hintCode?: unknown } | null | undefined;
  const message =
    typeof payload?.message === "string" && payload.message ? payload.message : undefined;
  if (typeof payload?.code === "string" && payload.code) {
    return errorText({
      code: payload.code,
      message: message ?? payload.code,
      hintCode: typeof payload?.hintCode === "string" && payload.hintCode ? payload.hintCode : undefined,
    });
  }
  if (message) return message;
  if (e instanceof Error && e.message) return e.message;
  return errorText({ code: "internal", message: String(e) });
}
