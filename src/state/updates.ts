// Состояние обновлений лаунчера (D46). Проверка — лёгкий `update_check`
// (GitHub releases/latest без авторизации); установка — updater-плагин
// (подписанные апдейты). D54: установка переехала из раздела настроек в
// стор — ею же пользуется попап бейджа в сайдбаре (жалоба владельца: клик
// по бейджу вёл в настройки, а не предлагал поставить обновление).
import { create } from "zustand";
import { api } from "../api/client";
import { apiErrorText, t } from "../i18n";
import type { UpdateInfo } from "../api/types";

interface UpdateState {
  info: UpdateInfo | null;
  /** Проверка в этой сессии уже была (тихий старт-чек не повторяется). */
  checked: boolean;
  busy: boolean;
  /** Поломка IPC — отдельный честный канал; «не удалось проверить» живёт в info.note. */
  error: string | null;
  /** Установка (updater-плагин): подписанный апдейт с прогрессом. */
  installing: boolean;
  /** null — прогресс не определён (ContentLength не пришёл), иначе 0–100. */
  progress: number | null;
  installError: string | null;
  check: () => Promise<void>;
  silentCheck: () => void;
  install: () => Promise<void>;
}

let silentTimer: ReturnType<typeof setTimeout> | null = null;

export const useUpdateStore = create<UpdateState>((set, get) => ({
  info: null,
  checked: false,
  busy: false,
  error: null,
  installing: false,
  progress: null,
  installError: null,
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
  // Установка: отдельная проверка updater-плагином даёт объект Update с
  // подписью; качаем с прогрессом и перезапускаемся. Релиз без latest.json
  // даёт ошибку — показываем честный ручной путь (ссылка на релизы).
  install: async () => {
    if (get().installing) return;
    set({ installing: true, installError: null, progress: null });
    try {
      const { check: pluginCheck } = await import("@tauri-apps/plugin-updater");
      const update = await pluginCheck();
      if (!update) {
        set({ installError: t("updates.installerUnavailable") });
        return;
      }
      let total = 0;
      let received = 0;
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") {
          total = ev.data.contentLength ?? 0;
        } else if (ev.event === "Progress") {
          received += ev.data.chunkLength ?? 0;
          if (total > 0) {
            set({ progress: Math.min(100, Math.round((received * 100) / total)) });
          }
        } else {
          set({ progress: 100 });
        }
      });
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch {
      set({ installError: t("updates.installerUnavailable") });
    } finally {
      set({ installing: false });
    }
  },
}));
