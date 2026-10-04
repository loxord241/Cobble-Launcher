// Главная — каталог Modrinth в стиле CurseForge App: переключатель типа
// (модпаки/моды), поиск, сортировка, фильтр загрузчика, бесконечный скролл
// строк. Клик по строке/кнопке «Установить» открывает окно проекта —
// установка происходит внутри окна. Инстансы живут на странице «Инстансы».
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react"
import { Dices, Download, Search, SearchX } from "lucide-react"
import { api } from "../../api/client"
import { apiErrorText, currentLanguage, plural, t } from "../../i18n"
import type { SearchHit } from "../../api/types"

// Самый тяжёлый лист каталога (4 таба + галерея + markdown) грузится только
// при первом открытии окна проекта — стартовый чанк меньше.
const ProjectDetailModal = lazy(() => import("../components/ProjectDetailModal"))

const PAGE_SIZE = 20

type CatalogType = "modpack" | "mod"
type SortIndex = "downloads" | "follows" | "updated" | "newest" | "relevance"

const LOADERS = ["fabric", "forge", "neoforge", "quilt"]

/** Относительная дата из ISO («5 дней назад») на языке UI — для «Обновлено {when}». */
function relativeDate(iso: string): string {
  const time = new Date(iso).getTime()
  if (!Number.isFinite(time)) return iso
  const rtf = new Intl.RelativeTimeFormat(currentLanguage(), { numeric: "auto" })
  const diffMs = Date.now() - time
  const minutes = Math.round(diffMs / 60000)
  if (Math.abs(minutes) < 60) return rtf.format(-minutes, "minute")
  const hours = Math.round(diffMs / 3600000)
  if (Math.abs(hours) < 24) return rtf.format(-hours, "hour")
  const days = Math.round(diffMs / 86400000)
  if (Math.abs(days) < 30) return rtf.format(-days, "day")
  if (Math.abs(days) < 365) return rtf.format(-Math.round(days / 30), "month")
  return rtf.format(-Math.round(days / 365), "year")
}

/**
 * Строка каталога в стиле CurseForge App: иконка 56px, заголовок + автор,
 * описание в 2 строки, метаданные (категории, загрузки, обновление, версии).
 * Клик по строке И по кнопке «Установить» не качает — обе открывают окно
 * проекта, установка живёт внутри окна (явное требование владельца).
 */
function CatalogRow({ hit, idx, onOpen }: { hit: SearchHit, idx: number, onOpen: () => void }) {
  return (
    <div
      role="button"
      tabIndex={0}
      // Каскад появления: задержка по индексу с кэпом 20 строк (~440ms) —
      // внутри каждой догруженной партии; backwards держит строку прозрачной
      // до старта, элемент в DOM и кликабелен с первого кадра.
      style={{ animationDelay: `${(idx % 20) * 22}ms` }}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return // Enter/Space на кнопке внутри строки
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault()
          onOpen()
        }
      }}
      className="anim-fade-up grid min-h-[76px] cursor-pointer grid-cols-[56px_1fr_auto] items-center gap-3 rounded-lg border border-border-app bg-card px-3 py-2.5 transition-all duration-200 hover:-translate-y-px hover:border-accent/70 hover:bg-card-hover hover:shadow-lg hover:shadow-black/20"
    >
      {hit.icon_url ? (
        <img
          src={hit.icon_url}
          alt=""
          loading="lazy"
          className="size-14 rounded-lg border border-border-app object-cover"
        />
      ) : (
        // Без иконки — заглушка с первой буквой названия.
        <span
          aria-hidden
          className="grid size-14 place-items-center rounded-lg bg-surface-2 text-lg font-semibold text-text-muted"
        >
          {hit.title.slice(0, 1).toUpperCase()}
        </span>
      )}
      <div className="min-w-0">
        <div className="flex min-w-0 items-baseline gap-2">
          <span className="min-w-0 truncate text-[15px] font-semibold leading-tight">
            {hit.title}
          </span>
          <span className="truncate text-[12px] text-text-muted">
            {t("mods.by", { author: hit.author })}
          </span>
        </div>
        {/* Описание всегда занимает две строки (min-h + line-clamp-2): без
            этого строки каталога «прыгают» по высоте — мета-чипы съезжают.
            Пустое описание рисуем пустым блоком той же высоты, сохраняя ритм. */}
        <div className="mt-0.5 line-clamp-2 min-h-[2.6em] text-[13px] leading-snug text-text-muted">
          {hit.description}
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1">
          {/* Modrinth кладёт в categories и категории, и загрузчики —
              возможны повторы (fabric дважды), дедуплицируем. */}
          {(() => {
            const cats = [...new Set(hit.categories)];
            return (
              <>
                {cats.slice(0, 3).map((c) => (
                  <span key={c} className="badge-soft">
                    {c}
                  </span>
                ))}
                {cats.length > 3 && <span className="chip-mono">+{cats.length - 3}</span>}
              </>
            );
          })()}
          <span className="font-mono text-[13px] text-text-muted">
            {plural("mods.downloads", hit.downloads, {
              n: hit.downloads.toLocaleString(currentLanguage()),
            })}
          </span>
          <span className="text-[12px] text-text-muted">
            {t("project.updated", { when: relativeDate(hit.date_modified) })}
          </span>
          {hit.versions.slice(0, 2).map((v) => (
            <span key={v} className="chip-mono">
              {v}
            </span>
          ))}
        </div>
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation() // клик строки и так открывает окно — действие то же
          onOpen()
        }}
        className="btn-primary"
        aria-label={`${t("project.install")} — ${hit.title}`}
      >
        <Download size={15} aria-hidden />
        {t("home.installPack")}
      </button>
    </div>
  )
}

