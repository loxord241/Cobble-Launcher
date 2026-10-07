// Диалог создания инстанса: версия (манифест) + загрузчик (спека §8).
import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { api } from "../../api/client";
import { apiErrorText, t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import type { LoaderVersionEntry, VersionEntry } from "../../api/types";

const LOADERS = [
  { id: "none", label: "instances.loader.none" },
  { id: "fabric", label: "Fabric" },
  { id: "quilt", label: "Quilt" },
  { id: "forge", label: "Forge" },
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
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose, {
    initialFocus: "#ci-name",
  });

  useEffect(() => {
    api
      .manifestVersions(showSnapshots, showOld)
      .then((vs) => {
        setVersions(vs);
        setVersion(vs[0]?.id ?? "");
      })
      .catch((e) => setError(apiErrorText(e)));
  }, [showSnapshots, showOld]);

  // D64: гонка версий загрузчика — при быстрой смене loader'а приходит ответ
  // прошлого запроса; применяем только самый свежий (seq-guard).
  const loaderSeqRef = useRef(0);
  useEffect(() => {
    if (loader === "none") {
      setLoaderVersions([]);
      return;
    }
    const seq = ++loaderSeqRef.current;
    api
      .loaderVersions(loader)
      .then((ls) => {
        if (seq !== loaderSeqRef.current) return;
        setLoaderVersions(ls);
        setLoaderVersion(ls[0]?.version ?? "");
      })
      .catch(() => {
        if (seq !== loaderSeqRef.current) return;
        setLoaderVersions([]);
      });
  }, [loader]);

  const submit = async () => {
    setBusy(true);
    setError(null);
    // B7: если упало удаление осиротевшего инстанса — показываем и эту ошибку
    // тоже, рядом с основной (не прячем).
    let deleteError: string | null = null;
    try {
      const inst = await api.instanceCreate(version, name || version);
      if (loader !== "none") {
        try {
          await api.loaderInstall(inst.id, loader, loaderVersion || undefined);
        } catch (e) {
          // B7: падение установки загрузчика оставляет на диске ванильный
          // инстанс без загрузчика — удаляем его насовсем (wipe: true).
          try {
            await api.instanceDelete(inst.id, true);
          } catch (delErr) {
            deleteError = apiErrorText(delErr);
          }
          throw e;
        }
      }
      onCreated();
    } catch (e) {
      const main = apiErrorText(e);
      setError(deleteError ? `${main}\n${deleteError}` : main);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      {/* D41/F10: ref и anim-классы на карточке, не на оверлее — иначе
          .anim-closing схлопывает весь экран, а не только диалог. */}
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("instances.create")}
        className="anim-dialog-in w-[440px] rounded-lg border border-border-app bg-card p-5"
      >
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">{t("instances.create")}</h2>
          <button onClick={requestClose} aria-label={t("common.close")} className="text-text-muted hover:text-text">
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

        {error && (
          <div className="mt-3 text-sm text-error" role="alert">
            {/* Сначала человеческая подсказка, сырое сообщение ОС — ниже, тише. */}
            <p>{t("instances.create.errorHint")}</p>
            <p className="mt-1 whitespace-pre-line text-[12px] text-text-muted">{error}</p>
          </div>
        )}

        <div className="mt-5 flex justify-end gap-3">
          <button onClick={onClose} className="btn-ghost h-10 px-4 text-sm">
            {t("common.cancel")}
          </button>
          <button
            onClick={() => void submit()}
            disabled={busy || !version}
            className="btn-primary h-10 px-5 text-sm"
          >
            {t("instances.create")}
          </button>
        </div>
      </div>
    </div>
  );
}
