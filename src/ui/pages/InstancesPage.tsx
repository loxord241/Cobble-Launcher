// Страница инстансов «Сланца»: строки + создание/удаление + фильтр поиска + управление.
import { useMemo, useState } from "react";
import { Plus, SearchX, Inbox, Import } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { useInstances } from "../../state/instances";
import { useAccounts } from "../../state/accounts";
import { api } from "../../api/client";
import { useSettings } from "../../state/settings";
import { apiErrorText, currentLanguage, t } from "../../i18n";
import { pickArchivePath, pickImagePath } from "../components/pickFile";
import InstanceCard from "../components/InstanceCard";
import CreateInstanceDialog from "../components/CreateInstanceDialog";
import DeleteInstanceDialog from "../components/DeleteInstanceDialog";
import InstanceSettingsModal from "../components/InstanceSettingsModal";
import RenameInstanceModal from "../components/RenameInstanceModal";
import ChangeVersionModal from "../components/ChangeVersionModal";
import LogViewerModal from "../components/LogViewerModal";
import { rowClickOpens } from "./InstanceDetailPage";
import type { Instance } from "../../api/types";

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
  onChangeVersion: () => void;
  onLogs: () => void;
  onBackup: () => void;
  onExport: () => void;
  onForceUnlock: () => void;
  onRepair: () => void;
  onShortcut: () => void;
  onIconSet: () => void;
  onIconClear?: () => void;
};

