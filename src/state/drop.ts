// D69: живёт ли сейчас UI drag&drop (модалка выбора инстанса / прогресс
// установки) — Shell прячет Ctrl+P-палитру поверх: две ловушки Tab
// сражались бы за фокус (ping-pong, как в D64 у попапа обновления).
import { create } from "zustand";

type DropUI = { busy: boolean; set: (v: boolean) => void };

export const useDropUI = create<DropUI>((set) => ({
  busy: false,
  set: (v) => set({ busy: v }),
}));
