// Состояние обновлений лаунчера (D46). Проверка — лёгкий `update_check`
// (GitHub releases/latest без авторизации); установка — updater-плагин
// (подписанные апдейты), включается только по кнопке в Настройках.
// Тихая проверка при старте — один раз за сессию с задержкой: ошибки не
// выпрыгивают никуда, честный результат виден в Настройках → «Обновления».
import { create } from "zustand";
import { api } from "../api/client";
import { apiErrorText } from "../i18n";
import type { UpdateInfo } from "../api/types";

interface UpdateState {
  info: UpdateInfo | null;
  /** Проверка в этой сессии уже была (тихий старт-чек не повторяется). */
  checked: boolean;
  busy: boolean;
  /** Поломка IPC — отдельный честный канал; «не удалось проверить» живёт в info.note. */
  error: string | null;
  check: () => Promise<void>;
  silentCheck: () => void;
}

let silentTimer: ReturnType<typeof setTimeout> | null = null;

export const useUpdateStore = create<UpdateState>((set, get) => ({
  info: null,
  checked: false,
  busy: false,
  error: null,
  check: async () => {
    set({ busy: true, error: null });
    try {
      const info = await api.updateCheck();
      set({ info, checked: true });
    } catch (e) {
      // B11: текст ошибки локализуется по языку UI, а не сырой из ядра.
      set({ info: null, checked: true, error: apiErrorText(e) });
    } finally {
      set({ busy: false });
    }
  },
  silentCheck: () => {
    if (get().checked || silentTimer) return;
    silentTimer = setTimeout(
      () => {
        silentTimer = null;
        void get().check();
      },
      8000,
    );
  },
}));
