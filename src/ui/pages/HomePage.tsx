// Главная «Сланца» (по прототипу slate-pro): hero последнего инстанса,
// список инстансов, популярные модпаки Modrinth с рабочей установкой
// (modpack_install: качает .mrpack по project_id в ядре). Никакого фейка:
// секция модпаков не рисуется без сети/данных.
import { useEffect, useMemo, useState } from "react";
import { Download, Inbox, Plus, SearchX, Sparkles, FolderOpen } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { useInstances } from "../../state/instances";
import { useAccounts } from "../../state/accounts";
import { useSettings } from "../../state/settings";
import { api } from "../../api/client";
import { apiErrorText, currentLanguage, t } from "../../i18n";
import { pickImagePath } from "../components/pickFile";
import { defaultArtwork } from "../defaultArt";
import InstanceCard from "../components/InstanceCard";
import InstanceSettingsModal from "../components/InstanceSettingsModal";
import RenameInstanceModal from "../components/RenameInstanceModal";
import LogViewerModal from "../components/LogViewerModal";
import DeleteInstanceDialog from "../components/DeleteInstanceDialog";
import { rowClickOpens } from "./InstanceDetailPage";
import type { Instance, SearchHit } from "../../api/types";

/** Колбэки строки (без status): строятся один раз на инстанс, чтобы memo работал. */
type RowCallbacks = {
  onLaunch: () => void;
  onStop: () => void;
  onOptimize: () => void;
  onFolder: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onSettings: () => void;
  onRename: () => void;
  onLogs: () => void;
  onBackup: () => void;
  onExport: () => void;
  onForceUnlock: () => void;
  onIconSet: () => void;
  onIconClear?: () => void;
};

/**
 * Карточка каталога в стиле CurseForge: картинка сверху, ниже название,
 * автор и загрузки. С `onInstall` карточка целиком — кнопка установки
 * (aria-label «Установить — {title}», блокировка на время чужой установки);
 * без него — витринная карточка (мод: ставить некуда без выбора целевого
 * инстанса, `content_install` требует instanceId).
 */
