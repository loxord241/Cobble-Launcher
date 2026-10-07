// Стейт скинов (D34): по одному запросу на аккаунт за сессию, ошибка/нет
// скина → null (дефолтная отрисовка), чтобы модалка не мигала скелетонами.
import { create } from "zustand";
import { api } from "../api/client";
import type { Account, SkinInfo } from "../api/types";

interface SkinsState {
  /** accountId → скин; ключа нет — не спрашивали/запрос идёт, null — скина нет. */
  byId: Record<string, SkinInfo | null>;
  /** Повторно спросить ядро (кнопка «Обновить скин»). */
  refresh: (acc: Account) => Promise<void>;
  load: (acc: Account) => Promise<void>;
  /**
   * D59: сброс кэша скина профиля — следующий load()/refresh() пойдёт в ядро,
   * а не отдаст часовой кэш. Для вызова после успешного skinApply (ядро
   * инвалидирует свой кэш отдельно; фронт-защита от устаревшего превью).
   */
  invalidate: (accountId: string) => void;
}

// Дедупликация параллельных запросов одного аккаунта (чип + модалка).
const inflight = new Map<string, Promise<void>>();

async function fetchSkin(acc: Account): Promise<void> {
  const key = acc.id;
  const existing = inflight.get(key);
  if (existing) return existing;
  const p = (async () => {
    try {
      const skin = await api.accountSkin(acc.id);
      useSkins.setState((s) => ({ byId: { ...s.byId, [key]: skin } }));
    } catch {
      // Сетевая/протокольная ошибка — честно показываем дефолт, не падая.
      useSkins.setState((s) => ({ byId: { ...s.byId, [key]: null } }));
    } finally {
      inflight.delete(key);
    }
  })();
  inflight.set(key, p);
  return p;
}

export const useSkins = create<SkinsState>((_set, get) => ({
  byId: {},
  load: async (acc) => {
    if (acc.id in get().byId) return; // уже знаем (в т.ч. «нет скина»)
    await fetchSkin(acc);
  },
  refresh: async (acc) => {
    // D62: fetchSkin при живом inflight вернул бы СТАРЫЙ промис: запрос,
    // начатый до skinApply, завершился бы после него и затёр свежий скин
    // устаревшей текстурой. Поэтому дожидаемся текущий запрос и запускаем
    // новую загрузку (цепочку), а не переиспользуем старый результат.
    const prev = inflight.get(acc.id);
    if (prev) await prev;
    await fetchSkin(acc);
  },
  invalidate: (accountId) => {
    // Ключ удаляем (а не пишем null): «не спрашивали» вернёт стор к обычному
    // load(), а null UI рисовал бы как «скина нет» до конца загрузки.
    useSkins.setState((s) => {
      if (!(accountId in s.byId)) return s;
      const byId = { ...s.byId };
      delete byId[accountId];
      return { byId };
    });
  },
}));

/** Скин аккаунта, если уже загружен (без запроса) — для синхронных рендеров. */
export function skinOf(acc: Account): SkinInfo | null | undefined {
  return useSkins.getState().byId[acc.id];
}
