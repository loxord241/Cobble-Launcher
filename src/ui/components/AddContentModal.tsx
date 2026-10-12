// Поиск и установка контента Modrinth прямо из меню инстанса (запрос
// владельца: без перекидывания на страницу «Моды»). Паттерн — ModsPage:
// табы типа, поиск, бесконечный скролл, установка; модалка — LogViewerModal.
import { useCallback, useEffect, useRef, useState } from "react";
import { Download, Loader2, Search, SearchX, X } from "lucide-react";
import { api } from "../../api/client";
import { apiErrorText, currentLanguage, plural, t } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import type { Instance, SearchHit } from "../../api/types";

const PAGE_SIZE = 20;

// Модпаки тут нет: они создают новый инстанс, а не ставятся в существующий.
type ContentType = "mod" | "resourcepack" | "shader" | "datapacks";

const TABS: { id: ContentType; defaultQuery: string }[] = [
  { id: "mod", defaultQuery: "mods" },
  { id: "resourcepack", defaultQuery: "resourcepacks" },
  { id: "shader", defaultQuery: "shaders" },
  { id: "datapacks", defaultQuery: "datapacks" },
];

/** project_type для Modrinth API — единственное число (как в ModsPage). */
function projectTypeOf(tab: ContentType): string {
  return tab === "datapacks" ? "datapack" : tab;
}

/** Фильтр загрузчика осмыслен только для модов/датапаков: у ресурспаков и
 * шейдеров своих загрузчиков нет — не сужаем выдачу чужим фильтром. */
function loaderFilterOf(tab: ContentType, loader: string | undefined): string | undefined {
  return tab === "resourcepack" || tab === "shader" ? undefined : loader;
}

