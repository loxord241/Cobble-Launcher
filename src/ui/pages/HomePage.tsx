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
import InstanceRow from "../components/InstanceRow";
import InstanceSettingsModal from "../components/InstanceSettingsModal";
import RenameInstanceModal from "../components/RenameInstanceModal";
import LogViewerModal from "../components/LogViewerModal";
import DeleteInstanceDialog from "../components/DeleteInstanceDialog";
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
  onIconSet: () => void;
  onIconClear?: () => void;
};

export default function HomePage({
  query,
  onCreate,
}: {
  query: string;
  onCreate: () => void;
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
  const { list: accounts } = useAccounts();
  const { settings } = useSettings();
  const [packs, setPacks] = useState<SearchHit[] | null>(null);
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

  const installPack = async (projectId: string) => {
    setInstalling(projectId);
    setPackInfo(null);
    setError(null);
    try {
      const inst = await api.modpackInstall(projectId);
      setPackInfo(t("mods.packInstalled", { name: inst.name, version: inst.mcVersion }));
      await load();
    } catch (e) {
      setError(String((e as { message?: string })?.message ?? e));
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
  // заново на каждый рендер — иначе memo(InstanceRow) не срабатывает.
  const rowCallbacks = useMemo(() => {
    const byId = new Map<string, RowCallbacks>();
    for (const inst of list) {
      const cb: RowCallbacks = {
        onLaunch: () =>
          void launch(inst.id, activeAccount?.name ?? "Player").catch((e) => setError(String(e))),
        onStop: () => void stop(inst.id).catch((e) => setError(String(e))),
        onOptimize: () => void optimize(inst.id).catch((e) => setError(String(e))),
        onFolder: () => void api.instanceOpenDir(inst.id).catch((e) => setError(String(e))),
        onDuplicate: () => void duplicate(inst.id).catch((e) => setError(String(e))),
        onSettings: () => setSettingsInst(inst),
        onRename: () => setRenameInst(inst),
        onLogs: () => setLogsInst(inst),
        onBackup: () =>
          void api
            .instanceBackup(inst.id)
            .then((r) => setNotice(t("instances.backup.done", { path: r.path })))
            .catch((e) => setError(String(e))),
        onExport: () =>
          void api
            .instanceExport(inst.id)
            .then((r) => setNotice(t("instances.export.done", { path: r.path })))
            .catch((e) => setError(String(e))),
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
            .catch((e) => setError(String(e)));
      }
      byId.set(inst.id, cb);
    }
    return byId;
  }, [list, activeAccount?.name, launch, stop, optimize, duplicate, setError, load]);

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
          <span
            aria-hidden
            className="grid size-14 place-items-center rounded-lg bg-accent-soft text-2xl font-bold text-accent"
          >
            {hero.name.slice(0, 1).toUpperCase()}
          </span>
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
          <div className="ml-auto flex items-center gap-2">
            {heroBusy ? (
              <button
                onClick={() => void stop(hero.id).catch((e) => setError(String(e)))}
                className="btn-primary"
              >
                {t("play.running")}
              </button>
            ) : (
              <button
                onClick={() =>
                  void launch(hero.id, activeAccount?.name ?? "Player").catch((e) =>
                    setError(String(e)),
                  )
                }
                className="btn-primary"
              >
                {t("play")}
              </button>
            )}
            <button
              onClick={() => void optimize(hero.id).catch((e) => setError(String(e)))}
              className="btn-ghost"
            >
              <Sparkles size={16} aria-hidden />
              {t("instances.optimize")}
            </button>
            <button
              onClick={() => void api.instanceOpenDir(hero.id).catch((e) => setError(String(e)))}
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
          visible.map((inst) => {
            const callbacks = rowCallbacks.get(inst.id);
            return callbacks ? (
              <InstanceRow
                key={inst.id}
                inst={inst}
                status={status[inst.id]}
                {...callbacks}
              />
            ) : null;
          })
        )}
      </section>

      {/* ПОПУЛЯРНЫЕ МОДПАКИ: только реальные данные с Modrinth */}
      {packs && packs.length > 0 && (
        <section className="flex flex-col gap-2">
          <div className="flex items-baseline gap-2">
            <h2 className="text-lg font-semibold tracking-tight">{t("home.popular")}</h2>
          </div>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-2">
            {packs.map((p) => (
              <div
                key={p.project_id}
                className="flex items-center gap-3 rounded-lg border border-border-app bg-card px-3 py-2 transition-colors hover:border-border-strong hover:bg-card-hover"
              >
                {p.icon_url ? (
                  <img
                    src={p.icon_url}
                    alt=""
                    className="size-11 shrink-0 rounded-md border border-border-app object-cover"
                  />
                ) : (
                  <span
                    aria-hidden
                    className="grid size-11 shrink-0 place-items-center rounded-md bg-surface-2 text-sm font-semibold text-text-muted"
                  >
                    {p.title.slice(0, 1)}
                  </span>
                )}
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-semibold leading-tight">{p.title}</div>
                  <div className="font-mono text-[12px] text-text-muted">
                    {t("mods.downloads", { n: p.downloads.toLocaleString(currentLanguage()) })}
                  </div>
                </div>
                <button
                  onClick={() => void installPack(p.project_id)}
                  disabled={installing !== null}
                  className="btn-ghost btn-sm shrink-0"
                  aria-label={`${t("home.installPack")} — ${p.title}`}
                >
                  <Download size={14} aria-hidden />
                  {installing === p.project_id ? t("home.installing") : t("home.installPack")}
                </button>
              </div>
            ))}
          </div>
          {packInfo && (
            <p className="rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success">
              {packInfo}
            </p>
          )}
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
            void remove(target.id, wipe).catch((e) => setError(String(e)));
          }}
        />
      )}
    </div>
  );
}
