// Стейт инстансов + статусы запуска (события ядра → UI).
import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, onCoreEvent } from "../api/client";
import { apiErrorText } from "../i18n";
import type { Diagnosis, Instance, LaunchPhase, QueueState } from "../api/types";

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
  /** Подряд упавших запусков (F30): >=2 — модал предлагает безопасный режим. */
  crashCount: number;
}

/**
 * F30: crashCount читается мягко через локальный тайпгард: поле добавлено в
 * types.ts, но от ядра/старого кэша список может прийти без него — тогда
 * считаем крашей 0 (блок безопасного режима просто не показывается).
 */
type CrashCounted = Instance & { crashCount?: number };
function crashCountOf(inst: Instance | undefined): number {
  const counted = inst as CrashCounted | undefined;
  return typeof counted?.crashCount === "number" ? counted.crashCount : 0;
}

interface InstancesState {
  list: Instance[];
  loaded: boolean;
  status: Record<string, LaunchStatus>;
  logs: Record<string, string[]>;
  /** Состояние очереди загрузок ядра (A30): данные есть, UI-панели пока нет. */
  queueState: QueueState | null;
  crashReport: CrashReport | null;
  error: string | null;
  load: () => Promise<void>;
  subscribeEvents: () => Promise<void>;
  launch: (id: string, player: string) => Promise<void>;
  /** F30: запуск в безопасном режиме (сброс флагов/шейдеров в ядре). */
  launchSafe: (id: string, player: string) => Promise<void>;
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
// A32: слушатели Tauri держим модульно, иначе их нельзя снять (утечка).
let unlistenCore: UnlistenFn | null = null;

/** A32: снять слушатели событий ядра (размонтирование Shell, повторная подписка). */
export function unsubscribeEvents() {
  isSubscribed = false;
  const unlisten = unlistenCore;
  unlistenCore = null;
  unlisten?.();
}

// D8: при запущенной игре game_log_line приходит десятками в секунду, а каждый
// set() перерисовывает всех подписчиков стора. Копим строки в буфере и
// сбрасываем пачкой (API стора не меняется — читатели видят те же logs).
const LOG_FLUSH_MS = 200;
const LOG_BUFFER_MAX = 500;
const LOG_KEEP = 300;

/**
 * A9: событие `exited` приходит раньше последних строк stdout (они ещё идут по
 * IPC), а crash-анализ читает logs сразу. Буфер сбрасываем немедленно (flushLogs
 * в обработчике), а сам анализ откладываем — иначе в отчёт попадёт лог без
 * хвоста падения.
 */
const CRASH_TAIL_MS = 600;

let logBuffer = new Map<string, string[]>();
let logFlushTimer: ReturnType<typeof setTimeout> | null = null;

function queueLogLine(instanceId: string, line: string) {
  const buf = logBuffer.get(instanceId);
  if (buf) {
    buf.push(line);
    // Старые строки буфера отбрасываем: держим не больше LOG_BUFFER_MAX.
    if (buf.length > LOG_BUFFER_MAX) buf.splice(0, buf.length - LOG_BUFFER_MAX);
  } else {
    logBuffer.set(instanceId, [line]);
  }
  if (logFlushTimer === null) {
    logFlushTimer = setTimeout(flushLogs, LOG_FLUSH_MS);
  }
}

/** Сброс буфера в стор: по таймеру и вручную (перед crash-анализом). */
function flushLogs() {
  if (logFlushTimer !== null) {
    clearTimeout(logFlushTimer);
    logFlushTimer = null;
  }
  if (logBuffer.size === 0) return;
  const pending = logBuffer;
  logBuffer = new Map();
  useInstances.setState((s) => {
    const logs: Record<string, string[]> = { ...s.logs };
    for (const [id, lines] of pending) {
      const merged = [...(logs[id] ?? []), ...lines];
      logs[id] = merged.length > LOG_KEEP ? merged.slice(merged.length - LOG_KEEP) : merged;
    }
    return { logs };
  });
}

/** Буфер инстанса больше не нужен (новый запуск очищает логи). */
function dropBufferedLogs(instanceId: string) {
  logBuffer.delete(instanceId);
}

// ENV-10: игра, запущенная из прошлой сессии лаунчера, восстанавливается в
// load() как running, но супервизора над процессом в этой сессии нет — событие
// launch_state(exited) не придёт, и UI навсегда покажет «Запущено». Лёгкий
// опрос ядра снимает зависший статус, когда процесс исчез из process_status.
const RUNNING_POLL_MS = 4000;
let runningPollTimer: ReturnType<typeof setInterval> | null = null;

/** ENV-10: один тик опроса — снимает статус «running», если процесса больше нет. */
async function pollRunningProcesses() {
  const statuses = useInstances.getState().status;
  // Пока UI не считает запущенным ничего, ядро дёргать не о чем.
  if (!Object.values(statuses).some((st) => st.phase === "running")) return;
  const alive = await api
    .processStatus()
    .then((pairs) => new Set(pairs.map(([id]) => id)))
    .catch(() => undefined);
  // Ядро не ответило — статус не трогаем, повторим на следующем тике.
  if (!alive) return;
  useInstances.setState((s) => {
    const status = { ...s.status };
    let changed = false;
    for (const id of Object.keys(status)) {
      // Снимаем ТОЛЬКО phase "running": подготовку/загрузку ведёт событийный
      // поток, а exited (+crashReport при падении) ставит обработчик launch_state.
      if (status[id]?.phase === "running" && !alive.has(id)) {
        delete status[id];
        changed = true;
      }
    }
    // Ничего не изменилось — возвращаем тот же стейт: подписчиков не дёргаем.
    return changed ? { status } : s;
  });
}

/** ENV-10: таймер — один на модуль (guard от повторной подписки Shell). */
function ensureRunningPoll() {
  if (runningPollTimer !== null) return;
  runningPollTimer = setInterval(() => void pollRunningProcesses(), RUNNING_POLL_MS);
}

// D18: прогресс загрузки принадлежит инстансу, чей launch_state перешёл в
// downloading. В DlProgress (src/api/types.ts) нет instance_id, поэтому
// опираемся на фазу запуска, а не на «первый попавшийся» статус.

export const useInstances = create<InstancesState>((set, get) => ({
  list: [],
  loaded: false,
  status: {},
  logs: {},
  queueState: null,
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
    // ENV-10: опрос живучести статусов «running», восстановленных из .lock.
    ensureRunningPoll();

    // A32: unlisten держим модульно — иначе слушатели Tauri не снять.
    const unlisten = await onCoreEvent((event) => {
      if (event.event === "launch_state") {
        const { instanceId, phase, exitCode } = event;

        set((s) => ({
          status: {
            ...s.status,
            [instanceId]: { phase, exitCode },
          },
        }));

        if (phase === "exited") {
          // Дозаписать буферизованные строки: crash-анализ читает logs сразу.
          flushLogs();
          // Обновить статистику launchCount/lastPlayed из ядра
          void get().load();

          if (exitCode !== undefined && exitCode !== 0) {
            // A9: анализ запускаем с задержкой — последние строки stdout ещё
            // идут по IPC, и без паузы хвост падения в отчёт не попадает.
            setTimeout(() => {
              const status = get().status[instanceId];
              // Инстанс успели перезапустить — отчёт о прошлом падении неактуален.
              if (status && status.phase !== "exited") return;
              const instanceLogs = get().logs[instanceId] ?? [];
              const logText = instanceLogs.join("\n");
              // F30: crashCount ядро инкрементировало ДО события exited,
              // load() выше уже перечитал список инстансов.
              const crashCount = crashCountOf(
                get().list.find((i) => i.id === instanceId),
              );
              void api.crashAnalyze(logText).then((diagnoses) => {
                set({
                  crashReport: {
                    instanceId,
                    exitCode,
                    diagnoses,
                    logTail: instanceLogs.slice(-50).join("\n"),
                    crashCount,
                  },
                });
              }).catch(() => {
                set({
                  crashReport: {
                    instanceId,
                    exitCode,
                    diagnoses: [],
                    logTail: instanceLogs.slice(-50).join("\n"),
                    crashCount,
                  },
                });
              });
            }, CRASH_TAIL_MS);
          }
        }
      } else if (event.event === "game_log_line") {
        queueLogLine(event.instanceId, event.line);
      } else if (event.event === "dl_queue_state") {
        // A30: состояние очереди загрузок (pending/downloading/done/failed).
        // UI-панели под него пока нет — данные честно храним в сторе.
        set({ queueState: event });
      } else if (event.event === "dl_progress") {
        // D18: владелец прогресса — группа задач из события (`instance:{id}`),
        // а не первый попавшийся запуск: при двух параллельных установках
        // прогресс раньше писался в чужой инстанс. mrpack:* пока без UI-панели.
        if (!event.group.startsWith("instance:")) return;
        const id = event.group.slice("instance:".length);
        const current = get().status[id];
        if (!current) return;
        if (current.phase !== "downloading" && current.phase !== "preparing") return;
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
    });

    if (!isSubscribed) {
      // Отписаться успели, пока шла подписка — снимаем свежие слушатели.
      unlisten();
      return;
    }
    // Гонка повторной подписки: прежние слушатели не оставляем висеть.
    if (unlistenCore) unlistenCore();
    unlistenCore = unlisten;
  },
  launch: async (id, player) => {
    // Очистить логи перед новым запуском (в т.ч. ещё не сброшенный буфер)
    dropBufferedLogs(id);
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
        // B11: текст ошибки локализуется по языку UI, а не сырой из ядра.
        return { status, error: apiErrorText(e) };
      });
      await get().load();
    }
  },
  // F30: как launch, но ядро (instance_launch_safe) перед спавном применяет
  // безопасный режим: сбрасывает JVM-флаги инстанса и выключает шейдеры.
  launchSafe: async (id, player) => {
    // Очистить логи перед новым запуском (в т.ч. ещё не сброшенный буфер)
    dropBufferedLogs(id);
    set((s) => ({
      status: { ...s.status, [id]: { phase: "preparing" } },
      logs: { ...s.logs, [id]: [] },
      crashReport: s.crashReport?.instanceId === id ? null : s.crashReport,
      error: null,
    }));
    try {
      await api.instanceLaunchSafe(id, player);
    } catch (e) {
      set((s) => {
        const status = { ...s.status };
        delete status[id];
        // B11: текст ошибки локализуется по языку UI, а не сырой из ядра.
        return { status, error: apiErrorText(e) };
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
