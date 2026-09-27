// Диалог создания инстанса: версия (манифест) + загрузчик (спека §8).
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { api } from "../../api/client";
import { t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import type { LoaderVersionEntry, VersionEntry } from "../../api/types";

const LOADERS = [
  { id: "none", label: "instances.loader.none" },
  { id: "fabric", label: "Fabric" },
  { id: "quilt", label: "Quilt" },
  { id: "neoforge", label: "NeoForge" },
];

export default function CreateInstanceDialog({
  showSnapshots,
  showOld,
  onClose,
  onCreated,
}: {
  showSnapshots: boolean;
  showOld: boolean;
  onClose: () => void;
  onCreated: () => void;
}) {
  const [versions, setVersions] = useState<VersionEntry[]>([]);
  const [loaderVersions, setLoaderVersions] = useState<LoaderVersionEntry[]>([]);
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [loader, setLoader] = useState("none");
  const [loaderVersion, setLoaderVersion] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // A33/A34: Escape, ловушка фокуса и возврат фокуса на триггер.
  const dialogRef = useModalA11y(onClose, { initialFocus: "#ci-name" });

  useEffect(() => {
    api
      .manifestVersions(showSnapshots, showOld)
      .then((vs) => {
        setVersions(vs);
        setVersion(vs[0]?.id ?? "");
      })
      .catch((e) => setError(e.message ?? String(e)));
  }, [showSnapshots, showOld]);

  useEffect(() => {
    if (loader === "none") {
      setLoaderVersions([]);
      return;
    }
    api
      .loaderVersions(loader)
      .then((ls) => {
        setLoaderVersions(ls);
        setLoaderVersion(ls[0]?.version ?? "");
      })
      .catch(() => setLoaderVersions([]));
  }, [loader]);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const inst = await api.instanceCreate(version, name || version);
      if (loader !== "none") {
        await api.loaderInstall(inst.id, loader, loaderVersion || undefined);
      }
      onCreated();
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      ref={dialogRef}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      role="dialog"
      aria-modal="true"
      aria-label={t("instances.create")}
      tabIndex={-1}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-[440px] rounded-lg border border-border-app bg-card p-5">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">{t("instances.create")}</h2>
          <button onClick={onClose} aria-label={t("common.close")} className="text-text-muted hover:text-text">
            <X size={18} aria-hidden />
          </button>
        </div>

        <label className="mt-4 block text-sm text-text-muted" htmlFor="ci-name">
          {t("instances.name")}
        </label>
        <input
          id="ci-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t("instances.newInstance")}
          className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 text-text outline-none focus:border-accent"
        />

        <label className="mt-3 block text-sm text-text-muted" htmlFor="ci-ver">
          {t("instances.version")}
        </label>
        <select
          id="ci-ver"
          value={version}
          onChange={(e) => setVersion(e.target.value)}
          className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 text-text outline-none focus:border-accent"
        >
          {versions.map((v) => (
            <option key={v.id} value={v.id}>
              {v.id} ({v.type})
            </option>
          ))}
        </select>

        <label className="mt-3 block text-sm text-text-muted" htmlFor="ci-loader">
          {t("instances.loader")}
        </label>
        <select
          id="ci-loader"
          value={loader}
          onChange={(e) => setLoader(e.target.value)}
          className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 text-text outline-none focus:border-accent"
        >
          {LOADERS.map((l) => (
            <option key={l.id} value={l.id}>
              {t(l.label)}
            </option>
          ))}
        </select>

        {loader !== "none" && (
          <>
            <label className="mt-3 block text-sm text-text-muted" htmlFor="ci-lv">
              {t("instances.loaderVersion", { loader })}
            </label>
            <select
              id="ci-lv"
              value={loaderVersion}
              onChange={(e) => setLoaderVersion(e.target.value)}
              className="mt-1 w-full rounded-md border border-border-app bg-bg px-3 py-2 text-text outline-none focus:border-accent"
            >
              {loaderVersions.map((l) => (
                <option key={l.version} value={l.version}>
                  {l.version}
                </option>
              ))}
            </select>
          </>
        )}

        {error && <p className="mt-3 text-sm text-error" role="alert">{error}</p>}

        <button
          onClick={() => void submit()}
          disabled={busy || !version}
          className="btn-primary mt-5 w-full"
        >
          {t("instances.create")}
        </button>
      </div>
    </div>
  );
}
