// Стейт инстансов + статусы запуска (события ядра → UI).
import { create } from "zustand";
import { api, onCoreEvent } from "../api/client";
import type { Instance, LaunchPhase } from "../api/types";

export interface LaunchStatus {
  phase: LaunchPhase;
  doneFiles?: number;
  totalFiles?: number;
  exitCode?: number;
}

interface InstancesState {
  list: Instance[];
  loaded: boolean;
  status: Record<string, LaunchStatus>;
  error: string | null;
  load: () => Promise<void>;
  subscribeEvents: () => Promise<void>;
  launch: (id: string, player: string) => Promise<void>;
  create: (mcVersion: string, name: string) => Promise<Instance>;
  remove: (id: string, wipe: boolean) => Promise<void>;
  duplicate: (id: string) => Promise<void>;
  setError: (msg: string | null) => void;
}

export const useInstances = create<InstancesState>((set, get) => ({
  list: [],
  loaded: false,
  status: {},
  error: null,
  load: async () => {
    const list = await api.instanceList();
    set({ list, loaded: true });
  },
  subscribeEvents: async () => {
    await onCoreEvent((event) => {
      if (event.event === "launch_state") {
        set((s) => ({
          status: {
            ...s.status,
            [event.instanceId]: { phase: event.phase, exitCode: event.exitCode },
          },
        }));
        if (event.phase === "exited") {
          // Обновить launchCount/lastPlayed из ядра
          void get().load();
        }
      } else if (event.event === "dl_progress") {
        const active = Object.entries(get().status).find(
          ([, st]) => st.phase === "downloading" || st.phase === "preparing",
        );
        if (active) {
          const [id] = active;
          set((s) => ({
            status: {
              ...s.status,
              [id]: {
                ...s.status[id],
                phase: "downloading",
                doneFiles: event.doneFiles,
                totalFiles: event.totalFiles,
              },
            },
          }));
        }
      }
    });
  },
  launch: async (id, player) => {
    set((s) => ({ status: { ...s.status, [id]: { phase: "preparing" } }, error: null }));
    try {
      await api.instanceLaunch(id, player);
    } catch (e) {
      set((s) => {
        const status = { ...s.status };
        delete status[id];
        return { status, error: String((e as { message?: string })?.message ?? e) };
      });
      await get().load();
    }
  },
  create: async (mcVersion, name) => {
    const inst = await api.instanceCreate(mcVersion, name);
    await get().load();
    return inst;
  },
  remove: async (id, wipe) => {
    await api.instanceDelete(id, wipe);
    await get().load();
  },
  duplicate: async (id) => {
    await api.instanceDuplicate(id);
    await get().load();
  },
  setError: (msg) => set({ error: msg }),
}));
