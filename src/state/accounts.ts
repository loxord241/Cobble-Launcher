// Стейт аккаунтов (M8): чип профиля в титлбаре + смена активного.
import { create } from "zustand";
import { api } from "../api/client";
import type { Account, AccountKind } from "../api/types";
interface AccountsState {
  list: Account[];
  loaded: boolean;
  load: () => Promise<void>;
  setActive: (id: string) => Promise<void>;
}

export const useAccounts = create<AccountsState>((set, get) => ({
  list: [],
  loaded: false,
  load: async () => {
    const list = await api.accountList().catch(() => []);
    set({ list, loaded: true });
  },
  setActive: async (id) => {
    await api.accountActiveSet(id);
    await get().load();
  },
}));

/** Человекочитаемый тип профиля: offline = «нелицензионный» (спека §6.8). */
export const accountKindLabelKey: Record<AccountKind, string> = {
  offline: "account.kind.offline",
  msa: "account.kind.msa",
  ely: "account.kind.ely",
};
