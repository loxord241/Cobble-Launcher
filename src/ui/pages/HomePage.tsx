// Главная: карточки инстансов + CTA (спека §8).
import { useInstances } from "../../state/instances";
import { api } from "../../api/client";
import { t } from "../../i18n";
import InstanceCard from "../components/InstanceCard";

export default function HomePage({ onCreate }: { onCreate: () => void }) {
  const { list, status, launch, remove, duplicate, setError } = useInstances();

  return (
    <div>
      <h1 className="text-xl font-semibold">{t("nav.home")}</h1>
      {list.length === 0 ? (
        <div className="mt-10 flex flex-col items-center gap-3 text-text-muted">
          <p className="text-lg">{t("home.noInstances")}</p>
          <button
            onClick={onCreate}
            className="rounded-md bg-accent px-5 py-2 font-semibold text-accent-contrast hover:bg-accent-hover"
          >
            {t("home.createFirst")}
          </button>
        </div>
      ) : (
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
      )}
    </div>
  );
}