export default function HomePage({ query }: { query: string }) {
  const [tab, setTab] = useState<CatalogType>("modpack")
  const [sort, setSort] = useState<SortIndex>("downloads")
  const [loader, setLoader] = useState("")
  const [localQuery, setLocalQuery] = useState("")
  const [hits, setHits] = useState<SearchHit[]>([])
  const [total, setTotal] = useState(0)
  const [searched, setSearched] = useState(false)
  const [busy, setBusy] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [detail, setDetail] = useState<SearchHit | null>(null)
  const [luckyBusy, setLuckyBusy] = useState(false)
  const sentinelRef = useRef<HTMLDivElement>(null)
  const stuckRef = useRef<HTMLDivElement>(null)
  const [stuck, setStuck] = useState(false)

  const doSearch = async () => {
    setBusy(true)
    setError(null)
    try {
      // mcVersion не передаём: на Главной нет целевого инстанса. Пустой
      // запрос + index=downloads — «популярное» (ядро: modrinth/api.rs).
      const r = await api.modrinthSearch(
        localQuery,
        undefined,
        loader || undefined,
        tab,
        sort,
        PAGE_SIZE,
        0,
      )
      setHits(r.hits)
      setTotal(r.total_hits)
      setSearched(true)
    } catch (e) {
      setError(apiErrorText(e))
    } finally {
      setBusy(false)
    }
  }

  // Автопоиск при монтировании + повторный поиск с нуля (offset 0) при смене
  // типа/сортировки/загрузчика.
  useEffect(() => {
    void doSearch()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, sort, loader])

  // «Мне повезёт»: случайный проект активного типа (модпак/мод) без учёта
  // текста запроса и фильтров. Два запроса: total_hits → случайная страница
  // из одного проекта; открывается окно проекта, установка — как из каталога.
  const feelingLucky = async () => {
    setLuckyBusy(true)
    setError(null)
    try {
      const count = await api.modrinthSearch("", undefined, undefined, tab, "relevance", 1, 0)
      if (count.total_hits === 0) return
      const offset = Math.floor(Math.random() * count.total_hits)
      const r = await api.modrinthSearch("", undefined, undefined, tab, "relevance", 1, offset)
      if (r.hits[0]) setDetail(r.hits[0])
    } catch (e) {
      setError(apiErrorText(e))
    } finally {
      setLuckyBusy(false)
    }
  }

  // Дозагрузка следующей страницы (offset = сколько уже загрузили, дедуп по project_id).
  const loadMore = useCallback(async () => {
    if (loadingMore || busy || !searched || hits.length >= total) return
    setLoadingMore(true)
    setError(null)
    try {
      const r = await api.modrinthSearch(
        localQuery,
        undefined,
        loader || undefined,
        tab,
        sort,
        PAGE_SIZE,
        hits.length,
      )
      const known = new Set(hits.map((h) => h.project_id))
      setHits((prev) => [...prev, ...r.hits.filter((h) => !known.has(h.project_id))])
      setTotal(r.total_hits)
    } catch (e) {
      setError(apiErrorText(e))
    } finally {
      setLoadingMore(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadingMore, busy, searched, hits, total, localQuery, tab, sort, loader])

  // Бесконечный скролл: sentinel догружает страницы, кнопка — для клавиатуры.
  useEffect(() => {
    const el = sentinelRef.current
    if (!el) return
    const root = el.closest("main")
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) void loadMore()
      },
      { root, rootMargin: "1500px" },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [loadMore])

  // Тень залипшей панели: маркер 0-высоты перед sticky-панелью уходит из
  // видимости main в момент залипания — observer переключает .sticky-fade.on.
  useEffect(() => {
    const el = stuckRef.current
    if (!el) return
    const root = el.closest("main")
    const io = new IntersectionObserver(
      (entries) => setStuck(!entries.some((e) => e.isIntersecting)),
      { root, threshold: 0 },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [])

  // Глобальный поиск из титлбара фильтрует результаты по подстроке.
  const q = query.trim().toLowerCase()
  const visibleHits = hits.filter(
    (h) => !q || h.title.toLowerCase().includes(q) || h.author.toLowerCase().includes(q),
  )

  return (
    <div className="mx-auto flex max-w-[1100px] flex-col gap-4">
      {/* Маркер залипания sticky-панели: 0-высоты, невидим; когда панель
          прилипает к верху main, маркер покидает viewport — это ловит
          IntersectionObserver выше. */}
      <div ref={stuckRef} aria-hidden />
      {/* Панель управления (заголовок+счётчик, табы, поиск+фильтры) залипает
          сверху: скроллится main, поэтому top-0. z-10 достаточно — модалка
          проекта рендерится с z-50 и остаётся поверх. -mx-2/px-2 не дают фону
          обрываться по краям страницы, flex+gap-4 сохраняют прежний ритм
          блоков (раньше их разносил gap родителя). */}
      <div className="sticky -top-6 z-10 -mx-2 flex flex-col gap-4 bg-bg/95 px-2 pt-8 pb-3 backdrop-blur-sm">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("home.catalog.title")}</h2>
        {searched && (
          <span className="chip-mono">{t("home.found", { n: total.toLocaleString(currentLanguage()) })}</span>
        )}
      </div>

      {/* Переключатель типа каталога (стиль табов ModsPage). */}
      <div role="tablist" className="flex gap-1">
        {(["modpack", "mod"] as CatalogType[]).map((x) => (
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
            {t(`mods.tab.${x}`)}
          </button>
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="flex h-11 min-w-72 flex-1 max-w-[460px] items-center gap-2 rounded-lg border border-border-strong bg-card px-3 text-text-muted">
          <Search size={16} aria-hidden />
          <input
            value={localQuery}
            onChange={(e) => setLocalQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void doSearch()}
            placeholder={t(tab === "modpack" ? "home.searchPacks" : "home.searchMods")}
            className="w-full bg-transparent font-mono text-sm text-text outline-none placeholder:text-text-muted placeholder:font-ui"
            aria-label={t(tab === "modpack" ? "home.searchPacks" : "home.searchMods")}
          />
        </div>
        <select
          value={sort}
          onChange={(e) => setSort(e.target.value as SortIndex)}
          disabled={busy}
          aria-label={t("home.sort.label")}
          className="h-11 min-w-40 rounded-lg border border-border-strong bg-card px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40"
        >
          <option value="downloads">{t("home.sort.downloads")}</option>
          <option value="follows">{t("home.sort.follows")}</option>
          <option value="updated">{t("home.sort.updated")}</option>
          <option value="newest">{t("home.sort.newest")}</option>
          <option value="relevance">{t("home.sort.relevance")}</option>
        </select>
        <select
          value={loader}
          onChange={(e) => setLoader(e.target.value)}
          disabled={busy}
          aria-label={t("home.filter.loader")}
          className="h-11 min-w-36 rounded-lg border border-border-strong bg-card px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40"
        >
          <option value="">{t("home.filter.any")}</option>
          {LOADERS.map((l) => (
            <option key={l} value={l}>
              {l}
            </option>
          ))}
        </select>
        {/* Та же высота, что поиск/селекты (h-11 = .btn 44px) — раньше был
            btn-sm 40px, кнопка «Найти» выглядела чужой в ряду (владелец). */}
        <button onClick={() => void doSearch()} disabled={busy} className="btn-primary">
          <Search size={15} aria-hidden />
          {t("mods.search")}
        </button>
        {/* «Мне повезёт» (D58): случайный проект активного таба — окно проекта
            открывается сразу, установка обычным путём. Отдельный busy — не
            мешает обычному поиску. */}
        <button
          onClick={() => void feelingLucky()}
          disabled={busy || luckyBusy}
          className="btn"
          aria-label={t("home.lucky")}
        >
          <Dices size={15} aria-hidden />
          {luckyBusy ? t("home.luckyRolling") : t("home.lucky")}
        </button>
      </div>
      {/* Тень под панелью — только когда панель реально залипла (.sticky-fade
          в tokens.css: absolute, opacity 0 → 1 через класс .on). */}
      <div aria-hidden className={`sticky-fade ${stuck ? "on" : ""}`} />
      </div>

      {error && (
        <p className="rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error" role="alert">
          {error}
        </p>
      )}

      {!searched ? (
        // До первого ответа — скелет-строки в ритме реальных строк каталога
        // (иконка, три полоски текста, место кнопки); пустое состояние —
        // только после поиска.
        <div className="flex flex-col gap-2">
          {Array.from({ length: 6 }, (_, i) => (
            <div
              key={i}
              className="grid min-h-[76px] grid-cols-[56px_1fr_auto] items-center gap-3 rounded-lg border border-border-app bg-card px-3 py-2.5"
            >
              <div className="size-14 rounded-lg bg-surface-2 animate-pulse" />
              <div className="flex min-w-0 flex-col gap-2">
                <div className="h-3 w-2/3 rounded bg-surface-2 animate-pulse" />
                <div className="h-3 w-full rounded bg-surface-2 animate-pulse" />
                <div className="h-3 w-1/2 rounded bg-surface-2 animate-pulse" />
              </div>
              <div className="h-10 w-28 rounded-lg bg-surface-2 animate-pulse" />
            </div>
          ))}
        </div>
      ) : visibleHits.length === 0 ? (
        // Пустое состояние: Modrinth ответил, но по запросу/фильтру ничего нет.
        <div className="mt-10 flex flex-col items-center gap-3 rounded-lg border border-border-app bg-card px-4 py-10 text-center">
          <SearchX size={32} aria-hidden className="text-text-muted" />
          <div className="text-[15px] font-semibold">{t("mods.empty.title")}</div>
          <div className="max-w-sm text-[13px] text-text-muted">{t("mods.empty.desc")}</div>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {visibleHits.map((h, idx) => (
            <CatalogRow key={h.project_id} hit={h} idx={idx} onOpen={() => setDetail(h)} />
          ))}
          {hits.length < total && (
            <div ref={sentinelRef} className="anim-fade-in flex justify-center py-2">
              <button
                onClick={() => void loadMore()}
                disabled={loadingMore}
                className="btn-ghost btn-sm"
              >
                {loadingMore ? t("mods.loadingMore") : t("mods.loadMore")}
              </button>
            </div>
          )}
        </div>
      )}

      {detail && (
        <Suspense fallback={null}>
          <ProjectDetailModal hit={detail} onClose={() => setDetail(null)} />
        </Suspense>
      )}
    </div>
  )
}
