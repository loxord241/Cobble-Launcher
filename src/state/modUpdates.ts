// F5: тихая проверка обновлений модов по всем инстансам. Запускается из Shell
// один раз после успешной загрузки списка; результат — счётчик доступных
// обновлений на инстанс для бейджа на карточке (InstanceCard). Всё молча:
// ошибки сети (в т.ч. офлайн-режим) не показываются и мешать лаунчеру не могут.
import { create } from "zustand";
import { api } from "../api/client";
import type { Instance } from "../api/types";

interface ModUpdatesState {
  /** Инстанс → количество доступных обновлений модов (0 тоже записываем). */
  byInstance: Record<string, number>;
  /** Момент завершения последней проверки (Date.now()); 0 — ещё не было. */
  checkedAt: number;
  /** Проверка идёт прямо сейчас — повторные вызовы (ремоунт Shell) игнорируем. */
  running: boolean;
  /** Тихая проверка по списку инстансов (стрэггер и троттлинг внутри). */
  checkAll: (instances: Instance[]) => Promise<void>;
  /** Точечная перепроверка одного инстанса (D69-ревизия): без троттлинга —
   * зовётся после ручного «Обновить всё» на странице инстанса, чтобы бейдж
   * на карточке не врал до конца сессии. Тихая, ошибки глотаем. */
  checkOne: (instanceId: string) => Promise<void>;
}

/** Троттлинг: повторная проверка не раньше, чем через 30 минут после прошлой. */
const THROTTLE_MS = 30 * 60 * 1000;
/** Стрэггер: между запусками запросов — 400 мс, чтобы не бить ядро пачкой. */
const STAGGER_MS = 400;
/** Максимум параллельных запросов content_update_check. */
const MAX_PARALLEL = 2;

export const useModUpdatesStore = create<ModUpdatesState>((set, get) => ({
  byInstance: {},
  checkedAt: 0,
  running: false,
  checkAll: async (instances) => {
    const { running, checkedAt } = get();
    // Проверка уже идёт (двойной вызов, ремоунт Shell) — выходим.
    if (running) return;
    // Свежий результат (меньше 30 минут) — повторная проверка не нужна.
    if (checkedAt > 0 && Date.now() - checkedAt < THROTTLE_MS) return;
    set({ running: true });
    try {
      // Во фронтовом Instance нет признака «есть моды» (счётчика контента),
      // поэтому проверяем все инстансы — частоту гасит троттлинг THROTTLE_MS.
      const ids = instances.map((inst) => inst.id);
      let next = 0;
      let inFlight = 0;
      // Цепочка промисов с шагом STAGGER_MS: каждый тик занимает свободные
      // слоты, одновременно в полёте не больше MAX_PARALLEL запросов. Без
      // внешних библиотек — только setTimeout и цепочки then/catch.
      await new Promise<void>((resolve) => {
        let timer: ReturnType<typeof setTimeout> | null = null;
        const finish = () => {
          if (timer !== null) clearTimeout(timer);
          resolve();
        };
        const tick = () => {
          while (inFlight < MAX_PARALLEL && next < ids.length) {
            const id = ids[next++];
            inFlight += 1;
            api
              .contentUpdateCheck(id)
              .then((updates) => {
                // 0 тоже записываем: устаревший бейдж должен сняться.
                set((s) => ({ byInstance: { ...s.byInstance, [id]: updates.length } }));
              })
              .catch(() => {
                // Тихий фейл: сеть недоступна / офлайн-режим — инстанс просто
                // пропускаем, прежний счётчик (если был) не трогаем.
              })
              .finally(() => {
                inFlight -= 1;
                // Ответы по всем инстансам получены — проверка завершена.
                if (next >= ids.length && inFlight === 0) finish();
              });
          }
          if (next < ids.length) {
            timer = setTimeout(tick, STAGGER_MS);
          } else if (inFlight === 0) {
            finish();
          }
        };
        if (ids.length === 0) finish();
        else tick();
      });
    } finally {
      // checkedAt ставим и после сплошных ошибок: в офлайне бессмысленно
      // долбить ядро на каждом ремоунте — следующая попытка через 30 минут.
      set({ running: false, checkedAt: Date.now() });
    }
  },
  checkOne: async (instanceId) => {
    try {
      const updates = await api.contentUpdateCheck(instanceId);
      set((s) => ({ byInstance: { ...s.byInstance, [instanceId]: updates.length } }));
    } catch {
      // Тихий фейл: бейдж останется прежним, сессию не ломаем.
    }
  },
}));
