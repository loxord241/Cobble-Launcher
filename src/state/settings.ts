// Стейт настроек (zustand). Спека §8: темы/язык применяются мгновенно.
import { create } from "zustand";
import { api } from "../api/client";
import { setLanguage } from "../i18n";
import type { Settings } from "../api/types";

interface SettingsState {
  settings: Settings | null;
  loaded: boolean;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

function applySideEffects(s: Settings) {
  document.documentElement.classList.toggle("theme-light", s.theme === "light");
  setLanguage(s.language);
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  loaded: false,
  load: async () => {
    const settings = await api.settingsGet();
    applySideEffects(settings);
    set({ settings, loaded: true });
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
