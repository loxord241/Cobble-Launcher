// Вкладка «Серверы» страницы инстанса (D68): список выделенных серверов,
// созданных из инстанса. Карточка сервера в стиле секций страницы инстанса
// (`rounded-lg border border-border-app bg-card`), чипы chip-mono, живой
// статус по каналу `server_event` (started/exited/crashed) и панель хвоста
// лога (последние ~400 строк) с автоскроллом, пока пользователь у низа.
// Таб монтируется только когда активен (как «Миры») — данные грузятся лениво.
import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  ChevronDown,
  ChevronUp,
  ExternalLink,
  FolderOpen,
  Loader2,
  Play,
  Plus,
  RefreshCw,
  Square,
  Trash2,
} from "lucide-react";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { api } from "../../../api/client";
import { apiErrorText, t } from "../../../i18n";
import { formatRamMb } from "../../format";
import { askConfirm } from "../../confirm";
import { useInstances } from "../../../state/instances";
import ServerCreateDialog from "./ServerCreateDialog";
import type { ServerEvent, ServerInfo } from "../../../api/types";

/** Официальная страница EULA Minecraft (та же, что пишет ядро в eula.txt). */
const EULA_URL = "https://aka.ms/MinecraftEULA";
/** Хвост лога сервера: старше — отрезаем (как TAIL_LINES в LogViewerModal). */
const LOG_CAP = 400;

/** Строка лога сервера; exited/crashed — статусные строки (иная подсветка). */
type ServerLogEntry = { text: string; kind: "log" | "exited" | "crashed" };

/** Живой статус поверх снимка из server_list (события/действия быстрее). */
type RunOverride = { running: boolean; pid: number | null };

/** Стабильная ссылка для пустого лога (как NO_LOGS на странице инстанса). */
const NO_LOGS: ServerLogEntry[] = [];

/**
 * P3-ревизии: переустановка серверного ПО (пересоздание серверных файлов
 * поверх текущих — деструктив для поставленных библиотек). Контракт
 * `api.serverReinstall(id): Promise<void>` — долгая операция (минуты).
 */