function HitCard({
  hit,
  busy,
  disabled,
  onInstall,
}: {
  hit: SearchHit;
  /** Идёт установка именно этого проекта. */
  busy: boolean;
  /** Идёт установка другого проекта — карточка недоступна (только с onInstall). */
  disabled?: boolean;
  onInstall?: () => void;
}) {
  const className = [
    "flex flex-col overflow-hidden rounded-lg border border-border-app bg-card text-left",
    "transition-all duration-200 hover:scale-[1.02] hover:border-accent/70 hover:shadow-lg hover:shadow-black/25",
    onInstall ? "cursor-pointer disabled:cursor-not-allowed disabled:opacity-60" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const body = (
    <>
      <div className="relative h-28 w-full shrink-0 overflow-hidden bg-surface-2">
        {hit.icon_url ? (
          <img src={hit.icon_url} alt="" loading="lazy" className="size-full object-cover" />
        ) : (
          // Без иконки — градиентная заглушка с первой буквой названия.
          <span
            aria-hidden
            className="grid size-full place-items-center bg-gradient-to-br from-accent/20 to-surface-2 text-2xl font-bold text-accent"
          >
            {hit.title.slice(0, 1).toUpperCase()}
          </span>
        )}
        {busy && (
          <span className="absolute inset-0 grid place-items-center bg-bg/70 text-[12px] font-semibold">
            {t("home.installing")}
          </span>
        )}
      </div>
      <div className="flex min-w-0 flex-col gap-0.5 p-2">
        <span className="truncate text-sm font-semibold leading-tight">{hit.title}</span>
        <span className="truncate text-[12px] text-text-muted">
          {t("mods.by", { author: hit.author })}
        </span>
        <span
          className="flex items-center gap-1 text-[12px] text-text-muted"
          title={t("mods.downloads", { n: hit.downloads.toLocaleString(currentLanguage()) })}
        >
          <Download size={12} aria-hidden />
          {hit.downloads.toLocaleString(currentLanguage())}
        </span>
      </div>
    </>
  );

  return onInstall ? (
    <button
      type="button"
      onClick={onInstall}
      disabled={disabled}
      aria-label={`${t("home.installPack")} — ${hit.title}`}
      className={className}
    >
      {body}
    </button>
  ) : (
    <div className={className}>{body}</div>
  );
}

export default function HomePage({
  query,
  onCreate,
  onOpenInstance,
}: {
  query: string;
  onCreate: () => void;
  /** Открыть страницу инстанса (клик по строке/карточке — не по её кнопкам). */
  onOpenInstance: (id: string) => void;
}) {
  // D8: узкие селекторы — страница не перерисовывается на каждую строку лога.
  const { list, status, launch, stop, optimize, remove, duplicate, setError, load } =
    useInstances(
      useShallow((s) => ({
        list: s.list,
        status: s.status,
        launch: s.launch,
        stop: s.stop,
        optimize: s.optimize,
        remove: s.remove,
        duplicate: s.duplicate,
        setError: s.setError,
        load: s.load,
      })),
    );
  const { list: accounts, setActive } = useAccounts();
  const { settings } = useSettings();
  const [packs, setPacks] = useState<SearchHit[] | null>(null);
  const [mods, setMods] = useState<SearchHit[] | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [packInfo, setPackInfo] = useState<string | null>(null);
  const [settingsInst, setSettingsInst] = useState<Instance | null>(null);
  const [renameInst, setRenameInst] = useState<Instance | null>(null);
  const [logsInst, setLogsInst] = useState<Instance | null>(null);
  // A10: паритет с InstancesPage — честный диалог удаления и итог бэкапа/экспорта.
  const [deleteInst, setDeleteInst] = useState<Instance | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Активный профиль — тот же выбор, что в чипе титлбара.
  const activeAccount =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;

  useEffect(() => {
    // Популярное: топ загрузок среди project_type=modpack. Офлайн → секции нет.
    api
      .modrinthSearch("", undefined, undefined, "modpack", 6, 0)
      .then((r) => setPacks(r.hits))
      .catch(() => setPacks(null));
  }, []);

  useEffect(() => {
    // Популярные моды: ядро ищет с index=downloads (modrinth/api.rs), тип mod.
    api
      .modrinthSearch("", undefined, undefined, "mod", 6, 0)
      .then((r) => setMods(r.hits))
      .catch(() => setMods(null));
  }, []);

  const installPack = async (projectId: string) => {
    setInstalling(projectId);
    setPackInfo(null);
    setError(null);
    try {
      const inst = await api.modpackInstall(projectId);
      setPackInfo(t("mods.packInstalled", { name: inst.name, version: inst.mcVersion }));
      await load();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setInstalling(null);
    }
  };

  const q = query.trim().toLowerCase();
  const visible = q ? list.filter((i) => i.name.toLowerCase().includes(q)) : list;

  // Hero: последний запускаемый (по last_played) либо первый в списке.
  const hero = useMemo(() => {
    if (list.length === 0) return null;
    const sorted = [...list].sort(
      (a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0),
    );
    return sorted[0];
  }, [list]);

  const heroStatus = hero ? status[hero.id] : undefined;
  const heroBusy = heroStatus && heroStatus.phase !== "exited";

  // D8: колбэки строк строятся один раз на список (замыкание на inst), а не
  // заново на каждый рендер — иначе memo(InstanceCard) не срабатывает.
  const rowCallbacks = useMemo(() => {
    const byId = new Map<string, RowCallbacks>();
    for (const inst of list) {
      const cb: RowCallbacks = {
        // F11: у инстанса свой профиль — запускаем от него (активируем, если он
        // ещё не активен). Профиль удалён — глобальный активный, как раньше.
        onLaunch: () => {
          void (async () => {
            const perInst =
              inst.accountId !== undefined
                ? accounts.find((a) => a.id === inst.accountId)
                : undefined;
            try {
              if (perInst && settings?.accountsActiveId !== perInst.id) {
                await setActive(perInst.id);
              }
              await launch(inst.id, perInst?.name ?? activeAccount?.name ?? "Player");
            } catch (e) {
              setError(apiErrorText(e));
            }
          })();
        },
        onStop: () => void stop(inst.id).catch((e) => setError(apiErrorText(e))),
        onOptimize: () => void optimize(inst.id).catch((e) => setError(apiErrorText(e))),
        onFolder: () => void api.instanceOpenDir(inst.id).catch((e) => setError(apiErrorText(e))),
        onDuplicate: () => void duplicate(inst.id).catch((e) => setError(apiErrorText(e))),
        onSettings: () => setSettingsInst(inst),
        onRename: () => setRenameInst(inst),
        onLogs: () => setLogsInst(inst),
        onBackup: () =>
          void api
            .instanceBackup(inst.id)
            .then((r) => setNotice(t("instances.backup.done", { path: r.path })))
            .catch((e) => setError(apiErrorText(e))),
        onExport: () =>
          void api
            .instanceExport(inst.id)
            .then((r) => setNotice(t("instances.export.done", { path: r.path })))
            .catch((e) => setError(apiErrorText(e))),
        // F27: снять зависшую .lock-блокировку (ядро проверяет живой процесс).
        onForceUnlock: () =>
          void api
            .instanceForceUnlock(inst.id)
            .then(() => {
              setNotice(t("instances.forceUnlock.done"));
              void load();
            })
            .catch((e) => setError(apiErrorText(e))),
        onIconSet: () => {
          void (async () => {
            try {
              const path = await pickImagePath();
              if (!path) return; // отмена диалога
              await api.instanceIconSet(inst.id, path);
              await load();
            } catch (e) {
              setError(apiErrorText(e));
            }
          })();
        },
        // A10: не window.confirm, а диалог с выбором «в корзину/навсегда».
        onDelete: () => setDeleteInst(inst),
      };
      if (inst.icon) {
        cb.onIconClear = () =>
          void api
            .instanceIconRemove(inst.id)
            .then(() => void load())
            .catch((e) => setError(apiErrorText(e)));
      }
      byId.set(inst.id, cb);
    }
    return byId;
  }, [
    list,
    accounts,
    activeAccount?.name,
    settings?.accountsActiveId,
    launch,
    stop,
    optimize,
    duplicate,
    setActive,
    setError,
    load,
  ]);

  return (
    <div className="mx-auto flex max-w-[980px] flex-col gap-5">
      {/* A10: итог бэкапа/экспорта — тот же инлайн-баннер, что на «Инстансах». */}
      {notice && (
        <div className="rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success">
          {notice}
        </div>
      )}

      {/* HERO: быстрый запуск последнего инстанса */}
      {hero && (
        <section
          aria-label={t("play")}
          className="flex flex-wrap items-center gap-4 rounded-lg border border-border-app bg-card p-5"
        >
          {/* Иконка + имя героя — та же «дверь» на страницу инстанса, что и строка. */}
          <div
            role="button"
            tabIndex={0}
            aria-label={t("instance.open", { name: hero.name })}
            onClick={(e) => {
              if (rowClickOpens(e.target)) onOpenInstance(hero.id);
            }}
            onKeyDown={(e) => {
              if (e.target !== e.currentTarget) return; // Enter на кнопках карточки
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onOpenInstance(hero.id);
              }
            }}
            className="flex min-w-0 cursor-pointer items-center gap-4"
          >
            <img
              src={hero.icon ?? defaultArtwork(hero.name)}
              alt=""
              aria-hidden
              className="size-16 shrink-0 rounded-lg border border-border-app object-cover"
            />
            <div className="min-w-0">
              <h1 className="truncate text-2xl font-bold leading-tight tracking-tight">
                {hero.name}
              </h1>
              <div className="font-mono text-[13px] text-text-muted">
                {hero.loader
                  ? `${hero.loader} ${hero.loaderVersion ?? ""}`
                  : t("instances.loader.none")}
                {" · "}
                {hero.lastPlayed
                  ? t("home.hero.lastPlayed", {
                      when: new Date(hero.lastPlayed * 1000).toLocaleString(currentLanguage(), {
                        day: "numeric",
                        month: "short",
                        hour: "2-digit",
                        minute: "2-digit",
                      }),
                    })
                  : t("home.hero.neverPlayed")}
              </div>
            </div>
          </div>
          <div className="ml-auto flex items-center gap-2">
            {heroBusy ? (
              <button
                onClick={() => void stop(hero.id).catch((e) => setError(apiErrorText(e)))}
                className="btn-primary"
              >
                {t("play.running")}
              </button>
            ) : (
              <button
                onClick={() => {
                  // F11: у инстанса может быть свой профиль запуска — тот же
                  // резервинг, что у карточек (rowCallbacks.onLaunch).
                  const override = hero.accountId
                    ? accounts.find((a) => a.id === hero.accountId)
                    : undefined;
                  void (async () => {
                    try {
                      if (override && settings?.accountsActiveId !== override.id) {
                        await setActive(override.id);
                      }
                      await launch(hero.id, override?.name ?? activeAccount?.name ?? "Player");
                    } catch (e) {
                      setError(apiErrorText(e));
                    }
                  })();
                }}
                className="btn-primary"
              >
                {t("play")}
              </button>
            )}
            <button
              onClick={() => void optimize(hero.id).catch((e) => setError(apiErrorText(e)))}
              className="btn-ghost"
            >
              <Sparkles size={16} aria-hidden />
              {t("instances.optimize")}
            </button>
            <button
              onClick={() => void api.instanceOpenDir(hero.id).catch((e) => setError(apiErrorText(e)))}
              aria-label={t("instances.folder")}
              className="icon-btn"
            >
              <FolderOpen size={18} aria-hidden />
            </button>
          </div>
        </section>
      )}

      {/* ИНСТАНСЫ */}
      <section className="flex flex-col gap-2">
        <div className="flex items-baseline gap-2">
          <h2 className="text-lg font-semibold tracking-tight">{t("nav.instances")}</h2>
          <span className="chip-mono">{list.length}</span>
          <button onClick={onCreate} className="btn-ghost btn-sm ml-auto">
            <Plus size={16} aria-hidden />
            {t("instances.newInstance")}
          </button>
        </div>

        {list.length === 0 ? (
          <div className="mt-6 flex flex-col items-center gap-3 rounded-lg border border-border-app bg-card px-4 py-10 text-center">
            <Inbox size={32} aria-hidden className="text-text-muted" />
            <div className="text-[15px] font-semibold">{t("instances.empty.title")}</div>
            <div className="max-w-xs text-[13px] text-text-muted">
              {t("instances.empty.desc")}
            </div>
            <button onClick={onCreate} className="btn-ghost btn-sm mt-1">
              <Plus size={16} aria-hidden />
              {t("home.createFirst")}
            </button>
          </div>
        ) : visible.length === 0 ? (
          <div className="mt-6 flex flex-col items-center gap-3 rounded-lg border border-border-app bg-card px-4 py-10 text-center">
            <SearchX size={32} aria-hidden className="text-text-muted" />
            <div className="text-[15px] font-semibold">{t("shell.search.noResults")}</div>
            <div className="max-w-xs text-[13px] text-text-muted">
              {t("shell.search.noResultsHint", { query })}
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(190px,1fr))] gap-3">
            {visible.map((inst) => {
              const callbacks = rowCallbacks.get(inst.id);
              return callbacks ? (
                // Клик по телу карточки открывает страницу инстанса; кнопки
                // внутри (Play/Stop, кебаб и его меню) остаются рабочими.
                <div
                  key={inst.id}
                  role="button"
                  tabIndex={0}
                  aria-label={t("instance.open", { name: inst.name })}
                  onClick={(e) => {
                    if (rowClickOpens(e.target)) onOpenInstance(inst.id);
                  }}
                  onKeyDown={(e) => {
                    if (e.target !== e.currentTarget) return; // Enter на кнопках карточки
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      onOpenInstance(inst.id);
                    }
                  }}
                  className="cursor-pointer rounded-lg"
                >
                  <InstanceCard inst={inst} status={status[inst.id]} callbacks={callbacks} />
                </div>
              ) : null;
            })}
          </div>
        )}
      </section>

      {/* ПОПУЛЯРНЫЕ МОДПАКИ: только реальные данные с Modrinth */}
      {packs && packs.length > 0 && (
        <section className="flex flex-col gap-3">
          <div className="flex items-baseline gap-2">
            <h2 className="text-lg font-semibold tracking-tight">{t("home.popular")}</h2>
          </div>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
            {packs.map((p) => (
              <HitCard
                key={p.project_id}
                hit={p}
                busy={installing === p.project_id}
                disabled={installing !== null}
                onInstall={() => void installPack(p.project_id)}
              />
            ))}
          </div>
          {packInfo && (
            <p className="rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success">
              {packInfo}
            </p>
          )}
        </section>
      )}

      {/* ПОПУЛЯРНЫЕ МОДЫ: та же карточка, но без установки — content_install
          требует целевой инстанс, а выбора инстанса на Главной нет. */}
      {mods && mods.length > 0 && (
        <section className="flex flex-col gap-3">
          <div className="flex items-baseline gap-2">
            <h2 className="text-lg font-semibold tracking-tight">{t("home.popularMods")}</h2>
          </div>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3">
            {mods.map((m) => (
              <HitCard key={m.project_id} hit={m} busy={false} />
            ))}
          </div>
        </section>
      )}

      {settingsInst && (
        <InstanceSettingsModal
          instance={settingsInst}
          onClose={() => setSettingsInst(null)}
          onSaved={() => void load()}
        />
      )}

      {renameInst && (
        <RenameInstanceModal
          instance={renameInst}
          onClose={() => setRenameInst(null)}
        />
      )}

      {logsInst && (
        <LogViewerModal
          instance={logsInst}
          onClose={() => setLogsInst(null)}
        />
      )}

      {deleteInst && (
        <DeleteInstanceDialog
          instance={deleteInst}
          onCancel={() => setDeleteInst(null)}
          onConfirm={(wipe) => {
            const target = deleteInst;
            setDeleteInst(null);
            void remove(target.id, wipe).catch((e) => setError(apiErrorText(e)));
          }}
        />
      )}
    </div>
  );
}