export default function AddContentModal({
  instance,
  onClose,
  onInstalled,
}: {
  instance: Instance;
  onClose: () => void;
  onInstalled?: () => void;
}) {
  const [tab, setTab] = useState<ContentType>("mod");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [total, setTotal] = useState(0);
  const [searched, setSearched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  // project_id ставящегося сейчас контента: у его кнопки — спиннер.
  const [installing, setInstalling] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installedFiles, setInstalledFiles] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const sentinelRef = useRef<HTMLDivElement>(null);
  // A33: Escape + ловушка фокуса + возврат фокуса на кнопку-триггер.
  // D41: requestClose — закрытие с анимацией (для ✕ и подложки).
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose);

  // D64: гонка поиска — при быстрой смене вкладки/запроса приходит ответ
  // прошлого запроса, и под вкладкой оказывается чужой список. Общий
  // seq-guard для поиска и дозагрузки: применяем только свежий ответ.
  const searchSeqRef = useRef(0);

  const doSearch = async () => {
    const seq = ++searchSeqRef.current;
    setBusy(true);
    setError(null);
    // P3-ревизии: баннер «Установлено: …» не должен переживать смену вкладки
    // и повторный поиск — висеть над чужим списком ему нечего.
    setInstalledFiles(null);
    try {
      const r = await api.modrinthSearch(
        query.trim() || TABS.find((x) => x.id === tab)!.defaultQuery,
        instance.mcVersion,
        loaderFilterOf(tab, instance.loader),
        projectTypeOf(tab),
        undefined,
        PAGE_SIZE,
        0,
      );
      if (seq !== searchSeqRef.current) return;
      setHits(r.hits);
      setTotal(r.total_hits);
      setSearched(true);
    } catch (e) {
      if (seq !== searchSeqRef.current) return;
      setError(apiErrorText(e));
    } finally {
      if (seq === searchSeqRef.current) setBusy(false);
    }
  };

  // Автопоиск при открытии модалки.
  useEffect(() => {
    void doSearch();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Смена вкладки — повторный поиск по выбранному типу контента. P2-ревизии:
  // безусловно (не только при searched) — переключение до первого ответа
  // иначе оставляло список модов под вкладкой «Шейдеры».
  useEffect(() => {
    void doSearch();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab]);

  // Дозагрузка следующей страницы (offset = сколько уже загрузили).
  const loadMore = useCallback(async () => {
    if (loadingMore || busy || !searched || hits.length >= total) return;
    const seq = ++searchSeqRef.current;
    setLoadingMore(true);
    setError(null);
    try {
      const r = await api.modrinthSearch(
        query.trim() || TABS.find((x) => x.id === tab)!.defaultQuery,
        instance.mcVersion,
        loaderFilterOf(tab, instance.loader),
        projectTypeOf(tab),
        undefined,
        PAGE_SIZE,
        hits.length,
      );
      if (seq !== searchSeqRef.current) return;
      // Дедуп по project_id: Modrinth может вернуть уже показанную строку.
      const known = new Set(hits.map((h) => h.project_id));
      setHits((prev) => [...prev, ...r.hits.filter((h) => !known.has(h.project_id))]);
      setTotal(r.total_hits);
    } catch (e) {
      if (seq !== searchSeqRef.current) return;
      setError(apiErrorText(e));
    } finally {
      if (seq === searchSeqRef.current) setLoadingMore(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadingMore, busy, searched, hits, total, query, tab]);

  // Бесконечный скролл: root — внутренний скролл-контейнер модалки, а не main.
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) void loadMore();
      },
      { root: scrollRef.current, rootMargin: "600px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [loadMore]);

  const install = async (projectId: string) => {
    setInstalling(projectId);
    setError(null);
    setInstalledFiles(null);
    try {
      const entries = await api.contentInstall(instance.id, projectId);
      setInstalledFiles(entries.map((e) => e.file).join(", "));
      onInstalled?.();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setInstalling(null);
    }
  };

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
        aria-label={t("addContent.title", { name: instance.name })}
        className="anim-dialog-in flex max-h-[80vh] w-full max-w-2xl flex-col rounded-lg border border-border-app bg-card p-5 shadow-2xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="truncate text-lg font-semibold text-text">
            {t("addContent.title", { name: instance.name })}
          </h2>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        {/* Вкладки типов контента (стиль вкладок страницы Модов). */}
        <div role="tablist" aria-label={t("mods.tabs.label")} className="mt-3 flex flex-wrap gap-1">
          {TABS.map((x) => (
            <button
              key={x.id}
              role="tab"
              aria-selected={tab === x.id}
              onClick={() => setTab(x.id)}
              className={`h-9 rounded-lg px-3 text-sm font-medium transition-colors ${
                tab === x.id
                  ? "bg-accent text-on-accent"
                  : "text-text-muted hover:bg-surface-2 hover:text-text"
              }`}
            >
              {t(`mods.tab.${x.id}`)}
            </button>
          ))}
        </div>

        {/* Шейдерам нужен шейдер-конвейер: тихая подсказка под вкладкой. */}
        {tab === "shader" && (
          <p className="mt-1.5 text-[12px] leading-snug text-text-muted">
            {t("mods.shader.needsIris")}
          </p>
        )}

        {/* Ряд поиска: Enter или кнопка. */}
        <div className="mt-2 flex items-center gap-2">
          <div className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-lg border border-border-strong bg-card px-3 text-text-muted">
            <Search size={15} aria-hidden />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && !busy && void doSearch()}
              placeholder={t("mods.searchPlaceholder")}
              className="w-full bg-transparent font-mono text-sm text-text outline-none placeholder:font-ui placeholder:text-text-muted"
              aria-label={t("mods.searchPlaceholder")}
            />
          </div>
          <button onClick={() => void doSearch()} disabled={busy} className="btn-primary btn-sm">
            <Search size={14} aria-hidden />
            {t("mods.search")}
          </button>
        </div>

        {/* Статусные баннеры: ошибка установки/поиска и успех установки. */}
        {error && (
          <p
            className="mt-2 rounded-lg border border-error/40 bg-error/10 p-2 text-sm text-error"
            role="alert"
          >
            {error}
          </p>
        )}
        {installedFiles && (
          <p className="mt-2 rounded-lg border border-success/40 bg-success/10 p-2 text-sm text-success">
            {t("addContent.installed", { files: installedFiles })}
          </p>
        )}

        {/* Список результатов: скроллится внутри модалки (root наблюдателя). */}
        <div ref={scrollRef} className="mt-3 min-h-0 flex-1 overflow-y-auto">
          {busy && hits.length === 0 ? (
            <div className="flex justify-center py-8">
              <Loader2 size={20} aria-hidden className="animate-spin text-text-muted" />
            </div>
          ) : searched && hits.length === 0 ? (
            // Пустое состояние: Modrinth ответил, но по фильтру ничего нет.
            <div className="flex flex-col items-center gap-2 px-4 py-8 text-center">
              <SearchX size={28} aria-hidden className="text-text-muted" />
              <div className="text-sm font-semibold">{t("mods.empty.title")}</div>
              <div className="max-w-sm text-[13px] text-text-muted">{t("mods.empty.desc")}</div>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              {hits.map((h) => (
                // Компактная строка-карточка: иконка 48px, заголовок + автор и
                // загрузки; описание не показываем — место в модалке дорого.
                <div
                  key={h.project_id}
                  className="grid min-h-[64px] cursor-default grid-cols-[48px_1fr_auto] items-center gap-3 rounded-lg border border-border-app bg-card px-3 py-2 transition-all duration-200 hover:border-accent/70 hover:bg-card-hover hover:shadow-lg hover:shadow-black/20 hover:-translate-y-px"
                >
                  {h.icon_url ? (
                    <img
                      src={h.icon_url}
                      alt=""
                      className="size-12 rounded-lg border border-border-app object-cover"
                    />
                  ) : (
                    <span
                      aria-hidden
                      className="grid size-12 place-items-center rounded-lg bg-surface-2 text-base font-semibold text-text-muted"
                    >
                      {h.title.slice(0, 1)}
                    </span>
                  )}
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold leading-tight">{h.title}</div>
                    <div className="mt-0.5 truncate font-mono text-[12px] text-text-muted">
                      {t("mods.by", { author: h.author })} ·{" "}
                      {plural("mods.downloads", h.downloads, {
                        n: h.downloads.toLocaleString(currentLanguage()),
                      })}
                    </div>
                  </div>
                  <button
                    onClick={() => void install(h.project_id)}
                    disabled={installing !== null}
                    aria-busy={installing === h.project_id}
                    className={`btn-primary btn-sm${installing === h.project_id ? " anim-pulse-soft" : ""}`}
                    aria-label={`${t("mods.installInto")} — ${h.title}`}
                  >
                    {installing === h.project_id ? (
                      <Loader2 size={14} aria-hidden className="animate-spin" />
                    ) : (
                      <Download size={14} aria-hidden />
                    )}
                    {t("mods.installInto")}
                  </button>
                </div>
              ))}
              {/* Бесконечный скролл: sentinel догружает страницы, кнопка — для клавиатуры. */}
              {hits.length < total && (
                <div ref={sentinelRef} className="flex justify-center py-2">
                  <button onClick={() => void loadMore()} disabled={loadingMore} className="btn-ghost btn-sm">
                    {loadingMore ? t("mods.loadingMore") : t("mods.loadMore")}
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