export default function ServerTab({ instanceId }: { instanceId: string }) {
  const [servers, setServers] = useState<ServerInfo[] | null>(null);
  // Таб получает только instanceId — ошибки показывает сам (локальная строка).
  const [error, setError] = useState<string | null>(null);
  /** Сервер, над которым идёт действие (старт/стоп/EULA/удаление) — блок кнопок. */
  const [busyId, setBusyId] = useState<string | null>(null);
  /** P3-ревизии: сервер с переустановкой ПО в полёте (долгая) — статус-строка. */
  const [reinstallId, setReinstallId] = useState<string | null>(null);
  /** Успех последнего действия — notice-строка таба (зеркало строки ошибки). */
  const [notice, setNotice] = useState<string | null>(null);
  const [overrides, setOverrides] = useState<Record<string, RunOverride>>({});
  /** P2-ревизии: поколение событий на сервер — гасит поздний ответ serverStatus. */
  const eventGenRef = useRef<Map<string, number>>(new Map());
  const [logs, setLogs] = useState<Record<string, ServerLogEntry[]>>({});
  /** Явный выбор пользователя; не задан → панель раскрыта при running. */
  const [logOpen, setLogOpen] = useState<Record<string, boolean>>({});
  const [createOpen, setCreateOpen] = useState(false);

  // Имя инстанса — дефолт имени сервера в диалоге создания (данные уже в сторе).
  const instanceName = useInstances((s) => s.list.find((i) => i.id === instanceId)?.name);

  const load = useCallback(async () => {
    try {
      setServers(await api.serverList(instanceId));
    } catch (e) {
      setServers([]);
      setError(apiErrorText(e));
    }
  }, [instanceId]);

  // Ленивая загрузка при активации таба (как «Миры»/«Моды»).
  useEffect(() => {
    void load();
  }, [load]);

  // Живой статус: started/exited/crashed перекрывают снимок из server_list,
  // строки лога (kind=log — ПАЧКА строк в line через \n) копятся в буфер.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let disposed = false;
    void listen<ServerEvent>("server_event", (e) => {
      const ev = e.payload;
      if (ev.kind === "log") {
        if (!ev.line) return;
        const parts = ev.line
          .split("\n")
          .filter((s) => s.length > 0)
          .map((text): ServerLogEntry => ({ text, kind: "log" }));
        if (parts.length === 0) return;
        setLogs((cur) => {
          const next = [...(cur[ev.serverId] ?? []), ...parts];
          return { ...cur, [ev.serverId]: next.length > LOG_CAP ? next.slice(-LOG_CAP) : next };
        });
        return;
      }
      if (ev.kind === "started") {
        // P2-ревизии: ответ serverStatus, прочитанный до смерти процесса,
        // не должен воскрешать карточку — метки поколений.
        const gen = (eventGenRef.current.get(ev.serverId) ?? 0) + 1;
        eventGenRef.current.set(ev.serverId, gen);
        setOverrides((cur) => ({
          ...cur,
          [ev.serverId]: { running: true, pid: cur[ev.serverId]?.pid ?? null },
        }));
        // Панель лога раскрыта при running; pid подтянем статусом (в событии его нет).
        setLogOpen((cur) => ({ ...cur, [ev.serverId]: true }));
        void api
          .serverStatus(ev.serverId)
          .then((st) => {
            if (eventGenRef.current.get(ev.serverId) !== gen) return;
            setOverrides((cur) => ({
              ...cur,
              [ev.serverId]: { running: st.running, pid: st.pid },
            }));
          })
          .catch(() => undefined);
        return;
      }
      // exited | crashed: статус в оверлей + статусная строка в хвост лога.
      eventGenRef.current.set(ev.serverId, (eventGenRef.current.get(ev.serverId) ?? 0) + 1);
      setOverrides((cur) => ({ ...cur, [ev.serverId]: { running: false, pid: null } }));
      const entry: ServerLogEntry =
        ev.kind === "crashed"
          ? { text: t("servers.log.crashed", { code: ev.code ?? 0 }), kind: "crashed" }
          : { text: t("servers.log.exited"), kind: "exited" };
      setLogs((cur) => {
        const next = [...(cur[ev.serverId] ?? []), entry];
        return { ...cur, [ev.serverId]: next.length > LOG_CAP ? next.slice(-LOG_CAP) : next };
      });
    })
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  /** Перечитать статус одного сервера (после действий/accept EULA). */
  const refresh = useCallback(async (id: string) => {
    try {
      const st = await api.serverStatus(id);
      setOverrides((cur) => ({ ...cur, [id]: { running: st.running, pid: st.pid } }));
      setServers((cur) => (cur ? cur.map((s) => (s.id === id ? st : s)) : cur));
    } catch (e) {
      setError(apiErrorText(e));
    }
  }, []);

  /** Обёртка действий: busy на кнопки карточки + честная ошибка в табе. */
  const run = useCallback(
    async (id: string, op: () => Promise<void>) => {
      setBusyId(id);
      setError(null);
      setNotice(null);
      try {
        await op();
      } catch (e) {
        setError(apiErrorText(e));
      } finally {
        setBusyId(null);
      }
    },
    [],
  );

  const start = (s: ServerInfo) =>
    void run(s.id, async () => {
      const pid = await api.serverStart(s.id);
      setOverrides((cur) => ({ ...cur, [s.id]: { running: true, pid } }));
      setLogOpen((cur) => ({ ...cur, [s.id]: true }));
    });

  const stop = (s: ServerInfo) =>
    void run(s.id, async () => {
      await api.serverStop(s.id);
      setOverrides((cur) => ({ ...cur, [s.id]: { running: false, pid: null } }));
      await refresh(s.id);
    });

  const acceptEula = async (s: ServerInfo) => {
    // RULE: window.confirm в Tauri не работает — только askConfirm (confirm.tsx).
    if (
      !(await askConfirm(t("servers.eula.confirm"), { confirmLabel: t("servers.eula.accept") }))
    )
      return;
    void run(s.id, async () => {
      await api.serverAcceptEula(s.id);
      await refresh(s.id);
    });
  };

  const remove = async (s: ServerInfo) => {
    if (
      !(await askConfirm(t("servers.deleteConfirm", { name: s.name }), {
        danger: true,
        confirmLabel: t("common.delete"),
      }))
    )
      return;
    // running-сервер ядро откажется удалять — ошибка уйдёт в строку таба.
    void run(s.id, async () => {
      await api.serverDelete(s.id);
      await load();
    });
  };

  const openDir = (s: ServerInfo) => {
    // opener:default даёт только reveal (open_path запрещён capability) —
    // проводник откроется с выделенной папкой сервера.
    void revealItemInDir(s.dir).catch((e: unknown) => setError(apiErrorText(e)));
  };

  /** P3-ревизии: переустановить серверное ПО. Долгая (минуты) и деструктивная
   * для уже поставленных библиотек — подтверждение текстом кнопки; на время
   * операции кнопки карточки погашены (busyId), в карточке — статус-строка. */
  const reinstall = async (s: ServerInfo) => {
    if (!(await askConfirm(t("servers.reinstall"), { danger: true }))) return;
    setReinstallId(s.id);
    void run(s.id, async () => {
      try {
        await api.serverReinstall(s.id);
        setNotice(t("servers.reinstalled"));
      } finally {
        setReinstallId(null);
      }
    });
  };

  const openEula = () => {
    void openUrl(EULA_URL).catch((e: unknown) => setError(apiErrorText(e)));
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="text-sm font-semibold">{t("servers.tab")}</h3>
        {servers !== null && servers.length > 0 && (
          <span className="chip-mono text-xs">{servers.length}</span>
        )}
        <button onClick={() => setCreateOpen(true)} className="btn-ghost btn-sm ml-auto">
          <Plus size={14} aria-hidden />
          {t("servers.create")}
        </button>
      </div>

      {error && (
        <p role="alert" className="text-[13px] text-error">
          {error}
        </p>
      )}
      {notice && !error && (
        <p role="status" className="text-[13px] text-success">
          {notice}
        </p>
      )}

      {servers === null ? null : servers.length === 0 ? (
        <div className="rounded-lg border border-border-app bg-card p-3">
          <div className="flex min-h-[200px] flex-col items-center justify-center gap-2 text-center">
            <p className="font-semibold">{t("servers.empty")}</p>
            <p className="text-[13px] text-text-muted">{t("servers.create.hint")}</p>
            <button onClick={() => setCreateOpen(true)} className="btn-primary btn-sm mt-2">
              <Plus size={14} aria-hidden />
              {t("servers.create")}
            </button>
          </div>
        </div>
      ) : (
        servers.map((s) => {
          const busy = busyId === s.id;
          const running = overrides[s.id]?.running ?? s.running;
          const pid = overrides[s.id]?.pid ?? s.pid;
          const loaderText = s.loader
            ? `${s.loader} ${s.loaderVersion ?? ""}`.trim()
            : "";
          return (
            <section
              key={s.id}
              className="rounded-lg border border-border-app bg-card p-4"
              aria-busy={busy}
            >
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-[15px] font-semibold" title={s.name}>
                  {s.name}
                </span>
                {running ? (
                  <span className="badge-soft accent">
                    {t("servers.status.running", { pid: pid ?? "—" })}
                  </span>
                ) : (
                  <span className="badge-soft">{t("servers.status.stopped")}</span>
                )}
              </div>

              <div className="mt-2 flex flex-wrap gap-2 text-[13px]">
                <span className="chip-mono">
                  {s.mcVersion}
                  {loaderText ? ` · ${loaderText}` : ""}
                </span>
                <span className="chip-mono tabular-nums">
                  {t("servers.port")} {s.port}
                </span>
                <span className="chip-mono tabular-nums">{formatRamMb(s.ramMb)}</span>
                <span className="chip-mono">{s.world ?? t("servers.world.none")}</span>
                <span className="chip-mono">
                  {s.onlineMode ? t("servers.online.on") : t("servers.online.off")}
                </span>
              </div>

              {!s.eulaAccepted && (
                <div className="mt-3 flex flex-wrap items-center gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-[13px]">
                  <span className="text-warning">{t("servers.eula.notAccepted")}</span>
                  <button
                    onClick={() => void acceptEula(s)}
                    disabled={busy}
                    className="btn-ghost btn-sm"
                  >
                    {t("servers.eula.accept")}
                  </button>
                  <button
                    onClick={openEula}
                    className="ml-auto flex items-center gap-1 text-xs text-text-muted hover:text-text"
                  >
                    <ExternalLink size={12} aria-hidden />
                    {t("servers.eula.link")}
                  </button>
                </div>
              )}

              <div className="mt-3 flex flex-wrap items-center gap-2">
                {running ? (
                  <button
                    onClick={() => stop(s)}
                    disabled={busy}
                    aria-busy={busy}
                    className="btn-ghost btn-sm"
                  >
                    <Square size={14} aria-hidden />
                    {t("servers.stop")}
                  </button>
                ) : (
                  <button
                    onClick={() => start(s)}
                    disabled={busy}
                    aria-busy={busy}
                    className="btn-primary btn-sm"
                  >
                    <Play size={14} aria-hidden />
                    {t("servers.start")}
                  </button>
                )}
                <button
                  onClick={() => openDir(s)}
                  className="btn-ghost btn-sm"
                  aria-label={t("servers.openDir")}
                  title={s.dir}
                >
                  <FolderOpen size={14} aria-hidden />
                  {t("servers.openDir")}
                </button>
                {/* P3-ревизии: тихая кнопка рядом с «Открыть папку» —
                    переустановка серверного ПО (с подтверждением). */}
                <button
                  onClick={() => void reinstall(s)}
                  disabled={busy}
                  className="btn-ghost btn-sm text-text-muted hover:text-text"
                >
                  <RefreshCw size={14} aria-hidden />
                  {t("servers.reinstall")}
                </button>
                <button
                  onClick={() => void remove(s)}
                  disabled={busy}
                  className="btn-ghost btn-sm ml-auto text-error"
                >
                  <Trash2 size={14} aria-hidden />
                  {t("servers.delete")}
                </button>
              </div>

              {/* Переустановка ПО в полёте (долгая, минуты) — статус-строка. */}
              {reinstallId === s.id && (
                <p role="status" className="mt-2 flex items-center gap-2 text-[13px] text-text-muted">
                  <Loader2 size={14} aria-hidden className="animate-spin" />
                  {t("servers.reinstalling")}
                </p>
              )}

              <ServerLogPanel
                lines={logs[s.id] ?? NO_LOGS}
                defaultOpen={running}
                openOverride={logOpen[s.id]}
                onToggle={(open) => setLogOpen((cur) => ({ ...cur, [s.id]: open }))}
              />
            </section>
          );
        })
      )}

      {createOpen && (
        <ServerCreateDialog
          instanceId={instanceId}
          instanceName={instanceName ?? ""}
          onClose={() => setCreateOpen(false)}
          onCreated={() => void load()}
        />
      )}
    </div>
  );
}

