// Страница инстансов: список + создание/удаление (спека §8).
import { useState } from "react";
import { Plus } from "lucide-react";
import { useInstances } from "../../state/instances";
import { api } from "../../api/client";
import { useSettings } from "../../state/settings";
import { t } from "../../i18n";
import InstanceCard from "../components/InstanceCard";
import CreateInstanceDialog from "../components/CreateInstanceDialog";

export default function InstancesPage() {
  const { list, status, launch, remove, duplicate, error, setError } = useInstances();
  const { settings } = useSettings();
  const [creating, setCreating] = useState(false);

  return (
    <div>
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">{t("nav.instances")}</h1>
        <button
          onClick={() => setCreating(true)}
          className="flex items-center gap-2 rounded-md bg-accent px-4 py-2 text-sm font-semibold text-accent-contrast hover:bg-accent-hover"
        >
          <Plus size={16} aria-hidden />
          {t("instances.create")}
        </button>
      </div>

      {error && (
        <p className="mt-3 rounded-md border border-error/40 bg-error/10 p-3 text-sm text-error" role="alert">
          {error}
        </p>
      )}

      <div className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-4">
        {list.map((inst) => (
          <InstanceCard
            key={inst.id}
            inst={inst}
            status={status[inst.id]}
            onLaunch={() => void launch(inst.id, "Player").catch((e) => setError(String(e)))}
            onFolder={() => void api.instanceOpenDir(inst.id).catch((e) => setError(String(e)))}
            onDuplicate={() => void duplicate(inst.id).catch((e) => setError(String(e)))}
            onDelete={() => {
              if (window.confirm(t("instances.confirmDelete", { name: inst.name }))) {
                void remove(inst.id, false).catch((e) => setError(String(e)));
              }
            }}
          />
        ))}
      </div>

      {creating && (
        <CreateInstanceDialog
          showSnapshots={settings?.showSnapshots ?? false}
          showOld={settings?.showOldVersions ?? false}
          onClose={() => setCreating(false)}
          onCreated={() => setCreating(false)}
        />
      )}
    </div>
  );
}
