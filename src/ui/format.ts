// Человекочитаемые размеры (D56, LOC#7): единицы следуют языку интерфейса —
// ru/uk видят КБ/МБ/ГБ, en/pl — KB/MB/GB. Раньше суффиксы были вшиты
// по-русски в восьми местах (аудит локализации 2026-10-03).
import { currentLanguage } from "../i18n";

function suffixes(): { kb: string; mb: string; gb: string } {
  const lang = currentLanguage();
  return lang === "ru" || lang === "uk"
    ? { kb: "КБ", mb: "МБ", gb: "ГБ" }
    : { kb: "KB", mb: "MB", gb: "GB" };
}

/** Авто-масштаб: <1 МБ → КБ, до 1 ГБ → МБ, дальше ГБ. */
export function formatSize(bytes: number): string {
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
