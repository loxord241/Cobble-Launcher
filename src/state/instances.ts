// Стейт инстансов + статусы запуска (события ядра → UI).
import { create } from "zustand";
import { api, onCoreEvent } from "../api/client";
import type { Diagnosis, Instance, LaunchPhase } from "../api/types";

export interface LaunchStatus {
  phase: LaunchPhase;
  doneFiles?: number;
  totalFiles?: number;
  exitCode?: number;
}

export interface CrashReport {
  instanceId: string;
  exitCode: number;
  diagnoses: Diagnosis[];
  logTail: string;
}

interface InstancesState {
  list: Instance[];
  loaded: boolean;
  status: Record<string, LaunchStatus>;
  logs: Record<string, string[]>;
  crashReport: CrashReport | null;
  error: string | null;
  load: () => Promise<void>;
  subscribeEvents: () => Promise<void>;
  launch: (id: string, player: string) => Promise<void>;
  create: (mcVersion: string, name: string) => Promise<Instance>;
  rename: (id: string, name: string) => Promise<Instance>;
  saveSettings: (instance: Instance) => Promise<Instance>;
  remove: (id: string, wipe: boolean) => Promise<void>;
  duplicate: (id: string) => Promise<void>;
  optimize: (id: string) => Promise<void>;
  stop: (id: string) => Promise<void>;
  clearCrashReport: () => void;
  setError: (msg: string | null) => void;
}

let isSubscribed = false;

export const useInstances = create<InstancesState>((set, get) => ({
  list: [],
  loaded: false,
  status: {},
  logs: {},
  crashReport: null,
  error: null,
  load: async () => {
    const list = await api.instanceList();
    // Процессы: рестарт лаунчера сбрасывает статусы — восстанавливаем по .lock.
    const running = await api
      .processStatus()
      .then((pairs) => new Set(pairs.map(([id]) => id)))
      .catch(() => new Set<string>());

    // Сохраняем существующие фазы подготовки и загрузки
    const status: Record<string, LaunchStatus> = { ...get().status };
    for (const inst of list) {
      if (running.has(inst.id)) {
        status[inst.id] = { phase: "running" };
      } else if (status[inst.id]?.phase === "running") {
        delete status[inst.id];
      }
    }
    const currentIds = new Set(list.map((i) => i.id));
    for (const id of Object.keys(status)) {
      if (!currentIds.has(id)) {
        delete status[id];
      }
    }

    set({ list, loaded: true, status });
  },
  subscribeEvents: async () => {
    if (isSubscribed) return;
    isSubscribed = true;

    await onCoreEvent((event) => {
      if (event.event === "launch_state") {
        const { instanceId, phase, exitCode } = event;
        set((s) => ({
          status: {
            ...s.status,
            [instanceId]: { phase, exitCode },
          },
        }));

        if (phase === "exited") {
          // Обновить статистику launchCount/lastPlayed из ядра
          void get().load();

          if (exitCode !== undefined && exitCode !== 0) {
            const instanceLogs = get().logs[instanceId] ?? [];
            const logText = instanceLogs.join("\n");
            void api.crashAnalyze(logText).then((diagnoses) => {
              set({
                crashReport: {
                  instanceId,
                  exitCode,
                  diagnoses,
                  logTail: instanceLogs.slice(-50).join("\n"),
                },
              });
            }).catch(() => {
              set({
                crashReport: {
                  instanceId,
                  exitCode,
                  diagnoses: [],
                  logTail: instanceLogs.slice(-50).join("\n"),
                },
              });
            });
          }
        }
      } else if (event.event === "game_log_line") {
        const { instanceId, line } = event;
        set((s) => {
          const current = s.logs[instanceId] ?? [];
          const updated = current.length >= 300 ? [...current.slice(1), line] : [...current, line];
          return {
            logs: {
              ...s.logs,
              [instanceId]: updated,
            },
          };
        });
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
    // Очистить логи перед новым запуском
    set((s) => ({
      status: { ...s.status, [id]: { phase: "preparing" } },
      logs: { ...s.logs, [id]: [] },
      crashReport: s.crashReport?.instanceId === id ? null : s.crashReport,
      error: null,
    }));
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
  rename: async (id, name) => {
    const inst = await api.instanceRename(id, name);
    await get().load();
    return inst;
  },
  saveSettings: async (instance) => {
    const inst = await api.instanceSettingsSet(instance);
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
  optimize: async (id: string) => {
    await api.instanceOptimize(id);
    await get().load();
  },
  stop: async (id: string) => {
    await api.instanceKill(id);
    set((s) => {
      const status = { ...s.status };
      delete status[id];
      return { status };
    });
    void get().load();
  },
  clearCrashReport: () => set({ crashReport: null }),
  setError: (msg) => set({ error: msg }),
}));
