// Страница установленного инстанса в стиле CurseForge (запрос владельца):
// крупная карточка с иконкой, именем, чипами и большой кнопкой «Играть», под
// ней — табы «Обзор / Моды (N) / Логи». Табов «Версии» и «Журнал изменений»
// нет намеренно: у ядра нет для них источника данных — пустышку не рисуем.
// Действия и данные — те же API, что у страниц Инстансов и Модов (логика
// панели «Установленные» переиспользуется, сама панель там остаётся).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Archive,
  ArrowDown,
  ArrowLeft,
  Check,
  Copy,
  FileText,
  FolderOpen,
  Image as ImageIcon,
  ImageOff,
  MoreVertical,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Settings as SettingsIcon,
  Share,
  Sparkles,
  Square,
  Trash2,
} from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { api } from "../../api/client";
import { apiErrorText, currentLanguage, plural, t } from "../../i18n";
import { useAccounts } from "../../state/accounts";
import { useInstances } from "../../state/instances";
import { useSettings } from "../../state/settings";
import { pickImagePath } from "../components/pickFile";
import DeleteInstanceDialog from "../components/DeleteInstanceDialog";
import InstanceSettingsModal from "../components/InstanceSettingsModal";
import RenameInstanceModal from "../components/RenameInstanceModal";
import { fileBase } from "./ModsPage";
import type { ContentEntry, UpdateCheck } from "../../api/types";

type Tab = "overview" | "mods" | "logs";

const TABS: Tab[] = ["overview", "mods", "logs"];

/** Логов этого инстанса ещё нет: стабильная ссылка для селектора стора. */
const NO_LOGS: string[] = [];

/**
 * Клик по строке инстанса открывает страницу инстанса, но только если он не
 * попал в управляющий элемент строки (Play/Stop, кебаб, его меню) — иначе
 * кнопки строки перестали бы работать. Подложка открытого меню кебаба в
 * InstanceRow — растянутый на всё окно div.fixed: её узнаём по классу.
 */
export function rowClickOpens(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (
    target.closest(
      "button, a, input, select, textarea, [role='menu'], [role='menuitem'], [role='menuitemradio'], [role='switch']",
    )
  ) {
    return false;
  }
  return !target.classList.contains("fixed");
}

