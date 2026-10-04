// Страница «Скины» (D59): слева большой превью персонажа с вращением,
// справа — «Мои скины» (загрузка PNG, удаление) и «Библиотека» (Steve/Alex
// из клиент-jar, D54). Применение — к активному аккаунту, честно по его типу:
// msa → Mojang API, ely/authlib → Ely.by API, offline → честный отказ с
// подсказкой про бесплатный Ely.by (никаких инжекций, D59).
import { useEffect, useState } from "react";
import { LoaderCircle, Trash2, Upload, UserPlus } from "lucide-react";
import { api } from "../../api/client";
import type { SkinInfo, SkinLibraryItem, SkinModel, SkinUserItem } from "../../api/types";
import { apiErrorText, t } from "../../i18n";
import { accountKindLabelKey, useAccounts } from "../../state/accounts";
import { useSettings } from "../../state/settings";
import { useSkins } from "../../state/skins";
import { askConfirm } from "../confirm";
import AccountsModal from "../components/AccountsModal";
import { pickPngPath } from "../components/pickFile";
import { FullSkinPreview, SkinHead } from "../components/SkinViews";

/** Что сейчас выбрано под превью и кнопку «Применить». */
type Selection =
  | { kind: "library"; item: SkinLibraryItem }
  | { kind: "user"; item: SkinUserItem };

type AccountNote = "none" | "offline" | "msa" | "ely";

/** Локализованные подписи библиотеки (id задаёт ядро, D59). */
const LIBRARY_LABEL_KEYS: Record<string, string> = {
  "steve-classic": "skins.name.steveClassic",
  "steve-slim": "skins.name.steveSlim",
  "alex-classic": "skins.name.alexClassic",
  "alex-slim": "skins.name.alexSlim",
};

function skinLabel(item: { id?: string; name?: string }): string {
  if (item.name) return item.name;
  const key = item.id ? LIBRARY_LABEL_KEYS[item.id] : undefined;
  return key ? t(key) : (item.id ?? "");
}

/** PNG-скин для превью: dataUrl есть — рисуем его, нет — честный дефолт (D34). */
function previewOf(item: { dataUrl?: string }): SkinInfo | null {
  return item.dataUrl ? { source: "default", dataUrl: item.dataUrl } : null;
}

