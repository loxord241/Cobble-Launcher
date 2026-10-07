// Окно проекта Modrinth (D39 → D65, анатомия GDLauncher): арт-хедер на всю
// ширину модалки с вуалью снизу, поверх — chip-mono версии, название и строка
// «автор · скачивания · обновлён»; ✕ в углу арта. Ниже — крупная установка,
// чипы категорий и табы «Обзор / Версии / Журнал изменений / Галерея» с
// 2px-подчёркиванием. Тело проекта грузим при открытии, версии — лениво при
// первом открытии табов «Журнал»/«Версии» (кэш в состоянии на всё время
// жизни модалки).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Download, ExternalLink, HardDrive, Heart, LoaderCircle, Scale, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, onCoreEvent } from "../../api/client";
import { formatSize } from "../format";
import { apiErrorText, currentLanguage, plural, t } from "../../i18n";
import { useInstances } from "../../state/instances";
import { useModalA11y } from "../hooks/useModalA11y";
import { Markdown } from "./markdown";
import type {
  CoreEvent,
  GalleryImage,
  Instance,
  ModrinthVersion,
  ProjectDetail,
  SearchHit,
} from "../../api/types";

// Порядок табов — как в GD-макете (экран 3): Обзор / Версии / Журнал / Галерея.
type Tab = "overview" | "versions" | "changelog" | "gallery";

const TABS: Tab[] = ["overview", "versions", "changelog", "gallery"];


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
  if (versionType === "release")
    return { key: "project.type.release", cls: "badge-soft accent" };
  if (versionType === "beta") return { key: "project.type.beta", cls: "badge-soft warning" };
  if (versionType === "alpha") return { key: "project.type.alpha", cls: "badge-soft warning" };
  return null;
}