export default function InstanceDetailPage({
  instanceId,
  onBack,
  onOpenMods,
}: {
  instanceId: string;
  onBack: () => void;
  /** Переход на страницу Модов (+ подстановка этого инстанса целью установки). */
  onOpenMods: (instanceId: string) => void;
}) {
  // D8: узкие селекторы — страница не перерисовывается на каждую строку лога.
  const { list, loaded, status, launch, stop, optimize, remove, duplicate, error, setError, load } =
    useInstances(
      useShallow((s) => ({
        list: s.list,
        loaded: s.loaded,
        status: s.status,
        launch: s.launch,
        stop: s.stop,
        optimize: s.optimize,
        remove: s.remove,
        duplicate: s.duplicate,
        error: s.error,
        setError: s.setError,
        load: s.load,
      })),
    );
  const { list: accounts } = useAccounts();
  const { settings } = useSettings();

  const [tab, setTab] = useState<Tab>("overview");
  const [menuOpen, setMenuOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [installed, setInstalled] = useState<ContentEntry[] | null>(null);
  const [updates, setUpdates] = useState<UpdateCheck[] | null>(null);
  const [contentBusy, setContentBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const kebabRef = useRef<HTMLButtonElement>(null);
  const firstItemRef = useRef<HTMLButtonElement>(null);

  const inst = useMemo(() => list.find((i) => i.id === instanceId), [list, instanceId]);
  const alive = inst !== undefined;

  // Инстанс удалили (в т.ч. из другого окна), пока страница открыта — уходим к списку.
  useEffect(() => {
    if (loaded && !alive) onBack();
  }, [loaded, alive, onBack]);

  // Фокус на первый пункт меню при открытии — навигация стрелками/Tab.
  useEffect(() => {
    if (menuOpen) firstItemRef.current?.focus();
  }, [menuOpen]);

  const loadInstalled = useCallback(async () => {
    try {
      setInstalled(await api.contentInstalled(instanceId));
    } catch (e) {
      setInstalled([]);
      setError(apiErrorText(e));
    }
  }, [instanceId, setError]);

  useEffect(() => {
    void loadInstalled();
  }, [loadInstalled]);

  const activeAccount =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;
  const st = status[instanceId];
  const phase = st?.phase;
  const busy = phase !== undefined && phase !== "exited";
  // Статус запуска — как в строке инстанса (aria-live для скринридеров).
  const statusText = !phase
    ? null
    : phase === "downloading"
      ? t("play.downloading", { done: st?.doneFiles ?? 0, total: st?.totalFiles ?? 0 })
      : phase === "running"
        ? t("play.running")
        : phase === "preparing" || phase === "launching"
          ? t("play.preparing")
          : null;

  const closeMenu = (refocus = false) => {
    setMenuOpen(false);
    if (refocus) kebabRef.current?.focus();
  };

  const toggleEntry = async (file: string) => {
    setContentBusy(true);
    setError(null);
    try {
      const e = await api.contentToggle(instanceId, file);
      setNotice(t(e.enabled ? "mods.switchedOn" : "mods.switchedOff", { file }));
      await loadInstalled();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setContentBusy(false);
    }
  };

  const removeEntry = async (file: string) => {
    setContentBusy(true);
    setError(null);
    try {
      await api.contentRemove(instanceId, file);
      setNotice(t("mods.removed", { file }));
      await loadInstalled();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setContentBusy(false);
    }
  };

  const checkUpdates = async () => {
    setContentBusy(true);
    setError(null);
    try {
      setUpdates(await api.contentUpdateCheck(instanceId));
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setContentBusy(false);
    }
  };

  const applyUpdates = async () => {
    setContentBusy(true);
    setError(null);
    try {
      const n = await api.contentUpdateAll(instanceId);
      setNotice(t("mods.updatedCount", { n }));
      setUpdates(null);
      await loadInstalled();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setContentBusy(false);
    }
  };

  if (!inst) {
    // Данных нет (инстанс удалён): кнопка на случай, если авто-возврат не сработал.
    return (
      <div className="mx-auto flex max-w-[980px] flex-col gap-4">
        <button onClick={onBack} className="btn-ghost btn-sm self-start">
          <ArrowLeft size={16} aria-hidden />
          {t("instance.back")}
        </button>
      </div>
    );
  }

  // Честная статистика из instance.json (пишется ядром при запусках).
  const hours = Math.floor((inst.playSeconds ?? 0) / 3600);
  const minutes = Math.round(((inst.playSeconds ?? 0) % 3600) / 60);
  // Заметки: пустая строка — это «их нет», а не пустой абзац (как в настройках).
  const notes = inst.notes ?? "";
  const lastPlayed = inst.lastPlayed
    ? new Date(inst.lastPlayed * 1000).toLocaleString(currentLanguage(), {
        day: "numeric",
        month: "short",
        hour: "2-digit",
        minute: "2-digit",
      })
    : null;
  const loaderText = inst.loader
    ? `${inst.loader} ${inst.loaderVersion ?? ""}`.trim()
    : t("instances.loader.none");

  return (
    <div className="mx-auto flex max-w-[980px] flex-col gap-4">
      <button onClick={onBack} className="btn-ghost btn-sm self-start">
        <ArrowLeft size={16} aria-hidden />
        {t("instance.back")}
      </button>

      {error && (
        <p className="rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <div className="rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success">
          {notice}
        </div>
      )}

      {/* Шапка в стиле CurseForge: иконка, имя, чипы и большая кнопка запуска. */}
      <section className="flex flex-wrap items-center gap-4 rounded-lg border border-border-app bg-card p-5">
        {inst.icon ? (
          <img
            src={inst.icon}
            alt=""
            className="size-16 shrink-0 rounded-lg border border-border-app object-cover"
          />
        ) : (
          <span
            aria-hidden
            className="grid size-16 shrink-0 place-items-center rounded-lg bg-accent-soft text-2xl font-bold text-accent"
          >
            {inst.name.slice(0, 1).toUpperCase()}
          </span>
        )}

        <div className="min-w-0 flex-1">
          <h1 className="truncate text-2xl font-bold leading-tight tracking-tight" title={inst.name}>
            {inst.name}
          </h1>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <span className="chip-mono">{inst.mcVersion}</span>
            <span className="chip-mono">{loaderText}</span>
            {statusText && (
              <span className="badge-soft accent" aria-live="polite">
                {statusText}
              </span>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2">
          {busy ? (
            <button
              onClick={() => void stop(instanceId).catch((e) => setError(String(e)))}
              aria-label={t("instance.stop", { name: inst.name })}
              className="flex h-12 min-w-[168px] items-center justify-center gap-2 rounded-lg bg-error px-6 text-[15px] font-semibold transition-colors hover:opacity-90"
            >
              <Square size={16} aria-hidden fill="currentColor" />
              {t("instances.stop")}
            </button>
          ) : (
            <button
              onClick={() =>
                void launch(instanceId, activeAccount?.name ?? "Player").catch((e) =>
                  setError(String(e)),
                )
              }
              aria-label={t("instance.play", { name: inst.name })}
              className="flex h-12 min-w-[168px] items-center justify-center gap-2 rounded-lg bg-accent px-6 text-[15px] font-semibold text-on-accent transition-colors hover:bg-accent-hover"
            >
              <Play size={18} aria-hidden fill="currentColor" strokeWidth={0} />
              {t("play")}
            </button>
          )}

          <button
            onClick={() => void optimize(instanceId).catch((e) => setError(String(e)))}
            className="btn-ghost"
          >
            <Sparkles size={16} aria-hidden />
            {t("instances.optimize")}
          </button>

          <div className="relative">
            <button
              ref={kebabRef}
              onClick={() => setMenuOpen((v) => !v)}
              aria-label={t("instance.actions", { name: inst.name })}
              aria-expanded={menuOpen}
              aria-haspopup="menu"
              className="icon-btn"
            >
              <MoreVertical size={18} aria-hidden />
            </button>
            {menuOpen && (
              <>
                <div className="fixed inset-0 z-40" onClick={() => closeMenu()} />
                <div
                  role="menu"
                  onKeyDown={(e) => {
                    if (e.key === "Escape") closeMenu(true);
                  }}
                  className="absolute right-0 top-11 z-50 min-w-48 rounded-lg border border-border-strong bg-card p-1 shadow-lg"
                >
                  <MenuItem
                    ref={firstItemRef}
                    icon={SettingsIcon}
                    label={t("instances.settings.menu")}
                    onClick={() => {
                      closeMenu();
                      setSettingsOpen(true);
                    }}
                  />
                  <MenuItem
                    icon={Pencil}
                    label={t("instances.rename.menu")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      setRenameOpen(true);
                    }}
                  />
                  <MenuItem
                    icon={FileText}
                    label={t("instances.logs.menu")}
                    onClick={() => {
                      closeMenu();
                      setTab("logs");
                    }}
                  />
                  <MenuItem
                    icon={Archive}
                    label={t("instances.backup.menu")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      void api
                        .instanceBackup(instanceId)
                        .then((r) => setNotice(t("instances.backup.done", { path: r.path })))
                        .catch((e) => setError(String(e)));
                    }}
                  />
                  <MenuItem
                    icon={Share}
                    label={t("instances.export.menu")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      void api
                        .instanceExport(instanceId)
                        .then((r) => setNotice(t("instances.export.done", { path: r.path })))
                        .catch((e) => setError(String(e)));
                    }}
                  />
                  <MenuItem
                    icon={ImageIcon}
                    label={t("instances.icon.menu")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      void (async () => {
                        try {
                          const path = await pickImagePath();
                          if (!path) return; // отмена диалога
                          await api.instanceIconSet(instanceId, path);
                          await load();
                        } catch (e) {
                          setError(apiErrorText(e));
                        }
                      })();
                    }}
                  />
                  {inst.icon && (
                    <MenuItem
                      icon={ImageOff}
                      label={t("instances.icon.clearMenu")}
                      disabled={busy}
                      onClick={() => {
                        closeMenu();
                        void api
                          .instanceIconRemove(instanceId)
                          .then(() => void load())
                          .catch((e) => setError(String(e)));
                      }}
                    />
                  )}
                  <MenuItem
                    icon={FolderOpen}
                    label={t("instances.folder")}
                    onClick={() => {
                      closeMenu();
                      void api.instanceOpenDir(instanceId).catch((e) => setError(String(e)));
                    }}
                  />
                  <MenuItem
                    icon={Copy}
                    label={t("instances.duplicate")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      void duplicate(instanceId).catch((e) => setError(String(e)));
                    }}
                  />
                  <MenuItem
                    icon={Trash2}
                    danger
                    label={t("instances.delete")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      setDeleteOpen(true);
                    }}
                  />
                </div>
              </>
            )}
          </div>
        </div>
      </section>

      {/* Табы страницы инстанса (стиль вкладок типов на странице Модов). */}
      <div role="tablist" aria-label={t("instance.tabs.label")} className="flex flex-wrap gap-1">
        {TABS.map((id) => (
          <button
            key={id}
            role="tab"
            id={`inst-tab-${id}`}
            aria-selected={tab === id}
            // Панель в DOM одна (текущая): ссылаемся только на существующий id.
            aria-controls={tab === id ? `inst-panel-${id}` : undefined}
            onClick={() => setTab(id)}
            className={`h-9 rounded-lg px-3 text-sm font-medium transition-colors ${
              tab === id
                ? "bg-accent text-on-accent"
                : "text-text-muted hover:bg-surface-2 hover:text-text"
            }`}
          >
            {id === "mods"
              ? t("instance.tab.mods", { n: installed?.length ?? 0 })
              : t(`instance.tab.${id}`)}
          </button>
        ))}
      </div>

      <div
        role="tabpanel"
        id={`inst-panel-${tab}`}
        aria-labelledby={`inst-tab-${tab}`}
        className="flex flex-col gap-4"
      >
        {tab === "overview" && (
          <>
            <section className="rounded-lg border border-border-app bg-card p-4">
              <h3 className="text-sm font-semibold">{t("instance.overview.stats")}</h3>
              <div className="mt-2 flex flex-wrap gap-2 text-[13px]">
                <span className="chip-mono">
                  {t("instances.stats.launches", { n: inst.launchCount ?? 0 })}
                </span>
                <span className="chip-mono">
                  {t("instances.stats.playTime", { h: hours, m: minutes })}
                </span>
                {lastPlayed && (
                  <span className="chip-mono">{t("instances.stats.lastPlayed", { at: lastPlayed })}</span>
                )}
              </div>
            </section>

            <section className="rounded-lg border border-border-app bg-card p-4">
              <h3 className="text-sm font-semibold">{t("instance.overview.info")}</h3>
              <dl className="mt-2 grid grid-cols-[minmax(120px,180px)_1fr] gap-x-4 gap-y-2 text-sm">
                <dt className="text-text-muted">{t("instances.version")}</dt>
                <dd className="font-mono">{inst.mcVersion}</dd>
                <dt className="text-text-muted">{t("instances.loader")}</dt>
                <dd className="font-mono">{loaderText}</dd>
              </dl>
            </section>

            <section className="rounded-lg border border-border-app bg-card p-4">
              <h3 className="text-sm font-semibold">{t("instance.overview.notes")}</h3>
              <p className="mt-2 whitespace-pre-wrap text-sm text-text-muted">
                {notes.trim() ? notes : t("instance.overview.noNotes")}
              </p>
            </section>
          </>
        )}

        {tab === "mods" && (
          <>
            <div className="rounded-lg border border-border-app bg-card p-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-semibold">{t("mods.installed")}</span>
                <span className="chip-mono">{installed?.length ?? 0}</span>
                <div className="ml-auto flex flex-wrap items-center gap-2">
                  <button
                    onClick={() => onOpenMods(instanceId)}
                    className="btn-ghost btn-sm"
                  >
                    <Plus size={14} aria-hidden />
                    {t("instance.mods.find")}
                  </button>
                  <button
                    onClick={() => void checkUpdates()}
                    disabled={contentBusy}
                    className="btn-ghost btn-sm"
                  >
                    <RefreshCw size={14} aria-hidden />
                    {t("mods.checkUpdates")}
                  </button>
                  <button
                    onClick={() => void applyUpdates()}
                    disabled={contentBusy || !installed || installed.length === 0}
                    className="btn-primary btn-sm"
                  >
                    <RefreshCw size={14} aria-hidden />
                    {t("mods.applyUpdates")}
                  </button>
                </div>
              </div>

              <div className="mt-2 border-t border-border-app pt-2">
                {installed === null ? (
                  <p className="px-2 py-3 text-[13px] text-text-muted">{t("instance.mods.loading")}</p>
                ) : installed.length === 0 ? (
                  <p className="px-2 py-3 text-[13px] text-text-muted">{t("instance.mods.empty")}</p>
                ) : (
                  <ul className="flex flex-col gap-1">
                    {installed.map((e) => (
                      <li
                        key={e.file}
                        className={`grid min-h-10 grid-cols-[1fr_auto_auto_auto] items-center gap-2 rounded-md px-2 ${
                          e.enabled ? "" : "opacity-55"
                        }`}
                      >
                        <span className="truncate font-mono text-[13px]" title={e.file}>
                          {fileBase(e.file)}
                        </span>
                        <span className="badge-soft">{t(`mods.kind.${e.kind}`)}</span>
                        <button
                          role="switch"
                          aria-checked={e.enabled}
                          aria-label={t(
                            e.enabled ? "mods.installed.disable" : "mods.installed.enable",
                            { file: fileBase(e.file) },
                          )}
                          disabled={contentBusy}
                          onClick={() => void toggleEntry(e.file)}
                          className={`h-6 w-10 rounded-full border border-border-strong transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                            e.enabled ? "bg-accent" : "bg-surface-2"
                          }`}
                        >
                          <span
                            aria-hidden
                            className={`block size-4 rounded-full bg-on-accent transition-transform ${
                              e.enabled ? "translate-x-5" : "translate-x-1"
                            }`}
                          />
                        </button>
                        <button
                          aria-label={t("mods.installed.delete", { file: fileBase(e.file) })}
                          disabled={contentBusy}
                          onClick={() => void removeEntry(e.file)}
                          className="icon-btn text-error"
                        >
                          <Trash2 size={15} aria-hidden />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </div>

            {updates !== null && (
              <div className="rounded-lg border border-border-app bg-card p-3">
                <span className="text-sm font-semibold">
                  {t("mods.updatesFound", { n: updates.length })}
                </span>
                <ul className="mt-2 space-y-1 text-sm text-text-muted">
                  {updates.map((u) => (
                    <li key={u.file}>
                      {u.file}: {u.latestVersionNumber}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </>
        )}

        {tab === "logs" && <LogsPanel instanceId={instanceId} />}
      </div>

      {settingsOpen && (
        <InstanceSettingsModal
          instance={inst}
          onClose={() => setSettingsOpen(false)}
          onSaved={() => void load()}
        />
      )}

      {renameOpen && <RenameInstanceModal instance={inst} onClose={() => setRenameOpen(false)} />}

      {deleteOpen && (
        <DeleteInstanceDialog
          instance={inst}
          onCancel={() => setDeleteOpen(false)}
          onConfirm={(wipe) => {
            setDeleteOpen(false);
            // Страницу закроет авто-возврат: список придёт из ядра без инстанса.
            void remove(instanceId, wipe).catch((e) => setError(String(e)));
          }}
        />
      )}
    </div>
  );
}

/** Встроенный просмотр логов инстанса: строки стора + автопрокрутка + копирование. */
function LogsPanel({ instanceId }: { instanceId: string }) {
  // Узкий селектор: на каждую строку лога перерисовывается только панель логов.
  const lines = useInstances((s) => s.logs[instanceId]) ?? NO_LOGS;
  const [autoScroll, setAutoScroll] = useState(true);
  const [copied, setCopied] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [lines, autoScroll]);

  // Таймер индикатора «Скопировано!» не должен жить после ухода с таба.
  useEffect(
    () => () => {
      if (copyTimer.current !== null) clearTimeout(copyTimer.current);
    },
    [],
  );

  const copyLogs = async () => {
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      setCopied(true);
      if (copyTimer.current !== null) clearTimeout(copyTimer.current);
      copyTimer.current = setTimeout(() => {
        copyTimer.current = null;
        setCopied(false);
      }, 2000);
    } catch {
      // Буфер обмена недоступен — молча ничего (как в LogViewerModal).
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <span className="chip-mono text-xs">
          {lines.length} {plural("instances.logs.lines", lines.length)}
        </span>
        <label className="ml-auto flex cursor-pointer items-center gap-2 text-sm text-text-muted hover:text-text">
          <input
            type="checkbox"
            checked={autoScroll}
            onChange={(e) => setAutoScroll(e.target.checked)}
            className="accent-accent"
          />
          <ArrowDown size={14} aria-hidden />
          {t("instances.logs.autoScroll")}
        </label>
        <button
          onClick={() => void copyLogs()}
          disabled={lines.length === 0}
          className="btn-ghost btn-sm"
        >
          {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
          {copied ? t("common.copied") : t("common.copy")}
        </button>
      </div>

      <div
        ref={containerRef}
        className="h-[420px] overflow-y-auto rounded-md border border-border-app bg-bg p-3 font-mono text-xs leading-relaxed text-text selection:bg-accent selection:text-on-accent"
      >
        {lines.length === 0 ? (
          <div className="text-text-muted">
            <p>{t("instances.logs.empty")}</p>
            <p className="mt-1 text-[13px]">{t("instance.logs.emptyHint")}</p>
          </div>
        ) : (
          lines.map((line, idx) => (
            <div key={idx} className="whitespace-pre-wrap break-all hover:bg-surface-2/40">
              {line}
            </div>
          ))
        )}
      </div>
    </div>
  );
}

/** Пункт меню кебаба (тот же вид, что в строке инстанса). */
function MenuItem({
  icon: Icon,
  label,
  onClick,
  danger,
  disabled,
  ref,
}: {
  icon: typeof FolderOpen;
  label: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
  ref?: React.Ref<HTMLButtonElement>;
}) {
  return (
    <button
      ref={ref}
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={`flex h-10 w-full items-center gap-2 rounded-md px-3 text-left text-sm transition-colors ${
        disabled
          ? "cursor-not-allowed text-text-muted/40"
          : danger
            ? "text-error hover:bg-error/10"
            : "text-text hover:bg-surface-2"
      }`}
    >
      <Icon size={16} aria-hidden />
      {label}
    </button>
  );
}
