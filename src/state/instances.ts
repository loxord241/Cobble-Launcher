// Стейт инстансов + статусы запуска (события ядра → UI).
import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, onCoreEvent } from "../api/client";
import { apiErrorText } from "../i18n";
import { useAccounts } from "./accounts";
import type {
  Diagnosis,
  Instance,
  LaunchPhase,
  QueueState,
} from "../api/types";

export interface LaunchStatus {
  phase: LaunchPhase;
  /** Момент входа в фазу (мс) — для watchdog зависших preparing/downloading. */
  startedAt: number;
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
  // Таймер живучести гасим вместе с подпиской — повторный старт в subscribeEvents.
  if (runningPollTimer !== null) {
    clearInterval(runningPollTimer);
    runningPollTimer = null;
  }
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

// P1-watchdog: preparing/downloading/launching без событий ядра (рестарт ядра,
// потерянный IPC) висят вечно. Если фазе старше 90 секунд, процесса нет и
// dl_progress не свежий — статус снимаем. 90с хватает и на Java-прогону без
// скачиваний, а честная длинная загрузка модпака держится на dl-событиях.
const WATCHDOG_MS = 90_000;
/** dl_progress свежее этого порога считается активной загрузкой. */
const DL_FRESH_MS = 15_000;
/** Момент последнего dl_progress по инстансу — критерий «загрузка жива». */
const lastDlProgressAt = new Map<string, number>();

/** ENV-10 + P1: тик опроса — снимает зависшие статусы running и заглохшие
 *  preparing/downloading/launching. */
async function pollRunningProcesses() {
  const now = Date.now();
  const statuses = useInstances.getState().status;
  const entries = Object.entries(statuses);
  const hung = entries.filter(([, st]) => {
    if (st.phase !== "preparing" && st.phase !== "downloading" && st.phase !== "launching") {
      return false;
    }
    return now - st.startedAt > WATCHDOG_MS;
  });
  // Пока UI не считает запущенным ничего и нечему протухать, ядро дёргать не о чем.
  if (!entries.some(([, st]) => st.phase === "running") && hung.length === 0) return;
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
      const st = status[id];
      if (!st) continue;
      if (st.phase === "running" && !alive.has(id)) {
        delete status[id];
        changed = true;
      } else if (
        (st.phase === "preparing" || st.phase === "downloading" || st.phase === "launching") &&
        now - st.startedAt > WATCHDOG_MS &&
        // Процесс по ядру не жив (подготовка/загрузка игрового процесса не имеют)
        !alive.has(id) &&
        // Активный dl_progress (события ~раз в секунду) держит фазу: не рвём
        // честную длинную загрузку модпака из-за одного медленного файла.
        now - (lastDlProgressAt.get(id) ?? 0) > DL_FRESH_MS
      ) {
        delete status[id];
        lastDlProgressAt.delete(id);
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

// D18/D60: владелец прогресса теперь приезжает в самом событии — ядро извлекает
// instanceId из группы задач (`instance:{id}` / `mrpack:{id}`). Поле объявлено
// в общем DlProgressEvent (types.ts); для событий без instanceId (старое ядро)
// остаётся эвристика-фолбэк по группе.

export const useInstances = create<InstancesState>((set, get) => ({
  list: [],
  loaded: false,
  status: {},
  logs: {},
  queueState: null,
  crashReport: null,
  error: null,
  load: async () => {
    try {
      const list = await api.instanceList();
      // Процессы: рестарт лаунчера сбрасывает статусы — восстанавливаем по .lock.
      const running = await api
        .processStatus()
        .then((pairs) => new Set(pairs.map(([id]) => id)))
        .catch(() => new Set<string>());

      // Сохраняем существующие фазы подготовки и загрузки
      const status: Record<string, LaunchStatus> = { ...get().status };
      for (const inst of list) {
        const prev = status[inst.id];
        if (running.has(inst.id)) {
          // P3: свежий exited/launching от события ядра не перезаписываем
          // опросом — процесс мог ещё не реапнуться, а перезапись exited
          // обратно в running ломает guard crash-репорта (гонка).
          if (prev?.phase !== "exited" && prev?.phase !== "launching") {
            status[inst.id] = { phase: "running", startedAt: Date.now() };
          }
        } else if (prev?.phase === "running") {
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
    } catch (e) {
      // P3: ошибка загрузки не выставляет loaded — UI останется на экране
      // загрузки/повтора с честной ошибкой, а не с пустым списком навсегда.
      set({ error: apiErrorText(e) });
    }
  },
  subscribeEvents: async () => {
    if (isSubscribed) return;
    isSubscribed = true;
    // ENV-10: опрос живучести статусов «running», восстановленных из .lock.
    ensureRunningPoll();

    // A32: unlisten держим модульно — иначе слушатели Tauri не снять.
    let unlisten: UnlistenFn;
    try {
      unlisten = await onCoreEvent((event) => {
        if (event.event === "launch_state") {
          const { instanceId, phase, exitCode } = event;

          set((s) => {
            const status = { ...s.status };
            if (phase === "exited" && !status[instanceId]) {
              // P3: фантомный exited — статуса не было (событие от прошлого
              // запуска после рестарта лаунчера). Статус не resurrect-им:
              // создаём exited только если запуск был виден стору, иначе
              // просто стираем следы. Честный exited после running/preparing
              // остаётся — UI считает «не занят» по `phase !== "exited"`.
              delete status[instanceId];
            } else {
              status[instanceId] = { phase, exitCode, startedAt: Date.now() };
            }
            return { status };
          });

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
          // D18/D60: если ядро прислало instanceId — привязываем прогресс строго
          // к этому инстансу. Иначе (старое ядро) прежняя эвристика-фолбэк:
          // владелец по группе `instance:{id}`. Группы `mrpack:*` дают id
          // проекта модпака, а не инстанса — статус не найдётся и апдейт
          // отсечётся ниже, как и раньше.
          const dl = event;
          const id =
            dl.instanceId ??
            (dl.group.startsWith("instance:")
              ? dl.group.slice("instance:".length)
              : undefined);
          if (!id) return;
          // P1-watchdog: любой прогресс помечает загрузку живой — честная
          // длинная установка модпака не снимается по таймауту.
          lastDlProgressAt.set(id, Date.now());
          const current = get().status[id];
          if (!current) {
            // P3/D60: прогресс по инстансу, о котором стор не знал (установка
            // модпака / смена версии в обход launch()) — создаём виртуальный
            // статус загрузки. Без явного instanceId от ядра статусы НЕ
            // плодим: эвристика по группе могла угадать не того владельца.
            if (dl.instanceId) {
              set((s) => ({
                status: {
                  ...s.status,
                  [id]: {
                    phase: "downloading",
                    doneFiles: dl.doneFiles,
                    totalFiles: dl.totalFiles,
                    startedAt: Date.now(),
                  },
                },
              }));
            }
            return;
          }
          if (current.phase !== "downloading" && current.phase !== "preparing") return;
          set((s) => ({
            status: {
              ...s.status,
              [id]: {
                ...s.status[id],
                phase: "downloading",
                doneFiles: dl.doneFiles,
                totalFiles: dl.totalFiles,
              },
            },
          }));
        } else if (event.event === "account_refresh_failed") {
          // F9: фоновый refresh токена профиля упал — помечаем аккаунт
          // деградировавшим (UI-показ — чужая волна; состояние и API:
          // useAccounts.degradedIds + setDegraded/clearDegraded).
          useAccounts.getState().setDegraded(event.accountId);
        }
      });
    } catch (e) {
      // P1: флаг ставился ДО await onCoreEvent — при ошибке подписки откатываем
      // его (повторный вызов из Shell попробует снова), иначе стор навсегда
      // считает себя подписанным без единого слушателя. Таймер опроса не трогаем:
      // без статусов он безвреден.
      isSubscribed = false;
      console.error("[instances] подписка на события ядра не удалась:", e);
      return;
    }
    // TODO(api/client.ts — чужая полоса): onCoreEvent регистрирует 5 слушателей
    // через Promise.all; при падении одного из listen() уже зарегистрированные
    // остаются висеть без unlisten (частичная утечка до закрытия окна). Чинится
    // только накоплением unlisten'ов в client.ts.

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
      status: { ...s.status, [id]: { phase: "preparing", startedAt: Date.now() } },
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
      status: { ...s.status, [id]: { phase: "preparing", startedAt: Date.now() } },
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
