// Главная «Сланца»: секция «Инстансы» — заголовок 18px + счётчик-чип + CTA.
// «Популярные модпаки» не рисуем: реальных данных популярного списка в UI нет.
import { useInstances } from "../../state/instances";
import { api } from "../../api/client";
import { t } from "../../i18n";
import InstanceRow from "../components/InstanceRow";
import { SearchX, Inbox, Plus } from "lucide-react";

export default function HomePage({
  query,
  onCreate,
}: {
  query: string;
  onCreate: () => void;
}) {
  const { list, status, launch, stop, optimize, remove, duplicate, setError } = useInstances();

  const q = query.trim().toLowerCase();
  const visible = q
    ? list.filter((i) => i.name.toLowerCase().includes(q))
    : list;

  const rowProps = (inst: (typeof list)[number]) => ({
    status: status[inst.id],
    onLaunch: () => void launch(inst.id, "Player").catch((e) => setError(String(e))),
    onStop: () => void stop(inst.id).catch((e) => setError(String(e))),
    onOptimize: () => void optimize(inst.id).catch((e) => setError(String(e))),
    onFolder: () => void api.instanceOpenDir(inst.id).catch((e) => setError(String(e))),
    onDuplicate: () => void duplicate(inst.id).catch((e) => setError(String(e))),
    onDelete: () => {
      if (window.confirm(t("instances.confirmDelete", { name: inst.name }))) {
        void remove(inst.id, false).catch((e) => setError(String(e)));
      }
    },
  });

  return (
    <div className="mx-auto flex max-w-[980px] flex-col gap-4">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("nav.instances")}</h2>
        <span className="chip-mono">{list.length}</span>
        <button
          onClick={onCreate}
          className="btn-ghost btn-sm ml-auto"
          aria-label={t("instances.newInstance")}
        >
          <Plus size={16} aria-hidden />
          {t("instances.newInstance")}
        </button>
      </div>

      {list.length === 0 ? (
        // Честное пустое состояние: инстансов нет вообще.
        <div className="mt-10 flex flex-col items-center gap-3 rounded-lg border border-border-app bg-card px-4 py-10 text-center">
          <Inbox size={32} aria-hidden className="text-text-muted" />
          <div className="text-[15px] font-semibold">{t("instances.empty.title")}</div>
          <div className="max-w-xs text-[13px] text-text-muted">{t("instances.empty.desc")}</div>
          <button onClick={onCreate} className="btn-ghost btn-sm mt-1">
            <Plus size={16} aria-hidden />
            {t("home.createFirst")}
          </button>
        </div>
      ) : visible.length === 0 ? (
        // Поиск не нашёл совпадений — говорим об этом прямо.
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
    </div>
  );
}
