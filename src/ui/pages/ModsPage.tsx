// Браузер контента Modrinth (спека §8): поиск, установка в инстанс,
// установка .mrpack, проверка/применение обновлений.
import { useEffect, useState } from "react";
import { Download, RefreshCw, Search } from "lucide-react";
import { api } from "../../api/client";
import { t } from "../../i18n";
import type { Instance, SearchResult, UpdateCheck } from "../../api/types";

export default function ModsPage() {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SearchResult | null>(null);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [targetId, setTargetId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [installedInfo, setInstalledInfo] = useState<string | null>(null);
  const [updates, setUpdates] = useState<UpdateCheck[] | null>(null);

  useEffect(() => {
    api.instanceList().then((list) => {
      setInstances(list);
      if (list[0]) setTargetId(list[0].id);
    });
  }, []);

  const doSearch = async () => {
    setBusy(true);
    setError(null);
    try {
      const target = instances.find((i) => i.id === targetId);
      setResult(await api.modrinthSearch(query || "mods", target?.mcVersion, target?.loader));
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  const install = async (projectId: string) => {
    setBusy(true);
    setError(null);
    setInstalledInfo(null);
    try {
      const entries = await api.contentInstall(targetId, projectId);
      setInstalledInfo(
        `Установлено в «${instances.find((i) => i.id === targetId)?.name ?? "?"}»: ${entries
          .map((e) => e.file)
          .join(", ")}`,
      );
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  const installMrpack = async () => {
    const path = window.prompt("Путь к .mrpack файлу:");
    if (!path) return;
    setBusy(true);
    setError(null);
    try {
      const inst = await api.mrpackInstall(path);
      setInstalledInfo(`Модпак установлен: ${inst.name} [${inst.mcVersion}]`);
      setInstances(await api.instanceList());
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  const checkUpdates = async () => {
    setBusy(true);
    setError(null);
    try {
      setUpdates(await api.contentUpdateCheck(targetId));
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  const applyUpdates = async () => {
    setBusy(true);
    try {
      const n = await api.contentUpdateAll(targetId);
      setInstalledInfo(`Обновлено модов: ${n}`);
      setUpdates(null);
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <h1 className="text-xl font-semibold">{t("nav.mods")}</h1>

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <div className="flex min-w-72 flex-1 items-center gap-2 rounded-md border border-border-app bg-card px-3">
          <Search size={16} aria-hidden className="text-text-muted" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void doSearch()}
            placeholder={t("mods.searchPlaceholder")}
            className="w-full bg-transparent py-2 outline-none"
            aria-label={t("mods.searchPlaceholder")}
          />
        </div>
        <select
          value={targetId}
          onChange={(e) => {
            setTargetId(e.target.value);
            setUpdates(null);
          }}
          aria-label={t("mods.targetInstance")}
          className="rounded-md border border-border-app bg-card px-3 py-2"
        >
          {instances.map((i) => (
            <option key={i.id} value={i.id}>
              {i.name} [{i.mcVersion}]
            </option>
          ))}
        </select>
        <button
          onClick={() => void doSearch()}
          disabled={busy}
          className="flex items-center gap-2 rounded-md bg-accent px-4 py-2 text-sm font-semibold text-accent-contrast hover:bg-accent-hover disabled:opacity-40"
        >
          <Search size={15} aria-hidden />
          {t("mods.search")}
        </button>
        <button
          onClick={() => void installMrpack()}
          disabled={busy}
          className="rounded-md border border-border-app px-3 py-2 text-sm hover:bg-card"
        >
          {t("mods.installMrpack")}
        </button>
        <button
          onClick={() => void checkUpdates()}
          disabled={busy || !targetId}
          className="flex items-center gap-2 rounded-md border border-border-app px-3 py-2 text-sm hover:bg-card disabled:opacity-40"
        >
          <RefreshCw size={14} aria-hidden />
          {t("mods.checkUpdates")}
        </button>
      </div>

      {error && (
        <p className="mt-3 rounded-md border border-error/40 bg-error/10 p-3 text-sm text-error" role="alert">
          {error}
        </p>
      )}
      {installedInfo && (
        <p className="mt-3 rounded-md border border-success/40 bg-success/10 p-3 text-sm text-success">
          {installedInfo}
        </p>
      )}

      {updates !== null && (
        <div className="mt-3 rounded-md border border-border-app bg-card p-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold">
              {t("mods.updatesFound", { n: updates.length })}
            </span>
            {updates.length > 0 && (
              <button
                onClick={() => void applyUpdates()}
                className="flex items-center gap-2 rounded-md bg-accent px-3 py-1.5 text-sm font-semibold text-accent-contrast hover:bg-accent-hover"
              >
                <RefreshCw size={14} aria-hidden />
                {t("mods.applyUpdates")}
              </button>
            )}
          </div>
          <ul className="mt-2 space-y-1 text-sm text-text-muted">
            {updates.map((u) => (
              <li key={u.file}>
                {u.file}: {u.latestVersionNumber}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4">
        {(result?.hits ?? []).map((h) => (
          <div key={h.project_id} className="rounded-lg border border-border-app bg-card p-4">
            <div className="flex items-start gap-3">
              {h.icon_url ? (
                <img src={h.icon_url} alt="" className="h-12 w-12 rounded-md object-cover" />
              ) : (
                <div className="flex h-12 w-12 items-center justify-center rounded-md bg-bg text-lg font-bold text-accent">
                  {h.title.slice(0, 1)}
                </div>
              )}
              <div className="min-w-0">
                <h3 className="truncate font-semibold">{h.title}</h3>
                <p className="text-xs text-text-muted">
                  {h.author} · {h.downloads.toLocaleString("ru")} ⬇
                </p>
              </div>
            </div>
            <p className="mt-2 line-clamp-2 min-h-8 text-sm text-text-muted">{h.description}</p>
            <button
              onClick={() => void install(h.project_id)}
              disabled={busy || !targetId}
              className="mt-3 flex w-full items-center justify-center gap-2 rounded-md bg-accent py-2 text-sm font-semibold text-accent-contrast hover:bg-accent-hover disabled:opacity-40"
            >
              <Download size={14} aria-hidden />
              {t("mods.installInto")}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
