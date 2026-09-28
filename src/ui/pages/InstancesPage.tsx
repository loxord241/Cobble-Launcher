// Страница инстансов «Сланца»: строки + создание/удаление + фильтр поиска + управление.
import { useMemo, useState } from "react";
import { Plus, SearchX, Inbox } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { useInstances } from "../../state/instances";
import { useAccounts } from "../../state/accounts";
import { api } from "../../api/client";
import { useSettings } from "../../state/settings";
import { apiErrorText, t } from "../../i18n";
import { pickImagePath } from "../components/pickFile";
import InstanceCard from "../components/InstanceCard";
import CreateInstanceDialog from "../components/CreateInstanceDialog";
import DeleteInstanceDialog from "../components/DeleteInstanceDialog";
import InstanceSettingsModal from "../components/InstanceSettingsModal";
import RenameInstanceModal from "../components/RenameInstanceModal";
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
  onLogs: () => void;
  onBackup: () => void;
  onExport: () => void;
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
  const { list: accounts } = useAccounts();
  const { settings } = useSettings();
  const [creating, setCreating] = useState(false);
  const [settingsInst, setSettingsInst] = useState<Instance | null>(null);
  const [renameInst, setRenameInst] = useState<Instance | null>(null);
  const [logsInst, setLogsInst] = useState<Instance | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // D7: честный диалог удаления — «в корзину» или «навсегда».
  const [deleteInst, setDeleteInst] = useState<Instance | null>(null);

  const activeAccount =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;

  const q = query.trim().toLowerCase();
  const visible = q ? list.filter((i) => i.name.toLowerCase().includes(q)) : list;

  // D8: колбэки строк строятся один раз на список (замыкание на inst), а не
  // заново на каждый рендер — иначе memo(InstanceCard) не срабатывает.
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
    <div className="mx-auto flex max-w-[980px] flex-col gap-4">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("nav.instances")}</h2>
        <span className="chip-mono">{list.length}</span>
        <button onClick={() => setCreating(true)} className="btn-ghost btn-sm ml-auto">
          <Plus size={16} aria-hidden />
          {t("instances.create")}
        </button>
      </div>

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

      {creating && (
        <CreateInstanceDialog
          showSnapshots={settings?.showSnapshots ?? false}
          showOld={settings?.showOldVersions ?? false}
          onClose={() => setCreating(false)}
          onCreated={() => setCreating(false)}
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

      {logsInst && (
        <LogViewerModal
          instance={logsInst}
          onClose={() => setLogsInst(null)}
        />
      )}
    </div>
  );
}
