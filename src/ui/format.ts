// Человекочитаемые размеры (D56, LOC#7): единицы следуют языку интерфейса —
// ru/uk видят КБ/МБ/ГБ, en/pl — KB/MB/GB. Раньше суффиксы были вшиты
// по-русски в восьми местах (аудит локализации 2026-10-03).
import { currentLanguage, t } from "../i18n";

function suffixes(): { kb: string; mb: string; gb: string } {
  const lang = currentLanguage();
  return lang === "ru" || lang === "uk"
    ? { kb: "КБ", mb: "МБ", gb: "ГБ" }
    : { kb: "KB", mb: "MB", gb: "GB" };
}

/** Авто-масштаб: <1 МБ → КБ, до 1 ГБ → МБ, дальше ГБ. */
export function formatSize(bytes: number): string {
  // D64: после очистки кэша сюда приезжал NaN (ядро отдало нечисловой объём) —
  // честное «—» вместо «NaN КБ».
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  const s = suffixes();
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} ${s.gb}`;
  if (mb >= 1) return `${Math.round(mb)} ${s.mb}`;
  return `${Math.max(1, Math.round(bytes / 1024))} ${s.kb}`;
}

/** Заданный объём RAM в МБ (слайдер памяти): «4096 МБ (4.0 ГБ)». */
export function formatRamMb(mb: number): string {
  const s = suffixes();
  return `${mb} ${s.mb} (${(mb / 1024).toFixed(1)} ${s.gb})`;
}

/**
 * D64 (контракт для карточек/деталей): slug → читаемый лейбл. Словарный ключ
 * `slug.<slug>` побеждает, если он есть (t() возвращает сам ключ, когда его
 * нет, — это и есть проверка); иначе человекочитаемый фолбэк по образцу
 * HomePage: «adventure-rpg» → «Adventure rpg».
 */
export function prettySlug(slug: string): string {
  if (!slug) return slug;
  const key = `slug.${slug}`;
  const localized = t(key);
  if (localized !== key) return localized;
  return slug.charAt(0).toUpperCase() + slug.slice(1).replace(/-/g, " ");
}
