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
  /** silent — тихий старт-чек: ошибка (напр. offline_mode) глотается молча. */
  check: (opts?: { silent?: boolean }) => Promise<void>;
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
  check: async (opts) => {
    set({ busy: true, error: null });
    try {
      const info = await api.updateCheck();
      set({ info, checked: true });
    } catch (e) {
      // B11: текст ошибки локализуется по языку UI, а не сырой из ядра.
      // D64: ядро в офлайн-режиме отвечает кодом offline_mode; тихий старт-чек
      // ошибку глотает (старт офлайн — не повод для плашки обновлений), ручной
      // чек показывает честный локализованный текст.
      set({ info: null, checked: true, error: opts?.silent ? null : apiErrorText(e) });
    } finally {
      set({ busy: false });
    }
  },
  silentCheck: () => {
    if (get().checked || silentTimer) return;
    silentTimer = setTimeout(
      () => {
        silentTimer = null;
        // Пока таймер тикал, мог пройти ручной чек (кнопка/бейдж) — не дублируем.
        if (!get().checked) void get().check({ silent: true });
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
    } catch (e) {
      // D64: любая ошибка установки маскировалась под «установщик не готов» —
      // теперь берём локализованный текст кода ядра (офлайн скажет про офлайн,
      // битая подпись — про подпись). Пустой ответ или сырой код без словарной
      // статьи — прежний нейтральный фолбэк.
      const text = apiErrorText(e);
      const code = (e as { code?: unknown } | null)?.code;
      const unresolved =
        !text || (typeof code === "string" && code !== "" && text === code);
      set({ installError: unresolved ? t("updates.installerUnavailable") : text });
    } finally {
      set({ installing: false });
    }
  },
}));
