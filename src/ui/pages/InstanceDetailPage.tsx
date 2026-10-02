// Страница установленного инстанса в стиле CurseForge (запрос владельца):
// крупная карточка с иконкой, именем, чипами и большой кнопкой «Играть», под
// ней — табы «Обзор / Миры / Моды (N) / Логи». Табов «Версии» и «Журнал
// изменений» нет намеренно: у ядра нет для них источника данных — пустышку
// не рисуем.
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
  MonitorSmartphone,
  MoreVertical,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Settings as SettingsIcon,
  Share,
  ShieldCheck,
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
import { defaultArtwork } from "../defaultArt";
import AddContentModal from "../components/AddContentModal";
import DeleteInstanceDialog from "../components/DeleteInstanceDialog";
import InstanceSettingsModal from "../components/InstanceSettingsModal";
import RenameInstanceModal from "../components/RenameInstanceModal";
import WorldsTab from "../components/instance/WorldsTab";
import ScreensTab from "../components/instance/ScreensTab";
import ConfigsTab from "../components/instance/ConfigsTab";
import PackArt from "../components/instance/PackArt";
/** Логический путь файла контента → имя на диске (без .disabled). */
export function fileBase(file: string): string {
  const base = file.slice(file.lastIndexOf("/") + 1);
  return base.endsWith(".disabled") ? base.slice(0, -".disabled".length) : base;
}
import type {
  ContentEntry,
  GameResourceSample,
  LogFileInfo,
  ModMetadata,
  PackArtInfo,
  ProjectMeta,
  SnapshotInfo,
  UpdateCheck,
} from "../../api/types";

type Tab = "overview" | "worlds" | "screens" | "configs" | "mods" | "logs";