export default function SkinsPage() {
  const { list: accounts } = useAccounts();
  const { settings } = useSettings();
  // Активный профиль — по тому же правилу, что чип титлбара (Shell).
  const active =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;

  const skinsById = useSkins((s) => s.byId);

  const [library, setLibrary] = useState<SkinLibraryItem[]>([]);
  const [userSkins, setUserSkins] = useState<SkinUserItem[]>([]);
  const [selected, setSelected] = useState<Selection | null>(null);
  // Модель для скинов без фиксированной (upload/часть библиотеки).
  const [modelChoice, setModelChoice] = useState<SkinModel>("classic");
  const [uploadBusy, setUploadBusy] = useState(false);
  const [applyBusy, setApplyBusy] = useState(false);
  const [applied, setApplied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [accountsOpen, setAccountsOpen] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .skinLibraryList()
      .then((items) => {
        if (alive) setLibrary(items);
      })
      .catch((e) => {
        if (alive) setError(apiErrorText(e));
      });
    api
      .skinUserList()
      .then((items) => {
        if (alive) setUserSkins(items);
      })
      .catch((e) => {
        if (alive) setError(apiErrorText(e));
      });
    return () => {
      alive = false;
    };
  }, []);

  /** Загрузить свой PNG: путь из системного диалога уходит ядру как есть. */
  const uploadSkin = async () => {
    const path = await pickPngPath();
    if (!path) return;
    setUploadBusy(true);
    setError(null);
    try {
      const base = path.split(/[\\/]/).pop() ?? "skin";
      const name = base.replace(/\.png$/i, "") || base;
      const saved = await api.skinUserSave(path, name);
      setUserSkins((list) => [...list.filter((s) => s.id !== saved.id), saved]);
      setSelected({ kind: "user", item: saved });
      setApplied(false);
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setUploadBusy(false);
    }
  };

  const deleteSkin = async (item: SkinUserItem) => {
    if (!item.id) return;
    const label = skinLabel(item);
    const yes = await askConfirm(t("skins.delete.confirm", { name: label }), {
      danger: true,
      confirmLabel: t("common.delete"),
      detail: t("skins.delete.detail"),
    });
    if (!yes) return;
    try {
      await api.skinUserDelete(item.id);
      setUserSkins((list) => list.filter((s) => s.id !== item.id));
      if (selected?.kind === "user" && selected.item.id === item.id) setSelected(null);
    } catch (e) {
      setError(apiErrorText(e));
    }
  };

  const effectiveModel: SkinModel = selected?.item.model ?? modelChoice;
  // Применение через лаунчер реально работает только для MSA (API Mojang —
  // оживает после одобрения заявки, D44). Ely.by: честно отправляем на сайт
  // (их account-API требует отдельного OAuth-токена с multipart, D59);
  // выбранный там скин лаунчер и так показывает через account_skin (D34).
  const canApply = active?.kind === "msa" && selected !== null;

  const applySkin = async () => {
    if (!active || !selected || !selected.item.id) return;
    setApplyBusy(true);
    setError(null);
    setApplied(false);
    try {
      await api.skinApply(active.id, selected.item.id, effectiveModel);
      setApplied(true);
      // Скин применили себе — переспрашиваем текстуру, чип и превью обновятся.
      void useSkins.getState().refresh(active).catch(() => undefined);
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setApplyBusy(false);
    }
  };

  const select = (sel: Selection) => {
    setSelected(sel);
    setApplied(false);
  };

  // Превью: выбранная карточка, иначе — текущий скин активного профиля.
  const previewSkin: SkinInfo | null = selected
    ? previewOf(selected.item)
    : active
      ? (skinsById[active.id] ?? null)
      : null;
  const previewName = active?.name ?? "Steve";
  const note: AccountNote = !active
    ? "none"
    : active.kind === "offline"
      ? "offline"
      : active.kind === "msa"
        ? "msa"
        : "ely"; // ely и authlib (D59: оба применяются через API Ely.by)

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
      <h1 className="text-xl font-semibold text-text">{t("skins.title")}</h1>

      {error && (
        <p role="alert" className="rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error">
          {error}
        </p>
      )}

      <div className="grid items-start gap-5 lg:grid-cols-[300px_minmax(0,1fr)]">
        {/* Левая колонка: большой превью персонажа (вращение перетаскиванием). */}
        <section
          aria-label={t("skins.title")}
          className="rounded-lg border border-border-app bg-card p-4"
        >
          <FullSkinPreview
            name={previewName}
            skin={previewSkin}
            model={effectiveModel}
            scale={8}
            className="w-full"
          />
          {active && (
            <div className="mt-3 text-center">
              <div className="truncate text-sm font-semibold text-text">{active.name}</div>
              <div className="text-xs text-text-muted">{t(accountKindLabelKey[active.kind])}</div>
            </div>
          )}
        </section>

        <div className="flex min-w-0 flex-col gap-5">
          {/* Мои скины: загрузка PNG + карточки с удалением. */}
          <section aria-label={t("skins.mySkins")} className="rounded-lg border border-border-app bg-card p-4">
            <div className="flex items-center justify-between gap-2">
              <h2 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
                {t("skins.mySkins")}
              </h2>
              <button
                type="button"
                onClick={() => void uploadSkin()}
                disabled={uploadBusy}
                aria-busy={uploadBusy}
                className="btn-primary btn-sm flex items-center gap-2"
              >
                {uploadBusy ? (
                  <LoaderCircle size={14} aria-hidden className="animate-spin" />
                ) : (
                  <Upload size={14} aria-hidden />
                )}
                {uploadBusy ? t("skins.uploading") : t("skins.upload")}
              </button>
            </div>
            {userSkins.length === 0 ? (
              <p className="mt-3 text-sm text-text-muted">{t("skins.mySkinsEmpty")}</p>
            ) : (
              <div className="mt-3 grid grid-cols-[repeat(auto-fill,minmax(88px,1fr))] gap-2">
                {userSkins.map((item) => (
                  <SkinCard
                    key={item.id}
                    label={skinLabel(item)}
                    dataUrl={item.dataUrl}
                    selected={selected?.kind === "user" && selected.item.id === item.id}
                    onSelect={() => select({ kind: "user", item })}
                    onDelete={() => void deleteSkin(item)}
                    deleteLabel={t("skins.delete", { name: skinLabel(item) })}
                  />
                ))}
              </div>
            )}
          </section>

          {/* Библиотека: дефолтные скины из клиент-jar (Steve/Alex). */}
          <section aria-label={t("skins.library")} className="rounded-lg border border-border-app bg-card p-4">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
              {t("skins.library")}
            </h2>
            {library.length === 0 ? (
              <p className="mt-3 text-sm text-text-muted">{t("skins.libraryEmpty")}</p>
            ) : (
              <div className="mt-3 grid grid-cols-[repeat(auto-fill,minmax(88px,1fr))] gap-2">
                {library.map((item) => (
                  <SkinCard
                    key={item.id}
                    label={skinLabel(item)}
                    dataUrl={item.dataUrl}
                    selected={selected?.kind === "library" && selected.item.id === item.id}
                    onSelect={() => select({ kind: "library", item })}
                  />
                ))}
              </div>
            )}
          </section>

          {/* Применение: честные состояния по типу активного аккаунта. */}
          <section aria-label={t("skins.apply")} className="rounded-lg border border-border-app bg-card p-4">
            <div className="flex flex-wrap items-center justify-end gap-3">
              {selected && !selected.item.model && (
                <label className="flex items-center gap-2 text-sm text-text-muted">
                  {t("skins.model")}
                  <select
                    value={modelChoice}
                    onChange={(e) => setModelChoice(e.target.value as SkinModel)}
                    className="h-8 rounded-md border border-border-strong bg-card px-2 text-sm text-text"
                  >
                    <option value="classic">{t("skins.model.classic")}</option>
                    <option value="slim">{t("skins.model.slim")}</option>
                  </select>
                </label>
              )}
              <button
                type="button"
                onClick={() => void applySkin()}
                disabled={!canApply || applyBusy}
                aria-busy={applyBusy}
                className="btn-primary flex h-10 items-center gap-2 px-5 text-sm"
              >
                {applyBusy && <LoaderCircle size={15} aria-hidden className="animate-spin" />}
                {applyBusy ? t("skins.applying") : t("skins.apply")}
              </button>
            </div>

            {note === "none" && (
              <div className="mt-3">
                <p className="text-xs leading-relaxed text-text-muted">{t("skins.noAccount")}</p>
                <button
                  type="button"
                  onClick={() => setAccountsOpen(true)}
                  className="btn-ghost btn-sm mt-2 flex items-center gap-2"
                >
                  <UserPlus size={14} aria-hidden />
                  {t("shell.account.manage")}
                </button>
              </div>
            )}
            {note === "offline" && (
              <div className="mt-3">
                <p className="text-xs leading-relaxed text-text-muted">{t("skins.offline.note")}</p>
                <button
                  type="button"
                  onClick={() => setAccountsOpen(true)}
                  className="btn-ghost btn-sm mt-2 flex items-center gap-2"
                >
                  <UserPlus size={14} aria-hidden />
                  {t("skins.offline.addEly")}
                </button>
              </div>
            )}
            {note === "msa" && (
              <p className="mt-3 text-xs leading-relaxed text-text-muted">{t("skins.msa.note")}</p>
            )}
            {note === "ely" && (
              <p className="mt-3 text-xs leading-relaxed text-text-muted">{t("skins.ely.note")}</p>
            )}
            {applied && (
              <p role="status" className="mt-3 rounded-lg border border-success/40 bg-success/10 p-2 text-xs text-success">
                {t("skins.applied")}
              </p>
            )}
          </section>
        </div>
      </div>

      {accountsOpen && <AccountsModal onClose={() => setAccountsOpen(false)} />}
    </div>
  );
}