// Changelog не рисуем одной pre-wrap «простынёй»: сырые дефисы и длинные
// переносы падают на левый край. Маркированные строки («- »/«* ») собираем
// в списки, пустые выбрасываем, остальное — отдельные абзацы.
function changelogBlocks(text: string): { ul: boolean; items: string[] }[] {
  const blocks: { ul: boolean; items: string[] }[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    const bullet = /^[-*]\s+(.+)$/.exec(line)
    if (bullet) {
      const last = blocks[blocks.length - 1]
      if (last && last.ul) last.items.push(bullet[1])
      else blocks.push({ ul: true, items: [bullet[1]] })
    } else if (line) {
      blocks.push({ ul: false, items: [line] })
    }
  }
  return blocks
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
  // Установка модпака (D64): группа загрузок приходит в событиях — точное имя
  // ядро генерирует со случайным хвостом, до ответа команды его знает только
  // ядро. Ловим dl_progress/dl_group_done по префиксу `mrpack:{projectId}:`
  // и запоминаем точную группу для «Отменить» (downloads_cancel_group).
  const [packProgress, setPackProgress] = useState<{ done: number; total: number } | null>(null);
  const packGroupRef = useRef<string | null>(null);

  const isPack = hit.project_type === "modpack";
  const isMod = hit.project_type === "mod";

  // Escape при открытом зуме закрывает только зум. Слушатель висит на window
  // в capture-фазе — она срабатывает РАНЬШЕ keydown-слушателя хука (document,
  // capture), и stopPropagation не даёт хуку пометить модалку закрывающейся
  // (anim-closing с pointer-events:none), когда закрылся лишь зум.
  useEffect(() => {
    if (!zoom) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setZoom(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [zoom]);
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose);

  // D64: установку модпака больше не держим в заложниках окна — ✕/Escape
  // закрывают модалку, установка честно идёт в фоне (ядро дочитает группу),
  // а подписка ниже снимается при размонтировании без утечек.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    const prefix = `mrpack:${hit.project_id}:`;
    const onEvent = (ev: CoreEvent) => {
      if (ev.event === "dl_progress" && ev.group.startsWith(prefix)) {
        // Точная группа (со случайным хвостом) — единственный адресат отмены.
        packGroupRef.current = ev.group;
        setPackProgress({ done: ev.doneBytes, total: ev.totalBytes });
      } else if (ev.event === "dl_group_done" && ev.group.startsWith(prefix)) {
        // Конец группы: полоска больше не нужна; успех/ошибку показывает
        // баннер от самого вызова установки.
        setPackProgress(null);
      }
    };
    void onCoreEvent(onEvent).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
      packGroupRef.current = null;
    };
  }, [hit.project_id]);

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
    // Сразу при открытии: чип размера в шапке одинаковый на всех табах,
    // а табы «Версии»/«Журнал изменений» не ждут догрузки.
    ensureVersions();
  }, [ensureVersions]);

  const install = async (versionId?: string) => {
    setBusy(true);
    setBanner(null);
    packGroupRef.current = null;
    setPackProgress(null);
    try {
      if (isPack) {
        // Модпак всегда ставится в НОВЫЙ инстанс (само ядро): { instance, group }.
        const { instance: inst } = await api.modpackInstall(hit.project_id);
        // F1: обновляем глобальный стор инстансов, иначе новая сборка —
        // «фантом»: в ядре есть, а на странице «Сборки» появится только
        // после перезапуска (стор уже загружен и сам не перечитывается).
        await useInstances.getState().load();
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
      setPackProgress(null);
      packGroupRef.current = null;
    }
  };

  /** D64: «Отменить» — гасим группу загрузок модпака; сама установка после
   * этого завершится ошибкой отмены, её покажет существующий баннер. */
  const cancelPack = () => {
    const group = packGroupRef.current;
    if (!group) return;
    void api
      .downloadsCancelGroup(group)
      .catch((e) => setBanner({ ok: false, text: apiErrorText(e) }));
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
  // Арт-хедер (GD): приоритет — широкая картинка галереи (featured, затем
  // любая), fallback — иконка проекта; пока detail не дошёл — иконка из hit.
  const headerArt =
    (detail ? (detail.gallery.find((g) => g.featured) ?? detail.gallery[0])?.url : undefined) ??
    hit.icon_url;
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
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={hit.title}
        className="anim-dialog-in flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-lg border border-border-app bg-card shadow-2xl"
      >
        {/* ── Арт-хедер на всю ширину (GD): картинка/иконка, вуаль снизу,
            поверх — версия, название, «автор · скачивания · обновлён»;
            ✕ в углу арта (доступна и при busy, D64). ─────────────────── */}
        <div className="relative h-[150px] shrink-0">
          {headerArt ? (
            <img
              src={headerArt}
              alt=""
              aria-hidden
              draggable={false}
              className="absolute inset-0 size-full object-cover"
            />
          ) : (
            // Арта нет вовсе: спокойный градиент вместо пустого пятна.
            <div
              aria-hidden
              className="absolute inset-0 bg-[linear-gradient(120deg,#2c3c50,#151c26_55%,#1a2330)]"
            />
          )}
          {/* Вуаль: сверху почти прозрачная, к низу темнеет — текст читается. */}
          <div
            aria-hidden
            className="absolute inset-0 bg-[linear-gradient(180deg,rgba(7,11,16,0.05)_30%,rgba(7,11,16,0.55)_62%,rgba(7,11,16,0.88)_96%)]"
          />
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="absolute right-3 top-3 grid size-8 place-items-center rounded-md bg-black/60 text-white/85 transition-colors hover:bg-black/80 hover:text-white"
          >
            <X size={16} aria-hidden />
          </button>
          <div className="absolute inset-x-5 bottom-3 flex min-w-0 flex-col gap-1">
            {hit.versions[0] && <span className="chip-mono self-start">{hit.versions[0]}</span>}
            <h2 className="truncate text-xl font-bold leading-tight text-white drop-shadow-[0_2px_12px_rgba(0,0,0,0.55)]">
              {hit.title}
              {/* Установка идёт — спиннер прямо в заголовке: окно «живое». */}
              {busy && (
                <LoaderCircle
                  size={16}
                  aria-hidden
                  className="ml-2 inline-block animate-spin align-[-2px] text-accent"
                />
              )}
            </h2>
            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[13px] text-white/85 drop-shadow-[0_1px_6px_rgba(0,0,0,0.6)]">
              <span>{t("mods.by", { author: hit.author })}</span>
              <span aria-hidden className="text-white/40">
                ·
              </span>
              <span className="tabular-nums">
                {plural("mods.downloads", detail?.downloads ?? hit.downloads, {
                  n: (detail?.downloads ?? hit.downloads).toLocaleString(currentLanguage()),
                })}
              </span>
              {when && (
                <>
                  <span aria-hidden className="text-white/40">
                    ·
                  </span>
                  <span>{t("project.updated", { when })}</span>
                </>
              )}
            </div>
          </div>
        </div>

        <div className="flex-1 overflow-y-auto px-5 pb-5">
          {/* Баннер установки: успех (зелёный) / ошибка (красный). */}
          {banner && (
            <p
              role={banner.ok ? "status" : "alert"}
              className={`mt-4 rounded-lg border p-3 text-sm ${
                banner.ok
                  ? "border-success/40 bg-success/10 text-success"
                  : "border-error/40 bg-error/10 text-error"
              }`}
            >
              {banner.text}
            </p>
          )}

          {/* Действия (GD): крупная «Установить», рядом цель (моды) либо
              прогресс + «Отменить» (модпак, D64 — логика прежняя). */}
          <div className="mt-4 flex flex-wrap items-center gap-2.5">
            <button
              onClick={() => void install()}
              disabled={busy || (!isPack && !targetId)}
              aria-busy={busy}
              className={`btn-primary h-11 min-w-40 px-6 text-[15px] ${busy ? "anim-pulse-soft" : ""}`}
            >
              {busy ? (
                <LoaderCircle size={16} aria-hidden className="animate-spin" />
              ) : (
                <Download size={16} aria-hidden />
              )}
              {busy ? t("project.installing") : t("project.install")}
            </button>
            {!isPack && (
              <select
                value={targetId}
                onChange={(e) => setTargetId(e.target.value)}
                disabled={busy || instances.length === 0}
                aria-label={t("project.chooseInstance")}
                className="h-10 max-w-52 rounded-lg border border-border-strong bg-card px-2.5 text-sm disabled:opacity-40"
              >
                {instances.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name} [{i.mcVersion}]
                  </option>
                ))}
              </select>
            )}
            {/* D64: прогресс скачивания модпака — проценты + полоска рядом
                с кнопкой; totalBytes ещё неизвестен ядру — показываем без %. */}
            {isPack && busy && packProgress && (
              <div className="flex w-44 flex-col gap-1" role="status">
                <span className="flex justify-between font-mono text-[11px] text-text-muted tabular-nums">
                  <span>
                    {packProgress.total > 0
                      ? `${Math.min(Math.floor((packProgress.done / packProgress.total) * 100), 100)}%`
                      : formatSize(packProgress.done)}
                  </span>
                  {packProgress.total > 0 && (
                    <span>
                      {formatSize(packProgress.done)} / {formatSize(packProgress.total)}
                    </span>
                  )}
                </span>
                <div className="h-1 w-full overflow-hidden rounded-full bg-surface-2">
                  <div
                    className="h-full rounded-full bg-accent anim-progress-fill"
                    style={{
                      transform: `scaleX(${
                        packProgress.total > 0
                          ? Math.min(packProgress.done / packProgress.total, 1)
                          : 0
                      })`,
                    }}
                  />
                </div>
              </div>
            )}
            {/* D64: «Отменить» видна сразу, но активна, только когда известна
                точная группа (пришло событие dl_progress): отменять нечего,
                пока группа не зарегистрирована ядром. */}
            {isPack && busy && (
              <button
                onClick={cancelPack}
                disabled={!packGroupRef.current}
                aria-label={t("common.cancel")}
                className="btn-ghost px-4 text-sm"
              >
                {t("common.cancel")}
              </button>
            )}
          </div>

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

          {/* Оставшаяся статистика (скачивания/обновление — в арт-хедере);
              чип рисуем только когда данные есть. */}
          {(detail || packSize !== null) && (
            <div className="mt-2 flex flex-wrap items-center gap-2">
              {detail && (
                <span className="chip-mono flex items-center gap-1 text-xs">
                  <Heart size={12} aria-hidden />
                  {t("project.follows", { n: detail.follows.toLocaleString(currentLanguage()) })}
                </span>
              )}
              {packSize !== null && (
                <span className="chip-mono flex items-center gap-1 text-xs">
                  <HardDrive size={12} aria-hidden />
                  {formatSize(packSize)}
                </span>
              )}
              {detail?.license && (
                <span className="chip-mono flex items-center gap-1 text-xs">
                  <Scale size={12} aria-hidden />
                  {t("project.license", { name: detail.license })}
                </span>
              )}
            </div>
          )}

          {/* Табы (GD): 2px-подчёркивание под активным, счётчики — в бейджах
              (появляются, только когда данные уже загружены). */}
          <div role="tablist" className="mt-4 flex gap-1 border-b border-border-app">
            {TABS.map((x) => {
              const count =
                x === "versions"
                  ? versions !== null
                    ? versions.length
                    : undefined
                  : x === "changelog"
                    ? versions !== null
                      ? changelogList.length
                      : undefined
                    : x === "gallery"
                      ? detail
                        ? detail.gallery.length
                        : undefined
                      : undefined;
              return (
                <button
                  key={x}
                  role="tab"
                  aria-selected={tab === x}
                  onClick={() => setTab(x)}
                  className={`-mb-px flex items-center gap-1.5 border-b-2 px-3 pb-2.5 pt-2 text-sm font-medium transition-colors ${
                    tab === x
                      ? "border-accent text-accent"
                      : "border-transparent text-text-muted hover:text-text"
                  }`}
                >
                  {t(`project.tab.${x}`)}
                  {count !== undefined && (
                    <span className="rounded-full bg-surface-2 px-1.5 text-[11px] font-semibold leading-[18px] text-text-muted tabular-nums">
                      {count}
                    </span>
                  )}
                </button>
              );
            })}
          </div>

          {/* Контент таба. key={tab}: при смене вкладки ремоунт переигрывает
              anim-fade-up — контент мягко вплывает. */}
          <div key={tab} className="anim-fade-up mt-3">
            {tab === "overview" &&
              (detailErr ? (
                failBlock(detailErr, () => void loadDetail(), detailBusy)
              ) : !detail ? (
                loadingBlock
              ) : (
                <div className="flex flex-col gap-3">
                  {hit.description && (
                    <p className="max-w-2xl text-[13px] leading-relaxed text-text-muted">
                      {hit.description}
                    </p>
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
                      // py-4 + граница: записи журнала не должны прилипать друг к другу.
                      <div key={v.id} className="border-t border-border-app py-4 first:border-t-0">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-sm font-semibold">{v.versionNumber}</span>
                          {badge && <span className={badge.cls}>{t(badge.key)}</span>}
                          {v.datePublished && (
                            <span className="text-xs text-text-muted">
                              {new Date(v.datePublished).toLocaleDateString(currentLanguage())}
                            </span>
                          )}
                        </div>
                        <div className="mt-1 space-y-1 text-[13px] leading-relaxed text-text-muted">
                          {changelogBlocks(v.changelog ?? "").map((b, bi) =>
                            b.ul ? (
                              <ul key={bi} className="list-disc space-y-1 pl-5">
                                {b.items.map((item, li) => (
                                  <li key={li}>{item}</li>
                                ))}
                              </ul>
                            ) : (
                              <p key={bi}>{b.items[0]}</p>
                            ),
                          )}
                        </div>
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
                      {/* Подпись под картинкой: caption приоритетнее title. */}
                      {(g.caption || g.title) && (
                        <span className="mt-1 block truncate text-[12px] text-text-muted">
                          {g.caption ?? g.title}
                        </span>
                      )}
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
                <div className="flex flex-col">
                  {/* Для модов — выбор инстанса и кнопка у каждой версии;
                      у остальных типов установка только главной кнопкой. */}
                  {isMod ? (
                    <label className="flex items-center gap-2 pb-1 text-[13px] text-text-muted">
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
                    <p className="pb-1 text-[13px] text-text-muted">{t("project.installPackNote")}</p>
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
                        // Компактная строка (GD .vrow): номер — mono, дата/размер
                        // справа, tabular-nums; граница снизу вместо карточек.
                        <div
                          key={v.id}
                          className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border-app py-2 text-[13px] tabular-nums last:border-b-0"
                        >
                          <span className="min-w-32 font-mono font-semibold">{v.versionNumber}</span>
                          {/* min-w-14: колонки «версия / тип» идут ровно по всем строкам;
                              justify-center — потому что .badge-soft это inline-flex. */}
                          {badge && (
                            <span className={`${badge.cls} min-w-14 justify-center text-center`}>
                              {t(badge.key)}
                            </span>
                          )}
                          {gv && <span className="text-text-muted">{gv}</span>}
                          {v.loaders.length > 0 && (
                            <span className="badge-soft">{v.loaders.join(", ")}</span>
                          )}
                          {/* Дата/размер — правый край строки. */}
                          <span className="ml-auto flex items-center gap-2 text-text-muted">
                            {v.datePublished && (
                              <span>
                                {new Date(v.datePublished).toLocaleDateString(currentLanguage())}
                              </span>
                            )}
                            {primary && (
                              <span className="chip-mono text-xs">{formatSize(primary.size)}</span>
                            )}
                          </span>
                          {isMod && (
                            <button
                              onClick={() => void install(v.id)}
                              disabled={busy || !targetId}
                              className="btn-ghost btn-sm"
                            >
                              {t("project.versions.install", { version: v.versionNumber })}
                            </button>
                          )}
                          {/* Ссылка на страницу версии на Modrinth — для ВСЕХ типов
                              проекта; id берём из hit: у ModrinthVersion поля projectId нет. */}
                          <button
                            onClick={() =>
                              void openUrl(
                                `https://modrinth.com/project/${hit.project_id}/version/${v.id}`,
                              ).catch(() => {})
                            }
                            aria-label={t("project.versions.open")}
                            title={t("project.versions.open")}
                            className="rounded p-1 text-text-muted hover:bg-surface-2 hover:text-text"
                          >
                            <ExternalLink size={14} aria-hidden />
                          </button>
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
              className="anim-zoom-in max-h-[80vh] max-w-full rounded-lg object-contain"
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
