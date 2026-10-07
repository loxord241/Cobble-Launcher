// Стейт настроек (zustand). Спека §8: темы/язык применяются мгновенно.
import { create } from "zustand";
import { api } from "../api/client";
import { apiErrorText, hasStoredLanguage, setLanguage } from "../i18n";
import { setTheme, isThemeId } from "../theme/theme";
import type { Settings } from "../api/types";

interface SettingsState {
  settings: Settings | null;
  loaded: boolean;
  /** Ошибка начальной загрузки: App показывает экран с «Повторить» (D6). */
  error: string | null;
  load: () => Promise<void>;
  /** F20/D64: импорт файла настроек — в отличие от load() применяет и язык. */
  importFrom: (path: string) => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

function applySideEffects(s: Settings) {
  // Кастомизация (шрифт + акцент) — поверх палитры темы, поэтому применяется
  // до проверок темы/языка: невалидная тема не должна отменять кастом.
  applyUiFont(s.uiFont);
  applyUiAccent(s.uiAccent);
  // Тема: выбор со страницы «Темы» живёт в localStorage (5 тем); settings.theme
  // (dark|light в ядре) применяется, только если localStorage ещё не выбран.
  if (!isThemeId(s.theme)) return;
  if (s.theme === "dark" || s.theme === "light") {
    try {
      if (localStorage.getItem("theme") === null) setTheme(s.theme);
    } catch {
      setTheme(s.theme);
    }
  }
  // A11/D30: ключ localStorage "lang" перекрывает settings — ровно как тема
  // выше. Язык из ядра применяется, только если он ещё не выбран явно.
  if (!hasStoredLanguage()) setLanguage(s.language);
  applyUiScale(s.uiScale);
}

/** Масштаб интерфейса: html font-size в % — Tailwind на rem масштабируется целиком. */
export function applyUiScale(percent?: number) {
  const clamped = Math.min(150, Math.max(80, Math.round(percent ?? 100)));
  document.documentElement.style.fontSize = `${clamped}%`;
}

/**
 * Кастомизация §8. Стеки шрифтов uiFont; "" — не переопределять (шрифт темы,
 * tokens.css: --font-ui). system — системный стек «Сланца» явно.
 */
const FONT_STACKS: Record<string, string> = {
  system: '"Segoe UI", system-ui, sans-serif',
  serif: 'Georgia, "Times New Roman", serif',
  mono: '"JetBrains Mono", Consolas, monospace',
  round: '"Nunito", "Segoe UI Rounded", "Segoe UI", sans-serif',
};

/** Переменные акцента, которые объявляет тема (tokens.css) — их и перекрываем. */
const ACCENT_VARS = ["--accent", "--accent-hover", "--accent-soft", "--on-accent"];

/** Зеркало валидации ядра (settings.rs::validate): только #RRGGBB — значение идёт в CSS. */
export function isHexColor(value?: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value ?? "");
}

/** Шрифт интерфейса: инлайн-переменная на <html> перекрывает --font-ui темы. */
export function applyUiFont(font?: string) {
  const style = document.documentElement.style;
  const stack = FONT_STACKS[font ?? ""];
  // "" и неизвестное значение — снимаем переопределение, шрифт снова из темы.
  if (stack === undefined) style.removeProperty("--font-ui");
  else style.setProperty("--font-ui", stack);
}

/**
 * Акцент поверх палитры темы: --accent + производные. Инлайн-стиль на <html>
 * сильнее правил :root[data-theme], поэтому тема не ломается — палитра просто
 * перекрывается, а сброс ("" / мусор) возвращает её целиком.
 */
export function applyUiAccent(accent?: string) {
  const style = document.documentElement.style;
  if (!isHexColor(accent)) {
    for (const name of ACCENT_VARS) style.removeProperty(name);
    return;
  }
  const hex = accent as string;
  const [r, g, b] = channels(hex);
  style.setProperty("--accent", hex);
  // Наведение — тот же цвет на 10% темнее.
  style.setProperty("--accent-hover", toHex(r * 0.9, g * 0.9, b * 0.9));
  // Мягкая подложка — тот же цвет с альфой 0x33 (~20%).
  style.setProperty("--accent-soft", `${hex}33`);
  // Контраст текста на акценте: тёмный на светлом, белый на тёмном.
  style.setProperty("--on-accent", luminance(r, g, b) > 150 ? "#101418" : "#FFFFFF");
}

function channels(hex: string): [number, number, number] {
  const at = (i: number) => parseInt(hex.slice(i, i + 2), 16);
  return [at(1), at(3), at(5)];
}

function toHex(r: number, g: number, b: number): string {
  const part = (v: number) =>
    Math.min(255, Math.max(0, Math.round(v))).toString(16).padStart(2, "0");
  return `#${part(r)}${part(g)}${part(b)}`;
}

/** Яркость (YIQ): порог 150 — как в спеке кастомизации. */
function luminance(r: number, g: number, b: number): number {
  return 0.299 * r + 0.587 * g + 0.114 * b;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  loaded: false,
  error: null,
  load: async () => {
    try {
      const settings = await api.settingsGet();
      applySideEffects(settings);
      set({ settings, loaded: true, error: null });
    } catch (e) {
      // Честная ошибка вместо вечного спиннера (D6); B11 — локализованный текст.
      set({
        error: apiErrorText(e),
        loaded: false,
      });
    }
  },
  importFrom: async (path) => {
    await api.settingsImport(path);
    await get().load();
    // Импорт — явная команда «примени файл целиком», поэтому язык из файла
    // применяем даже при сохранённом выборе в localStorage: обычный load()
    // его намеренно не применяет (hasStoredLanguage), и селект языка показал
    // бы импортированное значение, а интерфейс остался на прежнем языке.
    // setLanguage сам игнорирует невалидный код — «если валиден» уже тут.
    const lang = get().settings?.language;
    if (lang) setLanguage(lang);
  },
  update: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...patch };
    await api.settingsSet(next);
    // Явный выбор языка в UI — применяем и фиксируем в localStorage (A11),
    // иначе сохранённый ранее "lang" перебил бы его на следующем старте.
    if (patch.language !== undefined) setLanguage(patch.language);
    applySideEffects(next);
    set({ settings: next });
  },
}));
