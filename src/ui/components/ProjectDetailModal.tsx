// Окно проекта Modrinth (D39, стиль CurseForge): шапка с установкой, чипы
// категорий, статистика и табы «Обзор / Журнал / Галерея / Версии». Тело
// проекта грузим при открытии, версии — лениво при первом открытии табов
// «Журнал»/«Версии» (кэш в состоянии на всё время жизни модалки).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clock, Download, HardDrive, Heart, LoaderCircle, Scale, X } from "lucide-react";
import { api } from "../../api/client";
import { apiErrorText, currentLanguage, t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import { Markdown } from "./markdown";
import type {
  GalleryImage,
  Instance,
  ModrinthVersion,
  ProjectDetail,
  SearchHit,
} from "../../api/types";

type Tab = "overview" | "changelog" | "gallery" | "versions";

const TABS: Tab[] = ["overview", "changelog", "gallery", "versions"];

// Человеческий размер файла (как fmtBytes в LogViewerModal).
function fmtBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} ГБ`;
  if (mb >= 1) return `${Math.round(mb)} МБ`;
  return `${Math.round(bytes / 1024)} КБ`;
}

/** Относительная дата («5 дней назад») для чипа «Обновлено»; пусто — не дата. */
function relWhen(iso: string): string {
  const then = new Date(iso).getTime();
  if (!Number.isFinite(then)) return "";
  const rtf = new Intl.RelativeTimeFormat(currentLanguage(), { numeric: "auto" });
  const days = Math.round((then - Date.now()) / 86_400_000);
  if (Math.abs(days) < 30) return rtf.format(days, "day");
  if (Math.abs(days) < 365) return rtf.format(Math.round(days / 30), "month");
  return rtf.format(Math.round(days / 365), "year");
}

// Бейдж типа версии; тип не задан — бейджа нет (никакого фейка).
function typeBadge(versionType?: string): { key: string; cls: string } | null {
  if (versionType === "release") return { key: "project.type.release", cls: "badge-soft" };
  if (versionType === "beta") return { key: "project.type.beta", cls: "badge-soft opacity-70" };
  if (versionType === "alpha") return { key: "project.type.alpha", cls: "badge-soft opacity-70" };
  return null;
}

export default function ProjectDetailModal({
  hit,
  onClose,
  onInstalled,
}: {
  hit: SearchHit;
  onClose: () => void;
  onInstalled?: () => void;
}) {
  const [tab, setTab] = useState<Tab>("overview");
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [detailErr, setDetailErr] = useState<string | null>(null);
  const [detailBusy, setDetailBusy] = useState(true);
  const [versions, setVersions] = useState<ModrinthVersion[] | null>(null);
  const [versionsErr, setVersionsErr] = useState<string | null>(null);
  const [versionsBusy, setVersionsBusy] = useState(false);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [targetId, setTargetId] = useState("");
  const [busy, setBusy] = useState(false);
  const [banner, setBanner] = useState<{ ok: boolean; text: string } | null>(null);
  const [zoom, setZoom] = useState<GalleryImage | null>(null);

  const isPack = hit.project_type === "modpack";
  const isMod = hit.project_type === "mod";

  // Escape при открытом зуме закрывает только зум; реф — потому что хук
  // ставит keydown-слушатель один раз на всё время жизни модалки.
  const zoomRef = useRef(zoom);
  zoomRef.current = zoom;
  const dialogRef = useModalA11y(() => (zoomRef.current ? setZoom(null) : onClose()));

  const loadDetail = useCallback(async () => {
    setDetailBusy(true);
    setDetailErr(null);
    try {
      setDetail(await api.modrinthProject(hit.project_id));
    } catch (e) {
      setDetailErr(apiErrorText(e));
    } finally {
      setDetailBusy(false);
    }
  }, [hit.project_id]);

  const loadVersions = useCallback(async () => {
    setVersionsBusy(true);
    setVersionsErr(null);
    try {
      setVersions(await api.modrinthVersions(hit.project_id));
    } catch (e) {
      setVersionsErr(apiErrorText(e));
    } finally {
      setVersionsBusy(false);
    }
  }, [hit.project_id]);

  // Повторно табы не перечитываем: ошибка грузится только по кнопке «Повторить».
  const ensureVersions = useCallback(() => {
    if (versions === null && versionsErr === null) void loadVersions();
  }, [versions, versionsErr, loadVersions]);

  useEffect(() => {
    void loadDetail();
    // Инстансы могли измениться, пока окно было закрыто, — перечитываем.
    api
      .instanceList()
      .then((list) => {
        setInstances(list);
        setTargetId((cur) => (list.some((x) => x.id === cur) ? cur : (list[0]?.id ?? "")));
      })
      .catch((e: unknown) => setBanner({ ok: false, text: apiErrorText(e) }));
  }, [loadDetail]);

  useEffect(() => {
    if (tab === "changelog" || tab === "versions") ensureVersions();
  }, [tab, ensureVersions]);

  const install = async (versionId?: string) => {
    setBusy(true);
    setBanner(null);
    try {
      if (isPack) {
        // Модпак всегда ставится в НОВЫЙ инстанс (само ядро).
        const inst = await api.modpackInstall(hit.project_id);
        setBanner({ ok: true, text: t("project.installDone", { name: inst.name }) });
      } else {
        await api.contentInstall(targetId, hit.project_id, versionId);
        const name = instances.find((i) => i.id === targetId)?.name ?? "?";
        setBanner({ ok: true, text: t("project.installDone", { name }) });
      }
      onInstalled?.();
    } catch (e) {
      setBanner({ ok: false, text: apiErrorText(e) });
    } finally {
      setBusy(false);
    }
  };

  // Размер последней primary-версии (Modrinth отдаёт новые первыми); пока
  // версии не загружены — чипа нет.
  const packSize = useMemo(() => {
    if (!versions) return null;
    for (const v of versions) {
      const f = v.files.find((x) => x.primary) ?? v.files[0];
      if (f) return f.size;
    }
    return null;
  }, [versions]);

  const when = relWhen(detail?.dateModified ?? hit.date_modified);
  const cats = detail?.categories ?? hit.categories;
  const cover = detail ? (detail.gallery.find((g) => g.featured) ?? detail.gallery[0]) : undefined;
  const changelogList = versions?.filter((v) => v.changelog && v.changelog.trim()) ?? [];

  // Общий вид «не загрузилось + повторить» для страницы и для версий.
  const failBlock = (err: string | null, retry: () => void, retryBusy: boolean) => (
    <div className="flex flex-col items-start gap-2 py-8">
      <p className="text-sm text-text-muted">{t("project.loadFailed")}</p>
      {err && <p className="max-w-md text-[13px] text-error">{err}</p>}
      <button onClick={retry} disabled={retryBusy} className="btn-ghost btn-sm">
        {t("project.retry")}
      </button>
    </div>
  );
  const loadingBlock = (
    <p className="flex items-center gap-2 py-8 text-sm text-text-muted">
      <LoaderCircle size={14} aria-hidden className="animate-spin" />
      {t("project.loading")}
    </p>
  );

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={hit.title}
        className="flex max-h-[85vh] w-full max-w-3xl flex-col rounded-lg border border-border-app bg-card shadow-2xl"
      >
        {/* Шапка: иконка, название/автор/описание, инстанс + установка. */}
        <div className="flex items-start gap-3 border-b border-border-app p-5">
          {hit.icon_url ? (
            <img
              src={hit.icon_url}
              alt=""
              className="size-16 shrink-0 rounded-lg border border-border-app object-cover"
            />
          ) : (
            <span
              aria-hidden
              className="grid size-16 shrink-0 place-items-center rounded-lg bg-surface-2 text-2xl font-semibold text-text-muted"
            >
              {hit.title.slice(0, 1)}
            </span>
          )}
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-xl font-bold leading-tight">{hit.title}</h2>
            <div className="mt-0.5 text-[13px] text-text-muted">
              {t("mods.by", { author: hit.author })}
            </div>
            {hit.description && (
              <p className="mt-1 line-clamp-2 text-[13px] leading-snug text-text-muted">
                {hit.description}
              </p>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {!isPack && (
              <select
                value={targetId}
                onChange={(e) => setTargetId(e.target.value)}
                disabled={busy || instances.length === 0}
                aria-label={t("project.chooseInstance")}
                className="h-9 max-w-44 rounded-lg border border-border-strong bg-card px-2 text-sm disabled:opacity-40"
              >
                {instances.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name} [{i.mcVersion}]
                  </option>
                ))}
              </select>
            )}
            <button
              onClick={() => void install()}
              disabled={busy || (!isPack && !targetId)}
              aria-busy={busy}
              className="btn-primary h-9"
            >
              {busy ? (
                <LoaderCircle size={15} aria-hidden className="animate-spin" />
              ) : (
                <Download size={15} aria-hidden />
              )}
              {busy ? t("project.installing") : t("project.install")}
            </button>
            <button
              onClick={onClose}
              aria-label={t("common.close")}
              className="icon-btn text-text-muted hover:text-text"
            >
              <X size={18} aria-hidden />
            </button>
          </div>
        </div>

        {/* Баннер установки: успех (зелёный) / ошибка (красный), внутри модалки. */}
        {banner && (
          <div className="px-5 pt-3">
            <p
              role={banner.ok ? "status" : "alert"}
              className={`rounded-lg border p-3 text-sm ${
                banner.ok
                  ? "border-success/40 bg-success/10 text-success"
                  : "border-error/40 bg-error/10 text-error"
              }`}
            >
              {banner.text}
            </p>
          </div>
        )}

        <div className="flex-1 overflow-y-auto px-5 pb-5">
          {/* Чипы: категории (первые 4 + «+N») и загрузчики отдельно. */}
          {(cats.length > 0 || (detail?.loaders.length ?? 0) > 0) && (
            <div className="mt-3 flex flex-wrap items-center gap-1.5">
              {cats.slice(0, 4).map((c) => (
                <span key={c} className="badge-soft accent">
                  {c}
                </span>
              ))}
              {cats.length > 4 && <span className="badge-soft">+{cats.length - 4}</span>}
              {detail?.loaders.map((l) => (
                <span key={l} className="badge-soft">
                  {l}
                </span>
              ))}
            </div>
          )}

          {/* Статистика; чип рисуем только когда данные есть. */}
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <span className="chip-mono flex items-center gap-1 text-xs">
              <Download size={12} aria-hidden />
              {t("mods.downloads", {
                n: (detail?.downloads ?? hit.downloads).toLocaleString(currentLanguage()),
              })}
            </span>
            {detail && (
              <span className="chip-mono flex items-center gap-1 text-xs">
                <Heart size={12} aria-hidden />
                {t("project.follows", { n: detail.follows.toLocaleString(currentLanguage()) })}
              </span>
            )}
            {when && (
              <span className="chip-mono flex items-center gap-1 text-xs">
                <Clock size={12} aria-hidden />
                {t("project.updated", { when })}
              </span>
            )}
            {packSize !== null && (
              <span className="chip-mono flex items-center gap-1 text-xs">
                <HardDrive size={12} aria-hidden />
                {fmtBytes(packSize)}
              </span>
            )}
            {detail?.license && (
              <span className="chip-mono flex items-center gap-1 text-xs">
                <Scale size={12} aria-hidden />
                {t("project.license", { name: detail.license })}
              </span>
            )}
          </div>

          {/* Табы — стиль вкладок ModsPage. */}
          <div role="tablist" className="mt-3 flex gap-1">
            {TABS.map((x) => (
              <button
                key={x}
                role="tab"
                aria-selected={tab === x}
                onClick={() => setTab(x)}
                className={`h-9 rounded-lg px-3 text-sm font-medium transition-colors ${
                  tab === x
                    ? "bg-accent text-on-accent"
                    : "text-text-muted hover:bg-surface-2 hover:text-text"
                }`}
              >
                {t(`project.tab.${x}`)}
              </button>
            ))}
          </div>

          {/* Контент таба. */}
          <div className="mt-3">
            {tab === "overview" &&
              (detailErr ? (
                failBlock(detailErr, () => void loadDetail(), detailBusy)
              ) : !detail ? (
                loadingBlock
              ) : (
                <div className="flex flex-col gap-3">
                  {cover && (
                    <img
                      src={cover.url}
                      alt={cover.title ?? ""}
                      loading="lazy"
                      className="h-64 w-full rounded-lg object-cover"
                    />
                  )}
                  {detail.body && <Markdown text={detail.body} />}
                </div>
              ))}

            {tab === "changelog" &&
              (versionsErr ? (
                failBlock(versionsErr, () => void loadVersions(), versionsBusy)
              ) : versionsBusy || versions === null ? (
                loadingBlock
              ) : changelogList.length === 0 ? (
                <p className="py-8 text-sm text-text-muted">{t("project.changelog.empty")}</p>
              ) : (
                <div>
                  {changelogList.map((v) => {
                    const badge = typeBadge(v.versionType);
                    return (
                      <div key={v.id} className="border-t border-border-app py-3 first:border-t-0">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-sm font-semibold">{v.versionNumber}</span>
                          {badge && <span className={badge.cls}>{t(badge.key)}</span>}
                          {v.datePublished && (
                            <span className="text-xs text-text-muted">
                              {new Date(v.datePublished).toLocaleDateString(currentLanguage())}
                            </span>
                          )}
                        </div>
                        <p className="mt-1 whitespace-pre-wrap text-[13px] leading-relaxed text-text-muted">
                          {v.changelog}
                        </p>
                      </div>
                    );
                  })}
                </div>
              ))}

            {tab === "gallery" &&
              (detailErr ? (
                failBlock(detailErr, () => void loadDetail(), detailBusy)
              ) : !detail ? (
                loadingBlock
              ) : detail.gallery.length === 0 ? (
                <p className="py-8 text-sm text-text-muted">{t("project.gallery.empty")}</p>
              ) : (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
                  {detail.gallery.map((g, idx) => (
                    <button
                      key={idx}
                      onClick={() => setZoom(g)}
                      aria-label={g.title ?? g.caption ?? g.url}
                      className="overflow-hidden rounded-lg"
                    >
                      <img
                        src={g.url}
                        alt={g.title ?? ""}
                        title={g.title ?? g.caption}
                        loading="lazy"
                        className="h-36 w-full object-cover transition-transform hover:scale-105"
                      />
                    </button>
                  ))}
                </div>
              ))}

            {tab === "versions" &&
              (versionsErr ? (
                failBlock(versionsErr, () => void loadVersions(), versionsBusy)
              ) : versionsBusy || versions === null ? (
                loadingBlock
              ) : (
                <div className="flex flex-col gap-2">
                  {/* Для модов — выбор инстанса и кнопка у каждой версии;
                      у остальных типов установка только главной кнопкой. */}
                  {isMod ? (
                    <label className="flex items-center gap-2 text-[13px] text-text-muted">
                      {t("project.chooseInstance")}
                      <select
                        value={targetId}
                        onChange={(e) => setTargetId(e.target.value)}
                        disabled={busy || instances.length === 0}
                        className="h-8 max-w-56 rounded-lg border border-border-strong bg-card px-2 text-[13px] disabled:opacity-40"
                      >
                        {instances.map((i) => (
                          <option key={i.id} value={i.id}>
                            {i.name} [{i.mcVersion}]
                          </option>
                        ))}
                      </select>
                    </label>
                  ) : (
                    <p className="text-[13px] text-text-muted">{t("project.installPackNote")}</p>
                  )}
                  {versions.length === 0 ? (
                    <p className="py-8 text-sm text-text-muted">{t("project.versions.empty")}</p>
                  ) : (
                    versions.map((v) => {
                      const badge = typeBadge(v.versionType);
                      const primary = v.files.find((f) => f.primary) ?? v.files[0];
                      const gv =
                        v.gameVersions.slice(0, 3).join(", ") +
                        (v.gameVersions.length > 3 ? ` +${v.gameVersions.length - 3}` : "");
                      return (
                        <div
                          key={v.id}
                          className="flex flex-wrap items-center gap-2 rounded-lg border border-border-app px-3 py-2"
                        >
                          <span className="font-mono text-[13px] font-semibold">
                            {v.versionNumber}
                          </span>
                          {badge && <span className={badge.cls}>{t(badge.key)}</span>}
                          {gv && <span className="text-[13px] text-text-muted">{gv}</span>}
                          {v.loaders.length > 0 && (
                            <span className="badge-soft">{v.loaders.join(", ")}</span>
                          )}
                          {v.datePublished && (
                            <span className="text-[13px] text-text-muted">
                              {new Date(v.datePublished).toLocaleDateString(currentLanguage())}
                            </span>
                          )}
                          {primary && (
                            <span className="chip-mono text-xs">{fmtBytes(primary.size)}</span>
                          )}
                          {isMod && (
                            <button
                              onClick={() => void install(v.id)}
                              disabled={busy || !targetId}
                              className="btn-ghost btn-sm ml-auto"
                            >
                              {t("project.versions.install", { version: v.versionNumber })}
                            </button>
                          )}
                        </div>
                      );
                    })
                  )}
                </div>
              ))}
          </div>
        </div>

        {/* Увеличенный просмотр галереи: оверлей поверх модалки, клик закрывает. */}
        {zoom && (
          <div
            className="fixed inset-0 z-[60] flex cursor-zoom-out flex-col items-center justify-center gap-2 bg-black/80 p-6"
            onClick={() => setZoom(null)}
          >
            <img
              src={zoom.url}
              alt={zoom.title ?? ""}
              className="max-h-[80vh] max-w-full rounded-lg object-contain"
            />
            {zoom.caption && (
              <p className="max-w-2xl text-center text-[13px] text-white/80">{zoom.caption}</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
