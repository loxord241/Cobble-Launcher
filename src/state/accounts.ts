// Стейт аккаунтов (M8): чип профиля в титлбаре + управление профилями.
import { create } from "zustand";
import { currentLanguage } from "../i18n";
import { api } from "../api/client";
import type { Account, AccountKind } from "../api/types";

import { useSettings } from "./settings";

interface AccountsState {
  list: Account[];
  loaded: boolean;
  /**
   * F9: профили, у которых упал фоновый refresh токена (событие ядра
   * account_refresh_failed). Снимается перелогином/удалением профиля; UI-показ
   * (плашка «перелогиньтесь») — чужая волна, состояние готово.
   */
  degradedIds: Set<string>;
  load: () => Promise<void>;
  setActive: (id: string) => Promise<void>;
  addOffline: (name: string) => Promise<Account>;
  addMsaBrowser: () => Promise<Account>;
  addEly: (username: string, password: string) => Promise<Account>;
  remove: (id: string) => Promise<void>;
  /** F9: пометить профиль деградировавшим (обработчик account_refresh_failed). */
  setDegraded: (id: string) => void;
  /** F9: снять метку — успешный перелогин того же профиля. */
  clearDegraded: (id: string) => void;
}

export const useAccounts = create<AccountsState>((set, get) => ({
  list: [],
  loaded: false,
  degradedIds: new Set<string>(),
  load: async () => {
    const list = await api.accountList().catch(() => []);
    set((s) => {
      // F9: метки удалённых профилей чистим, живых — держим: load() дёргается
      // многими действиями, и стирать свежую метку нельзя (refresh/логин/
      // удаление аккаунта все проходят через load — этого достаточно).
      const ids = new Set(list.map((a) => a.id));
      const degradedIds = new Set([...s.degradedIds].filter((id) => ids.has(id)));
      return { list, loaded: true, degradedIds };
    });
  },
  setActive: async (id) => {
    await api.accountActiveSet(id);
    await get().load();
    const cur = useSettings.getState().settings;
    if (cur) {
      useSettings.setState({ settings: { ...cur, accountsActiveId: id } });
    }
  },
  addOffline: async (name: string) => {
    const acc = await api.accountAddOffline(name);
    await get().load();
    return acc;
  },
  addMsaBrowser: async () => {
    const acc = await api.accountAddMsaBrowser();
    await get().load();
    return acc;
  },
  addEly: async (username: string, password: string) => {
    const acc = await api.accountAddEly(username, password);
    await get().load();
    return acc;
  },
  remove: async (id: string) => {
    await api.accountRemove(id);
    await get().load();
    // Ядро переназначает активный аккаунт при удалении активного — подтягиваем
    // свежий accountsActiveId, иначе чип титлбара показывает удалённого.
    await useSettings.getState().load();
  },
  setDegraded: (id) => {
    set((s) => {
      if (s.degradedIds.has(id)) return s;
      const degradedIds = new Set(s.degradedIds);
      degradedIds.add(id);
      return { degradedIds };
    });
  },
  clearDegraded: (id) => {
    set((s) => {
      if (!s.degradedIds.has(id)) return s;
      const degradedIds = new Set(s.degradedIds);
      degradedIds.delete(id);
      return { degradedIds };
    });
  },
}));

/** Человекочитаемый тип профиля: offline = «нелицензионный» (спека §6.8). */
export const accountKindLabelKey: Record<AccountKind, string> = {
  offline: "account.kind.offline",
  msa: "account.kind.msa",
  ely: "account.kind.ely",
  authlib: "account.kind.authlib",
};

/** Сортировка профилей (D53): активный всегда первый, дальше по имени или по
 * порядку добавления. Выбор живёт в localStorage и общий для модалки аккаунтов
 * и меню чипа в титлбаре. */
export type AccountSort = "name" | "added";
const SORT_KEY = "accountsSort";

export function accountSortPref(): AccountSort {
  try {
    return localStorage.getItem(SORT_KEY) === "added" ? "added" : "name";
  } catch {
    return "name";
  }
}

export function setAccountSortPref(mode: AccountSort): void {
  try {
    localStorage.setItem(SORT_KEY, mode);
  } catch {
    /* приватный режим — сортировка просто не запоминается */
  }
}

export function sortAccounts(
  list: Account[],
  mode: AccountSort,
  activeId?: string,
): Account[] {
  const rest =
    mode === "name"
      // LOC-гипотеза из аудита: локаль интерфейса, а не ОС (польские ą/ć/ł).
      ? [...list].sort((a, b) => a.name.localeCompare(b.name, currentLanguage(), { sensitivity: "base" }))
      : [...list];
  const active = rest.find((a) => a.id === activeId);
  return active ? [active, ...rest.filter((a) => a.id !== activeId)] : rest;
}
