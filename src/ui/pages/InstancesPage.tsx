// Страница инстансов «Сланца»: строки + создание/удаление + фильтр поиска + управление.
import { useState } from "react";
import { Plus, SearchX, Inbox } from "lucide-react";
import { useInstances } from "../../state/instances";
import { useAccounts } from "../../state/accounts";
import { api } from "../../api/client";
import { useSettings } from "../../state/settings";
import { t } from "../../i18n";
import InstanceRow from "../components/InstanceRow";
import CreateInstanceDialog from "../components/CreateInstanceDialog";
import InstanceSettingsModal from "../components/InstanceSettingsModal";
import RenameInstanceModal from "../components/RenameInstanceModal";
import LogViewerModal from "../components/LogViewerModal";
import type { Instance } from "../../api/types";

export default function InstancesPage({ query }: { query: string }) {
  const { list, status, launch, stop, optimize, remove, duplicate, error, setError, load } = useInstances();
  const { list: accounts } = useAccounts();
  const { settings } = useSettings();
  const [creating, setCreating] = useState(false);
  const [settingsInst, setSettingsInst] = useState<Instance | null>(null);
  const [renameInst, setRenameInst] = useState<Instance | null>(null);
  const [logsInst, setLogsInst] = useState<Instance | null>(null);

  const activeAccount =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;

  const q = query.trim().toLowerCase();
  const visible = q ? list.filter((i) => i.name.toLowerCase().includes(q)) : list;

  const rowProps = (inst: Instance) => ({
    status: status[inst.id],
    onLaunch: () =>
      void launch(inst.id, activeAccount?.name ?? "Player").catch((e) => setError(String(e))),
    onStop: () => void stop(inst.id).catch((e) => setError(String(e))),
    onOptimize: () => void optimize(inst.id).catch((e) => setError(String(e))),
    onFolder: () => void api.instanceOpenDir(inst.id).catch((e) => setError(String(e))),
    onDuplicate: () => void duplicate(inst.id).catch((e) => setError(String(e))),
    onSettings: () => setSettingsInst(inst),
    onRename: () => setRenameInst(inst),
    onLogs: () => setLogsInst(inst),
    onDelete: () => {
      const wipe = window.confirm(
        `${t("instances.confirmDelete", { name: inst.name })}\n\nНажмите OK для удаления в корзину (или Отмена для отмены).`,
      );
      if (wipe) {
        void remove(inst.id, false).catch((e) => setError(String(e)));
      }
    },
  });

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
        <div className="flex flex-col gap-2">
          {visible.map((inst) => (
            <InstanceRow key={inst.id} inst={inst} {...rowProps(inst)} />
          ))}
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

      {logsInst && (
        <LogViewerModal
          instance={logsInst}
          onClose={() => setLogsInst(null)}
        />
      )}
    </div>
  );
}