/** Хвост лога сервера: заголовок-сворачивалка + моно-простыня с автоскроллом. */
function ServerLogPanel({
  lines,
  defaultOpen,
  openOverride,
  onToggle,
}: {
  lines: ServerLogEntry[];
  defaultOpen: boolean;
  /** undefined — не тронуто пользователем, следует за defaultOpen (running). */
  openOverride: boolean | undefined;
  onToggle: (open: boolean) => void;
}) {
  const open = openOverride ?? defaultOpen;
  const containerRef = useRef<HTMLDivElement>(null);
  // Автоскролл, пока пользователь у низа (ушёл вверх — не дёргаем прокрутку).
  const atBottomRef = useRef(true);

  const scrollToEnd = () => {
    const el = containerRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  };

  useEffect(() => {
    if (open && atBottomRef.current) scrollToEnd();
  }, [lines, open]);

  return (
    <div className="mt-3">
      <button
        onClick={() => onToggle(!open)}
        aria-expanded={open}
        className="flex w-full items-center justify-between rounded-md border border-border-app bg-bg px-3 py-1.5 text-xs font-semibold text-text-muted hover:text-text"
      >
        <span>
          {t("servers.log")}
          {lines.length > 0 && (
            <span className="ml-2 font-normal tabular-nums">{lines.length}</span>
          )}
        </span>
        {open ? <ChevronUp size={14} aria-hidden /> : <ChevronDown size={14} aria-hidden />}
      </button>
      {open && (
        <div
          ref={containerRef}
          onScroll={(e) => {
            const el = e.currentTarget;
            atBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          }}
          className="mt-1 max-h-64 overflow-y-auto rounded-md border border-border-app bg-bg p-3 font-mono text-xs leading-relaxed text-text"
        >
          {lines.length === 0 ? (
            <p className="text-text-muted">{t("servers.log.empty")}</p>
          ) : (
            lines.map((entry, idx) => (
              <div
                key={idx}
                className={`whitespace-pre-wrap break-all ${
                  entry.kind === "crashed"
                    ? "text-error"
                    : entry.kind === "exited"
                      ? "text-text-muted"
                      : ""
                }`}
              >
                {entry.text}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