/** Карточка скина: мини-превью головы CSS-кропом (как в чипе титлбара). */
function SkinCard({
  label,
  dataUrl,
  selected,
  onSelect,
  onDelete,
  deleteLabel,
}: {
  label: string;
  dataUrl?: string;
  selected: boolean;
  onSelect: () => void;
  onDelete?: () => void;
  deleteLabel?: string;
}) {
  return (
    <div
      className={`group relative rounded-lg border p-2 ${
        selected
          ? "border-accent bg-accent-soft/30"
          : "border-border-app bg-bg hover:border-border-strong"
      }`}
    >
      <button
        type="button"
        onClick={onSelect}
        aria-pressed={selected}
        className="flex w-full flex-col items-center gap-1.5"
      >
        <SkinHead name={label} skin={previewOf({ dataUrl })} size={40} />
        <span className="w-full truncate text-center text-xs text-text" title={label}>
          {label}
        </span>
      </button>
      {onDelete && (
        <button
          type="button"
          onClick={onDelete}
          aria-label={deleteLabel}
          title={deleteLabel}
          className="absolute right-1 top-1 grid size-6 place-items-center rounded-md text-text-muted opacity-0 transition-opacity hover:bg-error/10 hover:text-error focus-visible:opacity-100 group-hover:opacity-100"
        >
          <Trash2 size={13} aria-hidden />
        </button>
      )}
    </div>
  );
}