export default function InstancesPage({
  query,
  onOpenInstance,
}: {
  query: string;
  /** Открыть страницу инстанса (клик по строке — не по кнопкам Play/Stop/кебаб). */
  onOpenInstance: (id: string) => void;
}) {
  const { list, status, launch, stop, optimize, remove, duplicate, error, setError, load } =
    useInstances(
      useShallow((s) => ({
        list: s.list,
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
  const { list: accounts, setActive } = useAccounts();
  const { settings } = useSettings();
  const [creating, setCreating] = useState(false);
  const [settingsInst, setSettingsInst] = useState<Instance | null>(null);
  const [renameInst, setRenameInst] = useState<Instance | null>(null);
  const [changeVersionInst, setChangeVersionInst] = useState<Instance | null>(null);
  const [logsInst, setLogsInst] = useState<Instance | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // D7: честный диалог удаления — «в корзину» или «навсегда».
  const [deleteInst, setDeleteInst] = useState<Instance | null>(null);
  // Порядок карточек; по умолчанию «по последнему запуску».
  const [sort, setSort] = useState<"recent" | "name" | "created">("recent");

  const activeAccount =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;

  const q = query.trim().toLowerCase();
  const filtered = q ? list.filter((i) => i.name.toLowerCase().includes(q)) : list;
  // Сортировка — поверх фильтра поиска; работаем на копии, чтобы не мутировать
  // список стора (Array#sort in-place). sort стабилен, поэтому равные ключи
  // не «прыгают» между рендерами.
  const visible = [...filtered];
  if (sort === "name") {
    // localeCompare с учётом активного языка интерфейса — корректная кириллица/диакритика.
    visible.sort((a, b) => a.name.localeCompare(b.name, currentLanguage()));
  } else if (sort === "created") {
    visible.sort((a, b) => b.createdAt - a.createdAt);
  } else {
    // recent: по lastPlayed по убыванию; у неоткрывавшихся lastPlayed нет —
    // они уходят в конец (0 меньше любого таймстампа) в исходном порядке.
    visible.sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0));
  }

  // DISC-A02: импорт архива модпака (.mrpack/.zip) тем же ядерным вызовом, что
  // и drop в Shell; отмена диалога — тихо, ошибка — через apiErrorText (B11).
  const importArchive = () => {
    void (async () => {
      try {
        const path = await pickArchivePath();
        if (!path) return; // отмена диалога
        const inst = await api.instanceImport(path);
        setNotice(t("instances.import.done", { name: inst.name, version: inst.mcVersion }));
        await load();
      } catch (e) {
        // D62: список перечитываем и при ошибке — при частичном/отложенном
        // успехе инстанс уже мог появиться, а откат ядра мог что-то удалить
        // или изменить; без load() это не видно до перезапуска лаунчера.
        setError(apiErrorText(e));
        void load();
      }
    })();
  };

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
        onChangeVersion: () => setChangeVersionInst(inst),
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
        onShortcut: () => {
          void api
            .instanceShortcut(inst.id)
            .then((r) => setNotice(t("instances.shortcut.done", { path: r.path })))
            .catch((e) => setError(apiErrorText(e)));
        },
        // F2: проверка целостности игровых файлов (движок группы инстанса).
        onRepair: () => {
          setNotice(t("instances.repair.started"));
          void api
            .instanceRepair(inst.id)
            .then((r) =>
              setNotice(
                t("instances.repair.done", {
                  checked: r.checked,
                  redownloaded: r.redownloaded,
                }),
              ),
            )
            .catch((e) => setError(apiErrorText(e)));
        },
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
    // D65: сетка GD-«библиотеки» — шире (3 плитки в ряд на ~1280); паддинги
    // даёт Shell (p-6). Компактный ряд управления сверху, ниже — плитки.
    <div className="mx-auto flex max-w-[1280px] flex-col gap-4">
      <div className="mt-1 flex flex-wrap items-center gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("nav.instances")}</h2>
        <span className="chip-mono">{list.length}</span>
        <button onClick={importArchive} className="btn-ghost btn-sm ml-auto">
          <Import size={16} aria-hidden />
          {t("instances.import")}
        </button>
        <button onClick={() => setCreating(true)} className="btn-ghost btn-sm">
          <Plus size={16} aria-hidden />
          {t("instances.create")}
        </button>
        <select
          value={sort}
          onChange={(e) => setSort(e.target.value as "recent" | "name" | "created")}
          aria-label={t("instances.sort.label")}
          className="h-8 rounded-lg border border-border-strong bg-card px-2.5 text-[13px]"
        >
          <option value="recent">{t("instances.sort.recent")}</option>
          <option value="name">{t("instances.sort.name")}</option>
          <option value="created">{t("instances.sort.created")}</option>
        </select>
      </div>

      {error && (
        <p className="anim-slide-down rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <div className="anim-slide-down rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success">
          {notice}
        </div>
      )}

      {list.length === 0 ? (
        <div className="mt-10 flex flex-col items-center gap-3 rounded-lg border border-border-app bg-card px-4 py-10 text-center">
          <Inbox size={32} aria-hidden className="text-text-muted" />
          <div className="text-[15px] font-semibold">{t("instances.empty.title")}</div>
          <div className="max-w-xs text-[13px] text-text-muted">{t("instances.empty.desc")}</div>
          <button onClick={() => setCreating(true)} className="btn-ghost btn-sm mt-1">
            <Plus size={16} aria-hidden />
            {t("instances.create")}
          </button>
        </div>
      ) : visible.length === 0 ? (
        <div className="mt-10 flex flex-col items-center gap-3 rounded-lg border border-border-app bg-card px-4 py-10 text-center">
          <SearchX size={32} aria-hidden className="text-text-muted" />
          <div className="text-[15px] font-semibold">{t("shell.search.noResults")}</div>
          <div className="max-w-xs text-[13px] text-text-muted">
            {t("shell.search.noResultsHint", { query })}
          </div>
        </div>
      ) : (
        // D65: плитки ~300px высотой (высота задаёт InstanceCard) — auto-fill
        // даёт ровно 3 колонки на ~1280 и остаётся адаптивным на худших ширинах.
        <div className="grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] gap-3">
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

      {creating && (
        <CreateInstanceDialog
          showSnapshots={settings?.showSnapshots ?? false}
          showOld={settings?.showOldVersions ?? false}
          onClose={() => setCreating(false)}
          onCreated={() => {
            setCreating(false);
            void load(); // после создания инстанс должен сразу появиться в списке
          }}
        />
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

      {changeVersionInst && (
        <ChangeVersionModal
          instance={changeVersionInst}
          onClose={() => setChangeVersionInst(null)}
          onDone={(version) => {
            setChangeVersionInst(null);
            setNotice(t("changeVersion.done", { version }));
            void load();
          }}
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

      {logsInst && (
        <LogViewerModal
          instance={logsInst}
          onClose={() => setLogsInst(null)}
        />
      )}
    </div>
  );
}