const TABS: Tab[] = ["overview", "worlds", "screens", "configs", "mods", "logs"];

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
}: {
  instanceId: string;
  onBack: () => void;
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
  // Поиск/установка модов в модалке прямо здесь, без перехода на страницу Модов.
  const [addOpen, setAddOpen] = useState(false);
  const [installed, setInstalled] = useState<ContentEntry[] | null>(null);
  /** Метаданные Modrinth по projectId: название/иконка строки (CF-стиль). */
  const [meta, setMeta] = useState<Record<string, ProjectMeta>>({});
  const [updates, setUpdates] = useState<UpdateCheck[] | null>(null);
  const [contentBusy, setContentBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  // F8: снапшоты модов (список + подтверждение отката).
  const [snapshots, setSnapshots] = useState<SnapshotInfo[] | null>(null);
  // F10: файл, который копируем в другой инстанс (модалка выбора цели).
  const [copyFor, setCopyFor] = useState<string | null>(null);
  // F6/F9: зависимости и конфликты модов; F25: обложки паков.
  const [modMeta, setModMeta] = useState<Record<string, ModMetadata>>({});
  const [packArts, setPackArts] = useState<Record<string, PackArtInfo>>({});
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
    // Снапшоты — вторичный источник (нет их — панель просто не показывается).
    api.contentSnapshots(instanceId).then(setSnapshots).catch(() => setSnapshots(null));
    // F6/F9: метаданные модов; F25: обложки ресурспаков/шейдеров.
    api
      .contentMetadata(instanceId)
      .then((rows) =>
        setModMeta(Object.fromEntries(rows.map((r) => [r.file, r]))),
      )
      .catch(() => setModMeta({}));
    api
      .packArt(instanceId, ["resourcepack", "shader"])
      .then((rows) =>
        setPackArts(Object.fromEntries(rows.map((r) => [r.file, r]))),
      )
      .catch(() => setPackArts({}));
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

  // CF-стиль таба «Моды»: названия и иконки — ОДНИМ батч-запросом по записям
  // с projectId (чисто локальные моды сеть не дёргают). Открытие таба мету
  // обновляет: она дешёвая и может устареть (переименование проекта).
  const projectIdsKey = useMemo(
    () =>
      [
        ...new Set((installed ?? []).map((e) => e.projectId).filter((id): id is string => !!id)),
      ].join(","),
    [installed],
  );

  // Моды из модпаков, установленных до появления projectId в манифесте:
  // достаём связь файл→проект батч-проверкой обновлений (она ищет по sha1)
  // и дозаполняем projectId в стейте страницы (манифест на диске не трогаем).
  const backfillTried = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (tab !== "mods" || !installed) return;
    if (backfillTried.current.has(instanceId)) return;
    const missing = installed.filter((e) => !e.projectId && e.sha1);
    if (missing.length === 0) return;
    backfillTried.current.add(instanceId); // одна попытка: не looping по модам вне Modrinth
    void (async () => {
      try {
        const filled = await api.contentBackfillProjects(instanceId);
        if (filled > 0) {
          // Манифест на диске обновлён ядром — перечитываем записи.
          setInstalled(await api.contentInstalled(instanceId));
        }
      } catch {
        // Нет сети/нет связи — строки остаются с именами файлов.
      }
    })();
  }, [tab, installed, instanceId]);

  useEffect(() => {
    if (tab !== "mods" || projectIdsKey === "") return;
    let cancelled = false;
    void (async () => {
      try {
        const list = await api.modrinthProjects(projectIdsKey.split(","));
        if (cancelled) return;
        setMeta((prev) => {
          const next = { ...prev };
          for (const m of list) next[m.projectId] = m;
          return next;
        });
      } catch {
        // Мета недоступна (нет сети, проект скрыт) — строки остаются с именами
        // файлов, как было до этой фичи; отдельной ошибки не показываем.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [tab, projectIdsKey]);

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
        <img
          src={inst.icon ?? defaultArtwork(inst.name)}
          alt=""
          aria-hidden
          className="size-16 shrink-0 rounded-lg border border-border-app object-cover"
        />

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
                  className="anim-menu-in origin-top-right absolute right-0 top-11 z-50 min-w-48 rounded-lg border border-border-strong bg-card p-1 shadow-lg"
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
                  {/* F4: ярлык запуска инстанса на рабочем столе. */}
                  <MenuItem
                    icon={MonitorSmartphone}
                    label={t("instances.shortcut")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      void api
                        .instanceShortcut(instanceId)
                        .then((r) => setNotice(t("instances.shortcut.done", { path: r.path })))
                        .catch((e) => setError(apiErrorText(e)));
                    }}
                  />
                  {/* F2: проверка целостности игровых файлов (saves/config не трогает). */}
                  <MenuItem
                    icon={ShieldCheck}
                    label={t("instances.repair.menu")}
                    disabled={busy}
                    onClick={() => {
                      closeMenu();
                      setNotice(t("instances.repair.started"));
                      void api
                        .instanceRepair(instanceId)
                        .then((r) =>
                          setNotice(
                            t("instances.repair.done", {
                              checked: r.checked,
                              redownloaded: r.redownloaded,
                            }),
                          ),
                        )
                        .catch((e) => setError(apiErrorText(e)));
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
              : id === "worlds"
                ? t("instances.tab.worlds")
                : id === "screens"
                  ? t("instances.tab.screens")
                  : id === "configs"
                    ? t("instance.configs.title")
                    : t(`instance.tab.${id}`)}
          </button>
        ))}
      </div>

      <div
        key={tab}
        role="tabpanel"
        id={`inst-panel-${tab}`}
        aria-labelledby={`inst-tab-${tab}`}
        className="anim-fade-up flex flex-col gap-4"
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

        {tab === "worlds" && (
          <WorldsTab instanceId={instanceId} setError={setError} setNotice={setNotice} />
        )}

        {tab === "screens" && <ScreensTab instanceId={instanceId} setError={setError} />}

        {tab === "configs" && (
          <ConfigsTab instanceId={instanceId} setError={setError} setNotice={setNotice} />
        )}

        {tab === "mods" && (
          <>
            <div className="rounded-lg border border-border-app bg-card p-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-semibold">{t("mods.installed")}</span>
                <span className="chip-mono">{installed?.length ?? 0}</span>
                <div className="ml-auto flex flex-wrap items-center gap-2">
                  <button
                    onClick={() => setAddOpen(true)}
                    className="btn-ghost btn-sm"
                  >
                    <Plus size={14} aria-hidden />
                    {t("addContent.open")}
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

              {/* F8: снапшоты модов — авто перед «Обновить все», откат в клик. */}
              {snapshots !== null && snapshots.length === 0 && (
                <div className="mt-2 border-t border-border-app pt-2">
                  <div className="flex flex-wrap items-center gap-2 px-2">
                    <span className="text-[13px] font-semibold text-text">{t("mods.snapshots")}</span>
                    <span className="text-[12px] text-text-muted">{t("mods.snapshot.hint")}</span>
                  </div>
                  {/* Пустое состояние — своей строкой слева, как пункт списка. */}
                  <p className="mt-1 px-2 pb-1 text-[12px] text-text-muted">
                    {t("mods.snapshots.empty")}
                  </p>
                </div>
              )}
              {snapshots !== null && snapshots.length > 0 && (
                <div className="mt-2 border-t border-border-app pt-2">
                  <div className="flex flex-wrap items-center gap-2 px-2">
                    <span className="text-[13px] font-semibold text-text">
                      {t("mods.snapshots")}
                    </span>
                    <span className="chip-mono">{snapshots.length}</span>
                    <span className="text-[12px] text-text-muted">{t("mods.snapshot.hint")}</span>
                  </div>
                  <ul className="mt-1 flex flex-col gap-1 px-2 pb-1">
                    {snapshots.map((sn) => (
                      <li key={sn.name} className="flex items-center gap-2">
                        <span className="font-mono text-[12px] text-text-muted">{sn.name}</span>
                        <button
                          disabled={contentBusy}
                          onClick={() => {
                            setContentBusy(true);
                            setError(null);
                            api
                              .contentRollback(instanceId, sn.name)
                              .then((entries) => {
                                setInstalled(entries);
                                setNotice(t("mods.rollback.done", { name: sn.name }));
                              })
                              .catch((e) => setError(apiErrorText(e)))
                              .finally(() => setContentBusy(false));
                          }}
                          className="btn-ghost btn-sm ml-auto"
                        >
                          {t("mods.rollback")}
                        </button>
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              <div className="mt-2 border-t border-border-app pt-2">
                {installed === null ? (
                  <p className="px-2 py-3 text-[13px] text-text-muted">{t("instance.mods.loading")}</p>
                ) : installed.length === 0 ? (
                  <p className="px-2 py-3 text-[13px] text-text-muted">{t("instance.mods.empty")}</p>
                ) : (
                  <ul className="flex flex-col gap-1">
                    {installed.map((e) => {
                      const m = e.projectId ? meta[e.projectId] : undefined;
                      const title = m?.title ?? fileBase(e.file);
                      return (
                        <li
                          key={e.file}
                          className={`grid min-h-14 grid-cols-[40px_1fr_auto_auto_auto_auto] items-center gap-3 rounded-md border border-border-app px-3 py-2 transition-colors hover:border-accent/60 hover:bg-card-hover ${
                            e.enabled ? "" : "opacity-55"
                          }`}
                        >
                          {(e.kind === "resourcepack" || e.kind === "shader") &&
                          packArts[e.file] !== undefined ? (
                            <PackArt art={packArts[e.file]}>
                              <span />
                            </PackArt>
                          ) : m?.iconUrl ? (
                            <img
                              src={m.iconUrl}
                              alt=""
                              className="size-10 rounded-md border border-border-app object-cover"
                            />
                          ) : (
                            <span
                              aria-hidden
                              className="grid size-10 place-items-center rounded-md bg-surface-2 text-sm font-semibold text-text-muted"
                            >
                              {fileBase(e.file).slice(0, 1).toUpperCase()}
                            </span>
                          )}
                          <div className="min-w-0">
                            <div className="truncate text-[13px] font-semibold leading-tight" title={title}>
                              {title}
                            </div>
                            <div className="truncate font-mono text-[12px] text-text-muted" title={e.file}>
                              {e.file}
                            </div>
                            {(() => {
                              const mm = modMeta[e.file];
                              if (!mm) return null;
                              const badges: { text: string; cls: string }[] = [];
                              if (mm.missingDeps.length > 0)
                                badges.push({
                                  text: t("mods.deps.missing", { deps: mm.missingDeps.join(", ") }),
                                  cls: "border-error/50 text-error",
                                });
                              if (mm.conflicts.length > 0)
                                badges.push({
                                  text: t("mods.conflicts", { others: mm.conflicts.join(", ") }),
                                  cls: "border-error/50 text-error",
                                });
                              if (mm.disabledDeps.length > 0)
                                badges.push({
                                  text: t("mods.deps.disabled", { name: mm.disabledDeps.join(", ") }),
                                  cls: "border-border-strong text-text-muted",
                                });
                              if (badges.length === 0) return null;
                              return (
                                <div className="mt-0.5 flex flex-wrap gap-1">
                                  {badges.map((b) => (
                                    <span
                                      key={b.text}
                                      className={`rounded border px-1 py-px text-[10px] leading-tight ${b.cls}`}
                                    >
                                      {b.text}
                                    </span>
                                  ))}
                                </div>
                              );
                            })()}
                          </div>
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
                            aria-label={t("mods.copyTo")}
                            title={t("mods.copyTo")}
                            disabled={contentBusy}
                            onClick={() => setCopyFor(e.file)}
                            className="icon-btn"
                          >
                            <Copy size={15} aria-hidden />
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
                      );
                    })}
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

            {/* Модалка поиска/установки контента; после установки перечитываем список. */}
            {addOpen && (
              <AddContentModal
                instance={inst}
                onClose={() => setAddOpen(false)}
                onInstalled={() => void loadInstalled()}
              />
            )}
          </>
        )}

        {tab === "logs" && <LogsPanel instanceId={instanceId} setError={setError} />}
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

      {/* F10: выбор целевого инстанса для копирования мода. */}
      {copyFor && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label={t("mods.copy.choose", { file: fileBase(copyFor) })}
          className="anim-fade-in fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
          onClick={(e) => {
            if (e.target === e.currentTarget) setCopyFor(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") setCopyFor(null);
          }}
        >
          <div className="anim-dialog-in flex max-h-[70vh] w-[420px] max-w-full flex-col rounded-lg border border-border-app bg-card p-4 shadow-2xl">
            <div className="text-sm font-semibold">
              {t("mods.copy.choose", { file: fileBase(copyFor) })}
            </div>
            <div className="mt-3 flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
              {list
                .filter((i) => i.id !== instanceId)
                .map((i) => (
                  <button
                    key={i.id}
                    disabled={contentBusy}
                    onClick={() => {
                      const toId = i.id;
                      setCopyFor(null);
                      setContentBusy(true);
                      setError(null);
                      api
                        .contentCopy(instanceId, toId, copyFor)
                        .then(() => setNotice(t("mods.copyTo.done", { file: fileBase(copyFor), instance: i.name })))
                        .catch((e) => setError(apiErrorText(e)))
                        .finally(() => setContentBusy(false));
                    }}
                    className="flex h-10 items-center gap-2 rounded-md border border-border-app px-3 text-left text-sm hover:border-accent/60 hover:bg-card-hover disabled:opacity-50"
                  >
                    <span className="truncate font-semibold">{i.name}</span>
                    <span className="chip-mono ml-auto">{i.mcVersion}</span>
                  </button>
                ))}
              {list.filter((i) => i.id !== instanceId).length === 0 && (
                <p className="px-1 py-3 text-[13px] text-text-muted">{t("instances.empty.title")}</p>
              )}
            </div>
            <div className="mt-3 flex justify-end border-t border-border-app pt-2">
              <button onClick={() => setCopyFor(null)} className="btn-ghost btn-sm">
                {t("common.close")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** Встроенный просмотр логов инстанса: строки стора + автопрокрутка + копирование. */
function LogsPanel({
  instanceId,
  setError,
}: {
  instanceId: string;
  setError: (msg: string | null) => void;
}) {
  // Узкий селектор: на каждую строку лога перерисовывается только панель логов.
  const lines = useInstances((s) => s.logs[instanceId]) ?? NO_LOGS;
  const [autoScroll, setAutoScroll] = useState(true);
  const [copied, setCopied] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // F22: шеринг на mclo.gs; F23: архив + фильтр уровней.
  const [shareState, setShareState] = useState<{ kind: "busy" | "ok" | "err"; text: string } | null>(null);
  const [filter, setFilter] = useState<"all" | "errors" | "warns">("all");
  const [archive, setArchive] = useState<{ name: string; lines: string[] } | null>(null);
  const [archiveList, setArchiveList] = useState<LogFileInfo[] | null>(null);
  const [archiveOpen, setArchiveOpen] = useState(false);

  const shown =
    archive?.lines ??
    (filter === "all"
      ? lines
      : lines.filter((l) => l.includes(filter === "errors" ? "ERROR" : "WARN")));

  useEffect(() => {
    if (autoScroll && containerRef.current && !archive) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [shown, autoScroll, archive]);

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
          {shown.length} {plural("instances.logs.lines", shown.length)}
        </span>
        <ResMonitor instanceId={instanceId} />
        {/* F23: фильтр уровней (подстроки, как в LogViewerModal). */}
        {(["all", "errors", "warns"] as const).map((f) => (
          <button
            key={f}
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
            className={`rounded-md px-2 py-1 text-xs transition-colors ${
              filter === f ? "bg-accent text-on-accent" : "text-text-muted hover:bg-surface-2"
            }`}
          >
            {t(
              f === "all"
                ? "logs.filter.all"
                : f === "errors"
                  ? "logs.filter.errors"
                  : "logs.filter.warns",
            )}
          </button>
        ))}
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
          disabled={shown.length === 0}
          className="btn-ghost btn-sm"
        >
          {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
          {copied ? t("common.copied") : t("common.copy")}
        </button>
        <button
          disabled={lines.length === 0 || shareState?.kind === "busy"}
          onClick={() => {
            setShareState({ kind: "busy", text: "" });
            api
              .logShareMclogs(instanceId)
              .then((r) => setShareState({ kind: "ok", text: r.url }))
              .catch((e) => setShareState({ kind: "err", text: apiErrorText(e) }));
          }}
          className="btn-ghost btn-sm"
        >
          <Share size={14} aria-hidden />
          {t("logs.share")}
        </button>
      </div>

      {shareState && shareState.kind !== "busy" && (
        <p
          className={`rounded-md border px-3 py-2 text-[13px] ${
            shareState.kind === "ok"
              ? "border-success/40 bg-success/10 text-success"
              : "border-error/40 bg-error/10 text-error"
          }`}
        >
          {shareState.kind === "ok"
            ? t("logs.share.done", { url: shareState.text })
            : shareState.text}
        </p>
      )}

      {/* F23: архив логов — сворачиваемая секция с переключением просмотра. */}
      <div className="rounded-md border border-border-app">
        <button
          aria-expanded={archiveOpen}
          onClick={() => {
            const next = !archiveOpen;
            setArchiveOpen(next);
            if (next && archiveList === null) {
              api
                .instanceLogsList(instanceId)
                .then(setArchiveList)
                .catch(() => setArchiveList([]));
            }
          }}
          className="flex h-9 w-full items-center gap-2 px-3 text-[13px] font-semibold hover:bg-card-hover"
        >
          {/* Счётчик в заголовке — только когда архив уже загружен. */}
          {archiveList !== null
            ? t("logs.historyCount", { n: archiveList.length })
            : t("logs.history")}
        </button>
        {archiveOpen && (
          <div className="border-t border-border-app p-2">
            {archive && (
              <button onClick={() => setArchive(null)} className="btn-ghost btn-sm mb-2">
                <ArrowLeft size={14} aria-hidden />
                {t("onboarding.back")}
              </button>
            )}
            {archiveList === null ? null : archiveList.length === 0 ? (
              <p className="px-1 py-1 text-[13px] text-text-muted">{t("logs.archive.empty")}</p>
            ) : (
              <ul className="flex flex-col gap-1">
                {archiveList.map((f) => (
                  <li key={f.name}>
                    <button
                      onClick={() => {
                        api
                          .instanceLogRead(instanceId, f.name)
                          .then((text) => {
                            setArchive({ name: f.name, lines: text.split(/\r?\n/) });
                            setFilter("all");
                          })
                          .catch((e) => setError(apiErrorText(e)));
                      }}
                      className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] hover:bg-card-hover ${
                        archive?.name === f.name ? "text-accent" : ""
                      }`}
                    >
                      <span className="truncate font-mono">{f.name}</span>
                      <span className="ml-auto text-text-muted">
                        {Math.max(1, Math.round(f.bytes / 1024))} КБ
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>

      <div
        ref={containerRef}
        className="h-[420px] overflow-y-auto rounded-md border border-border-app bg-bg p-3 font-mono text-xs leading-relaxed text-text selection:bg-accent selection:text-on-accent"
      >
        {shown.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center font-ui">
            <FileText size={28} aria-hidden className="text-text-muted" />
            <p>{t("instances.logs.empty")}</p>
            <p className="text-[13px] text-text-muted">{t("instance.logs.emptyHint")}</p>
          </div>
        ) : (
          shown.map((line, idx) => (
            <div key={idx} className="whitespace-pre-wrap break-all hover:bg-surface-2/40">
              {line}
            </div>
          ))
        )}
      </div>
    </div>
  );
}

/** F28: CPU/RAM запущенной игры; опрос раз в 2 с, вне игры не рисуется. */
function ResMonitor({ instanceId }: { instanceId: string }) {
  const status = useInstances((s) => s.status[instanceId]);
  const running = status?.phase === "running";
  const [sample, setSample] = useState<GameResourceSample | null>(null);

  useEffect(() => {
    if (!running) {
      setSample(null);
      return;
    }
    let alive = true;
    const tick = () => {
      api
        .gameResources(instanceId)
        .then((r) => {
          if (alive) setSample(r);
        })
        .catch(() => undefined);
    };
    tick();
    const timer = window.setInterval(tick, 2000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [running, instanceId]);

  if (!running || !sample) return null;
  return (
    <span className="chip-mono text-xs" aria-live="off">
      {t("instances.monitor.cpu", { cpu: Math.round(sample.cpuPercent) })}
      {" · "}
      {t("instances.monitor.ram", {
        ram: `${Math.max(1, Math.round(sample.ramBytes / 1024 / 1024))} МБ`,
      })}
    </span>
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
