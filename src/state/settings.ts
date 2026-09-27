// Стейт настроек (zustand). Спека §8: темы/язык применяются мгновенно.
import { create } from "zustand";
import { api } from "../api/client";
import { setLanguage } from "../i18n";
import { setTheme, isThemeId } from "../theme/theme";
import type { Settings } from "../api/types";

interface SettingsState {
  settings: Settings | null;
  loaded: boolean;
  /** Ошибка начальной загрузки: App показывает экран с «Повторить» (D6). */
  error: string | null;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

function applySideEffects(s: Settings) {
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
  setLanguage(s.language);
  applyUiScale(s.uiScale);
}

/** Масштаб интерфейса: html font-size в % — Tailwind на rem масштабируется целиком. */
export function applyUiScale(percent?: number) {
  const clamped = Math.min(150, Math.max(80, Math.round(percent ?? 100)));
  document.documentElement.style.fontSize = `${clamped}%`;
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
      // Честная ошибка вместо вечного спиннера (D6).
      set({
        error: (e as { message?: string })?.message ?? String(e),
        loaded: false,
      });
    }
  },
  update: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...patch };
    await api.settingsSet(next);
    applySideEffects(next);
    set({ settings: next });
  },
}));
