// Главная — каталог Modrinth в стиле GDLauncher (D65): hero «Популярное
// сейчас» (лидер по скачиваниям среди уже загруженных строк), сетка плиток
// с артом во всю плитку, переключатель типа (модпаки/моды), поиск, сортировка,
// фильтр загрузчика, бесконечный скролл. Клик по плитке/кнопке «Установить»
// открывает окно проекта — установка происходит внутри окна. Инстансы живут
// на странице «Инстансы». Ряд «Избранное» (D67): горизонтальная лента плиток
// проектов, отмеченных сердцем, над выдачей — рисуется, только когда список
// непуст; сердце на плитке каталога и в ряду переключает favoritesToggle.
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react"
import { Dices, Download, Search, SearchX } from "lucide-react"
import { api } from "../../api/client"
import { apiErrorText, currentLanguage, plural, t } from "../../i18n"
import { prettySlug } from "../format"
import FavoritesRow, { HeartToggle } from "../components/FavoritesRow"
import type { ProjectMeta, SearchHit } from "../../api/types"

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

/** Компактный счётчик («838 тыс.» / «1.2M») — для подписей на арте плиток. */
function compactCount(n: number): string {
  return new Intl.NumberFormat(currentLanguage(), {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(n)
}

/**
 * Арт-материал каталога (D65-фикс качества): поиск Modrinth отдаёт иконки
 * 96px (_96.webp) — растянутые на плитку они «сжатые и мутные». Решение:
 * лениво (по одному запросу на проект, кэш на сессию) подтягиваем первую
 * галерейную картинку (~350px — для плитки хватает), мелкие иконки до её
 * прихода показываем честным ambient-стилем (размытая подложка + чёткая
 * иконка в центре), а не апскейлом на всю плитку.
 */
/**
 * Кэп сессионных кэшей: модульные Map живут до перезапуска приложения, а
 * долгая сессия с бесконечным скроллом и сердцами растит их без предела.
 * При превышении лимита выталкиваем САМЫЕ СТАРЫЕ по порядку вставки (Map
 * сохраняет порядок вставки) — удаляем первые ключи, пока размер не дойдёт
 * до лимита. Вытесненная запись при повторном запросе просто перечитается.
 */
const GALLERY_CACHE_CAP = 300
const META_CACHE_CAP = 300
const HIT_CACHE_CAP = 100

function capCache<T>(map: Map<string, T>, limit: number): void {
  if (map.size <= limit) return
  for (const key of map.keys()) {
    map.delete(key)
    if (map.size <= limit) break
  }
}

const galleryCache = new Map<string, Promise<string | null>>()

function loadGalleryArt(projectId: string): Promise<string | null> {
  let p = galleryCache.get(projectId)
  if (!p) {
    p = api
      .modrinthProject(projectId)
      .then((d) => d.gallery?.[0]?.url ?? null)
      .catch(() => null)
    galleryCache.set(projectId, p)
    capCache(galleryCache, GALLERY_CACHE_CAP)
  }
  return p
}

/** Галерейный арт проекта (null — галереи нет); undefined — ещё грузится. */
function useGalleryArt(projectId: string | undefined): string | null | undefined {
  const [url, setUrl] = useState<string | null | undefined>(undefined)
  useEffect(() => {
    let alive = true
    setUrl(undefined)
    if (!projectId) return
    loadGalleryArt(projectId).then((u) => {
      if (alive) setUrl(u)
    })
    return () => {
      alive = false
    }
  }, [projectId])
  return url
}

/** Иконка оказалась мелкой (натуральная ширина < 220px) — не тянуть на плитку. */
function useIconTooSmall(): [boolean, (e: { currentTarget: HTMLImageElement }) => void] {
  const [small, setSmall] = useState(false)
  return [small, (e) => setSmall(e.currentTarget.naturalWidth < 220)]
}

// ---------- Избранное (D67) ----------

/** Метаданные плитки избранного (кэш на сессию — как galleryCache выше). */
const metaCache = new Map<string, Promise<ProjectMeta | null>>()

function loadProjectMeta(projectId: string): Promise<ProjectMeta | null> {
  let p = metaCache.get(projectId)
  if (!p) {
    p = api
      .modrinthProjects([projectId])
      .then((list) => list[0] ?? null)
      .catch(() => null)
    metaCache.set(projectId, p)
    capCache(metaCache, META_CACHE_CAP)
  }
  return p
}

/**
 * Полный SearchHit для плитки избранного: с D67 modrinthProjects несёт и
 * project_type, но не все поля (автор/категории/версии) — поиск по названию
 * с ТОЧНЫМ совпадением project_id досыталяет hit полностью. Решение
 * по названию + ТОЧНОЕ совпадение project_id в выдаче — hit настоящий, без
 * выдуманных полей. null — не нашли (модалка останется на метаданных).
 */
const hitCache = new Map<string, Promise<SearchHit | null>>()

function resolveFavoriteHit(projectId: string, title: string): Promise<SearchHit | null> {
  let p = hitCache.get(projectId)
  if (!p) {
    p = api
      .modrinthSearch(title, undefined, undefined, undefined, "relevance", 20, 0)
      .then((r) => r.hits.find((h) => h.project_id === projectId) ?? null)
      .catch(() => null)
    hitCache.set(projectId, p)
    capCache(hitCache, HIT_CACHE_CAP)
  }
  return p
}

/**
 * Стартовый hit плитки избранного из ProjectMeta: название/описание/иконка
 * настоящие, остальное пусто — модалка это переживает (чипов без данных нет,
 * detail грузит сама), а пустой проектный сценарий — безопасный дефолт.
 * Как только поиск ответит, hit заменяется полным (см. openFavorite).
 */
function metaToHit(meta: ProjectMeta): SearchHit {
  return {
    project_id: meta.projectId,
    project_type: meta.projectType || "mod",
    slug: "",
    author: "",
    title: meta.title,
    description: meta.description,
    categories: [],
    versions: [],
    downloads: 0,
    icon_url: meta.iconUrl,
    date_modified: "",
  }
}

/**
 * HERO «Популярное сейчас» (D65): широкая плитка на всю ширину контента с
 * артом пака-лидера по скачиваниям среди УЖЕ загруженных строк (icon_url той
 * же выдачи — никакого нового API и выдуманных данных). Снизу градиент-вуаль
 * (как .veil макета), поверх — chip-mono версии, надзаголовок, название,
 * «автор · скачивания · обновлён N» и ghost-кнопка «Установить»: и клик по
 * плитке, и клик по кнопке открывают окно проекта — установка не отсюда.
 */
function HeroTile({ hit, onOpen }: { hit: SearchHit, onOpen: () => void }) {
  // D65-фикс качества: галерея (~350px) как арт; фон — размытая подложка,
  // справа — ЧЁТКИЙ кадр в рамке (даунскейл, не апскейл). Иконку 96px на
  // всю ширину hero не растягиваем.
  const gallery = useGalleryArt(hit.project_id)
  const art = gallery ?? hit.icon_url
  return (
    <section
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return // Enter/Space на кнопке внутри
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault()
          onOpen()
        }
      }}
      className="anim-fade-up relative flex h-48 cursor-pointer flex-col overflow-hidden rounded-xl border border-border-app bg-surface-2 shadow-lg shadow-black/25 transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/70"
    >
      {art && (
        <>
          {/* Размытая подложка на всю ширину — скрывает малый источник. */}
          <div
            aria-hidden
            className="absolute inset-0 scale-125 bg-cover bg-center blur-xl brightness-[.45]"
            style={{ backgroundImage: `url(${art})` }}
          />
          {/* Чёткий кадр справа: даунскейл до ~208×128 — резкий при любом
              источнике; рамка/тень как у «featured» витрины. */}
          <img
            src={art}
            alt=""
            loading="lazy"
            className="absolute bottom-4 right-4 hidden h-32 w-52 rounded-lg border border-white/20 object-cover shadow-lg shadow-black/40 sm:block"
          />
        </>
      )}
      {/* Вуаль, как .veil макета: арт темнеет книзу — текст читается на любом арте.
          Верх тоже притемнён: на светлых баннерах бейдж версии тонул. */}
      <div
        aria-hidden
        className="absolute inset-0 bg-gradient-to-b from-black/25 via-black/55 to-black/90"
      />
      {hit.versions[0] && (
        <span className="chip-mono absolute left-3 top-3 bg-bg/85">{hit.versions[0]}</span>
      )}
      <div className="relative mt-auto flex items-end gap-4 p-4">
        <div className="min-w-0 sm:max-w-[calc(100%-13.5rem)]">
          <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-white/75">
            {t("home.featured")}
          </div>
          <div className="mt-0.5 truncate text-[17px] font-bold leading-tight text-white [text-shadow:0_2px_8px_rgb(0_0_0_/_0.6)]">
            {hit.title}
          </div>
          <div className="mt-1 truncate text-[12px] text-white/90 [text-shadow:0_1px_6px_rgb(0_0_0_/_0.55)]">
            {hit.author}
            {" · "}
            {plural("mods.downloads", hit.downloads, { n: compactCount(hit.downloads) })}
            {" · "}
            {t("project.updated", { when: relativeDate(hit.date_modified) })}
          </div>
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation() // клик hero и так открывает окно — действие то же
            onOpen()
          }}
          aria-label={`${t("project.install")} — ${hit.title}`}
          className="ml-auto inline-flex h-10 shrink-0 items-center gap-2 rounded-lg border border-white/25 bg-black/55 px-4 text-sm font-semibold text-white backdrop-blur-sm transition-colors hover:border-accent hover:text-accent"
        >
          <Download size={15} aria-hidden />
          {t("home.installPack")}
        </button>
      </div>
    </section>
  )
}

/**
 * Плитка каталога в стиле GDLauncher (D65): арт во всю плитку (пропорция
 * макета ~280×238), chip-mono версии слева-сверху и ПОСТОЯННАЯ плашка имени
 * снизу (название + «скачивания · загрузчик») — имя видно всегда, требование
 * владельца, hover не единственный путь к нему. По hover/фокусу над плашкой
 * проявляются доп-метаданные (автор, обновление) и мини-кнопка «Установить»
 * (ghost). Клик по плитке И по кнопке открывает окно проекта (D39).
 */
function CatalogTile({
  hit,
  idx,
  favorite,
  favPending,
  onOpen,
  onToggleFavorite,
}: {
  hit: SearchHit,
  idx: number,
  /** Проект в избранном — сердце заполнено. */
  favorite: boolean,
  /** Переключение сердца в полёте — кнопка погашена. */
  favPending: boolean,
  onOpen: () => void,
  onToggleFavorite: () => void,
}) {
  // Загрузчик для подписи под именем: Modrinth кладёт его в categories.
  const loader = hit.categories.find((c) => LOADERS.includes(c))
  // D65-фикс качества: галерея (~350px) во всю плитку — резкая; мелкая
  // иконка (натуральная ширина < 220px) — ambient (размытая подложка +
  // чёткая иконка по центру), НЕ апскейл на всю плитку.
  const gallery = useGalleryArt(hit.project_id)
  const [iconSmall, onIconLoad] = useIconTooSmall()
  const bigArt = gallery ?? (hit.icon_url && !iconSmall ? hit.icon_url : null)
  return (
    <div
      role="button"
      tabIndex={0}
      // Каскад появления: задержка по индексу с кэпом 20 строк (~440ms) —
      // внутри каждой догруженной партии; backwards держит плитку прозрачной
      // до старта, элемент в DOM и кликабелен с первого кадра.
      style={{ animationDelay: `${(idx % 20) * 22}ms` }}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return // Enter/Space на кнопке внутри
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault()
          onOpen()
        }
      }}
      className="anim-fade-up group relative aspect-[280/238] cursor-pointer overflow-hidden rounded-xl border border-border-app bg-surface-2 shadow-md shadow-black/20 transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/70 hover:shadow-lg hover:shadow-black/25"
    >
      {bigArt ? (
        <img
          src={bigArt}
          alt=""
          loading="lazy"
          onLoad={gallery ? undefined : onIconLoad}
          className="absolute inset-0 size-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
        />
      ) : hit.icon_url ? (
        // Ambient: мелкая иконка как материал подложки (blur прячет апскейл),
        // сверху — чёткая копия в натуральном размере.
        <>
          <div
            aria-hidden
            className="absolute inset-0 scale-125 bg-cover bg-center blur-lg brightness-[.4]"
            style={{ backgroundImage: `url(${hit.icon_url})` }}
          />
          <img
            src={hit.icon_url}
            alt=""
            loading="lazy"
            className="absolute inset-0 m-auto size-24 object-contain drop-shadow-lg"
          />
        </>
      ) : (
        // Без иконки — честная заглушка с первой буквой названия.
        <span
          aria-hidden
          className="absolute inset-0 grid place-items-center bg-gradient-to-br from-surface-2 to-bg text-4xl font-bold text-text-muted"
        >
          {hit.title.slice(0, 1).toUpperCase()}
        </span>
      )}
      {/* Вуаль снизу — подложка постоянной плашки имени. */}
      <div
        aria-hidden
        className="absolute inset-0 bg-gradient-to-b from-black/10 via-transparent to-black/80"
      />
      {hit.versions[0] && (
        <span className="chip-mono absolute left-2.5 top-2.5">{hit.versions[0]}</span>
      )}
      {/* Сердце избранного — видно всегда (запрет hover-зависимости), клик
          не открывает проект. Правый верх — chip версии занимает левый. */}
      <HeartToggle
        projectId={hit.project_id}
        title={hit.title}
        active={favorite}
        disabled={favPending}
        onToggle={() => onToggleFavorite()}
      />
      {/* Hover/фокус: автор, обновление и мини-кнопка «Установить». Проявляется
          и с клавиатуры (group-focus-within) — не только мышью; pointer-events
          не дают кликать по невидимому слою вне hover. */}
      <div className="pointer-events-none absolute inset-x-3 bottom-[52px] flex translate-y-1 flex-col opacity-0 transition-all duration-200 group-hover:pointer-events-auto group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:pointer-events-auto group-focus-within:translate-y-0 group-focus-within:opacity-100">
        <div className="truncate text-[11.5px] text-white/85">
          {t("mods.by", { author: hit.author })}
        </div>
        <div className="truncate text-[11px] text-white/70">
          {t("project.updated", { when: relativeDate(hit.date_modified) })}
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation() // клик плитки и так открывает окно — действие то же
            onOpen()
          }}
          aria-label={`${t("project.install")} — ${hit.title}`}
          className="mt-1.5 inline-flex h-8 w-fit items-center gap-1.5 rounded-lg border border-white/25 bg-black/60 px-3 text-xs font-semibold text-white backdrop-blur-sm transition-colors hover:border-accent hover:text-accent"
        >
          <Download size={13} aria-hidden />
          {t("home.installPack")}
        </button>
      </div>
      {/* Постоянная плашка имени: название + «скачивания · загрузчик». */}
      <div className="absolute inset-x-3 bottom-2.5 min-w-0">
        <h3 className="truncate text-[13.5px] font-semibold leading-tight text-white drop-shadow">
          {hit.title}
        </h3>
        <div className="truncate text-[11.5px] text-white/80">
          {compactCount(hit.downloads)}
          {loader ? ` · ${prettySlug(loader)}` : ""}
        </div>
      </div>
    </div>
  )
}

export default function HomePage({ query }: { query: string }) {
  const [tab, setTab] = useState<CatalogType>("modpack")
  const [sort, setSort] = useState<SortIndex>("downloads")
  const [loader, setLoader] = useState("")
  const [category, setCategory] = useState("")
  const [localQuery, setLocalQuery] = useState("")
  const [hits, setHits] = useState<SearchHit[]>([])
  const [total, setTotal] = useState(0)
  const [searched, setSearched] = useState(false)
  const [busy, setBusy] = useState(false)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [detail, setDetail] = useState<SearchHit | null>(null)
  const [luckyBusy, setLuckyBusy] = useState(false)
  // Избранное (D67): id в порядке добавления (favoritesList) + метаданные
  // плиток (modrinthProjects). Ряд рисуется, только когда список непуст —
  // правило честности «нет данных — нет элемента».
  const [favoriteIds, setFavoriteIds] = useState<string[]>([])
  const [favMeta, setFavMeta] = useState<Record<string, ProjectMeta>>({})
  // Сердца с переключением в полёте: кнопку гасим, параллельные toggle одного
  // проекта не гоняем (их ответы могли бы прийти не в порядке кликов).
  const [favPending, setFavPending] = useState<Set<string>>(() => new Set())
  // P3-ревизии: проекты, для которых Modrinth метаданные НЕ вернул (удалены
  // с сайта) — вместо вечного скелета рисуем плитку-заглушку.
  const [favOrphans, setFavOrphans] = useState<Set<string>>(() => new Set())
  const sentinelRef = useRef<HTMLDivElement>(null)
  const stuckRef = useRef<HTMLDivElement>(null)
  const [stuck, setStuck] = useState(false)
  // Гонка поисковых ответов: устаревший запрос, пришедший позже свежего,
  // не должен затирать выдачу (счётчик поколений запроса).
  const searchSeqRef = useRef(0)

  const doSearch = async () => {
    const mySeq = ++searchSeqRef.current
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
        // Категория уходит в ядро фасетом (одна — из селекта).
        category ? [category] : undefined,
      )
      if (searchSeqRef.current !== mySeq) return // ответ устарел — отбрасываем
      setHits(r.hits)
      setTotal(r.total_hits)
      setSearched(true)
    } catch (e) {
      if (searchSeqRef.current !== mySeq) return
      setError(apiErrorText(e))
    } finally {
      // busy снимает только самое свежее поколение запроса.
      if (searchSeqRef.current === mySeq) setBusy(false)
    }
  }

  // Автопоиск при монтировании + повторный поиск с нуля (offset 0) при смене
  // типа/сортировки/загрузчика/категории.
  useEffect(() => {
    void doSearch()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, sort, loader, category])

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
    // D62: поколение запроса — как в doSearch: смена tab/sort/loader/category
    // в полёте дозагрузки не должна аппендить страницы старой выдачи к новой.
    const mySeq = ++searchSeqRef.current
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
        category ? [category] : undefined,
      )
      if (searchSeqRef.current !== mySeq) return // ответ устарел — не аппендим
      const known = new Set(hits.map((h) => h.project_id))
      setHits((prev) => [...prev, ...r.hits.filter((h) => !known.has(h.project_id))])
      setTotal(r.total_hits)
    } catch (e) {
      if (searchSeqRef.current !== mySeq) return
      setError(apiErrorText(e))
    } finally {
      // loadingMore снимаем безусловно: параллельного loadMore нет (гард
      // выше), а устаревший ответ не должен навсегда блокировать дозагрузку.
      setLoadingMore(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadingMore, busy, searched, hits, total, localQuery, tab, sort, loader, category])

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

  // HERO «Популярное сейчас»: лид по скачиваниям среди УЖЕ загруженных строк
  // (клиентский max — без нового запроса, это честно). Только для модпаков;
  // на «Модах» и пока строк нет — hero не рисуется (и без скелета: просто
  // пусто до первого ответа).
  const featured = useMemo(() => {
    if (tab !== "modpack") return null
    let best: SearchHit | null = null
    for (const h of visibleHits) if (!best || h.downloads > best.downloads) best = h
    return best
  }, [tab, visibleHits])

  // Динамический union категорий текущей выдачи (для селекта фильтра):
  // загрузчики исключаем — у них отдельный селект выше.
  const categories = useMemo(() => {
    const set = new Set<string>()
    for (const h of hits) for (const c of h.categories) set.add(c)
    return [...set].filter((c) => !LOADERS.includes(c)).sort()
  }, [hits])

  // Выбранной категории нет в union текущей выдачи (пришёл пустой ответ,
  // фильтры поменялись в обход селекта) — сбрасываем, иначе селект висит «мимо».
  useEffect(() => {
    if (category !== "" && searched && !categories.includes(category)) setCategory("")
  }, [category, categories, searched])

  // ---------- Избранное (D67) ----------

  /** Идемпотентно добавить/убрать id — оптимистичный апдейт, сверка, откат. */
  const applyFavorite = (projectId: string, present: boolean) => {
    setFavoriteIds((prev) => {
      const has = prev.includes(projectId)
      if (present === has) return prev
      return present ? [...prev, projectId] : prev.filter((id) => id !== projectId)
    })
  }

  const rememberMeta = (meta: ProjectMeta) => {
    setFavMeta((prev) => (prev[meta.projectId] ? prev : { ...prev, [meta.projectId]: meta }))
  }

  // Первичная загрузка: список id + метаданные одним батчем. Ответ сливаем
  // через union, а не подменой: сердце могли успеть нажать, пока список ехал, —
  // подмена стёрла бы оптимистичную плитку (расхождение закроет сверка toggle).
  useEffect(() => {
    let alive = true
    api
      .favoritesList()
      .then(async (ids) => {
        if (!alive) return
        setFavoriteIds((prev) => {
          const merged = [...prev]
          for (const id of ids) if (!merged.includes(id)) merged.push(id)
          return merged
        })
        if (ids.length === 0) return
        try {
          const metas = await api.modrinthProjects(ids)
          if (!alive) return
          setFavMeta((prev) => {
            const next = { ...prev }
            for (const m of metas) next[m.projectId] = m
            return next
          })
          // Батч вернулся без части id — эти проекты удалены с Modrinth,
          // метаданные для них не приедут никогда; помечаем, чтобы плитка-
          // заглушка сменила вечный скелет (ошибка сети сюда не попадает —
          // она уходит в catch ниже и скелеты остаются честными).
          const got = new Set(metas.map((m) => m.projectId))
          const missing = ids.filter((id) => !got.has(id))
          if (missing.length > 0) {
            setFavOrphans((prev) => {
              const next = new Set(prev)
              for (const id of missing) next.add(id)
              return next
            })
          }
        } catch (e) {
          if (alive) setError(apiErrorText(e))
        }
      })
      .catch((e) => {
        if (alive) setError(apiErrorText(e))
      })
    return () => {
      alive = false
    }
  }, [])

  // Переключение сердца: оптимистично меняем список, ядро подтверждает
  // (boolean = «добавлено» — по нему сверяемся), при ошибке откатываем и
  // показываем её общим notice страницы. Свежедобавленному подтягиваем
  // метаданные плитки (кэш на сессию — повторно ядро не дёргаем).
  const toggleFavorite = async (projectId: string) => {
    if (favPending.has(projectId)) return
    const wasFavorite = favoriteIds.includes(projectId)
    setFavPending((prev) => {
      const next = new Set(prev)
      next.add(projectId)
      return next
    })
    applyFavorite(projectId, !wasFavorite)
    // Свежедобавленному подтягиваем метаданные плитки (кэш на сессию —
    // повторно ядро не дёргаем). null — Modrinth проект не отдал (удалён
    // с сайта) — плитка-заглушка вместо вечного скелета.
    if (!wasFavorite) {
      void loadProjectMeta(projectId).then((m) => {
        if (m) rememberMeta(m)
        else
          setFavOrphans((prev) => {
            const next = new Set(prev)
            next.add(projectId)
            return next
          })
      })
    }
    try {
      const added = await api.favoritesToggle(projectId)
      applyFavorite(projectId, added)
    } catch (e) {
      applyFavorite(projectId, wasFavorite)
      setError(apiErrorText(e))
    } finally {
      setFavPending((prev) => {
        const next = new Set(prev)
        next.delete(projectId)
        return next
      })
    }
  }

  // Клик по плитке избранного: окно проекта открывается МГНОВЕННО по
  // метаданным (как из каталога), полный hit (downloads/author/тип проекта)
  // догоняет поиском по точному совпадению project_id и заменяет объект —
  // модалка с тем же project_id не перечитывает вкладки, только подставляет
  // данные. Пока поиск ехал, окно могли закрыть/открыть другое — меняем
  // только своё.
  const openFavorite = (meta: ProjectMeta) => {
    setDetail(metaToHit(meta))
    void resolveFavoriteHit(meta.projectId, meta.title).then((hit) => {
      if (hit) setDetail((cur) => (cur?.project_id === meta.projectId ? hit : cur))
    })
  }

  // Элементы ряда: порядок favoritesList; плитки, чьи метаданные ещё не
  // доехали, FavoritesRow рисует скелетами (в ритме скелетов страницы) —
  // но только пока запрос в полёте: помеченные осиротевшими рисуются
  // плиткой-заглушкой (метаданные для них не приедут никогда).
  const favoriteItems = favoriteIds.map((id) => ({
    projectId: id,
    meta: favMeta[id] ?? null,
    orphan: favOrphans.has(id),
  }))

  return (
    <div className="mx-auto flex max-w-[1240px] flex-col gap-4">
      {/* Маркер залипания sticky-панели: 0-высоты, невидим; когда панель
          прилипает к верху main, маркер покидает viewport — это ловит
          IntersectionObserver выше. */}
      <div ref={stuckRef} aria-hidden />
      {/* Панель управления (заголовок+счётчик, табы, поиск+фильтры) залипает
          сверху: скроллится main, поэтому top-0. z-10 достаточно — модалка
          проекта рендерится с z-50 и остаётся поверх. -mx-2/px-2 не дают фону
          обрываться по краям страницы, flex+gap-4 сохраняют прежний ритм
          блоков (раньше их разносил gap родителя). */}
      <div className="sticky -top-6 z-10 -mx-2 flex flex-col gap-3 bg-bg/95 px-2 pt-8 pb-3 backdrop-blur-sm">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("home.catalog.title")}</h2>
        {searched && (
          <span className="chip-mono">{t("home.found", { n: total.toLocaleString(currentLanguage()) })}</span>
        )}
      </div>

      {/* Переключатель типа каталога — сегмент, как .seg в макете GD. */}
      <div
        role="tablist"
        className="flex h-10 w-fit items-center gap-0.5 rounded-lg border border-border-strong bg-card p-1"
      >
        {(["modpack", "mod"] as CatalogType[]).map((x) => (
          <button
            key={x}
            role="tab"
            aria-selected={tab === x}
            // Категорию сбрасываем вместе с табом: union категорий у модпаков
            // и модов разный — выбранное в другом табе могло там отсутствовать.
            onClick={() => {
              setTab(x)
              setCategory("")
            }}
            className={`h-8 rounded-md px-3.5 text-sm font-medium transition-colors ${
              tab === x
                ? "bg-surface-2 text-text"
                : "text-text-muted hover:text-text"
            }`}
          >
            {t(`mods.tab.${x}`)}
          </button>
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="flex h-10 min-w-56 flex-1 max-w-[420px] items-center gap-2 rounded-lg border border-border-strong bg-card px-3 text-text-muted">
          <Search size={16} aria-hidden />
          <input
            value={localQuery}
            onChange={(e) => setLocalQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !busy && void doSearch()}
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
          className="h-10 min-w-40 rounded-lg border border-border-strong bg-card px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40"
        >
          <option value="downloads">{t("home.sort.downloads")}</option>
          <option value="follows">{t("home.sort.follows")}</option>
          <option value="updated">{t("home.sort.updated")}</option>
          <option value="newest">{t("home.sort.newest")}</option>
          <option value="relevance">{t("home.sort.relevance")}</option>
        </select>
        <select
          value={loader}
          onChange={(e) => {
            // Категорию сбрасываем вместе с загрузчиком: union категорий
            // зависит и от него — выбранное могло там отсутствовать.
            setLoader(e.target.value)
            setCategory("")
          }}
          disabled={busy}
          aria-label={t("home.filter.loader")}
          className="h-10 min-w-36 rounded-lg border border-border-strong bg-card px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40"
        >
          <option value="">{t("home.filter.any")}</option>
          {LOADERS.map((l) => (
            <option key={l} value={l}>
              {prettySlug(l)}
            </option>
          ))}
        </select>
        <select
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          disabled={busy}
          aria-label={t("home.filter.category")}
          className="h-10 min-w-36 rounded-lg border border-border-strong bg-card px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40"
        >
          <option value="">{t("home.filter.categoryAny")}</option>
          {categories.map((c) => (
            <option key={c} value={c}>
              {/* Slug → читаемый лейбл: t("slug.*"), фолбэк — «Adventure rpg». */}
              {prettySlug(c)}
            </option>
          ))}
        </select>
        {/* Та же высота, что поиск/селекты (h-10 = btn-sm 40px) — компактный
            ряд фильтров над сеткой (D65). */}
        <button onClick={() => void doSearch()} disabled={busy} className="btn-primary btn-sm">
          <Search size={15} aria-hidden />
          {t("mods.search")}
        </button>
        {/* «Мне повезёт» (D58): случайный проект активного таба — окно проекта
            открывается сразу, установка обычным путём. Отдельный busy — не
            мешает обычному поиску. */}
        <button
          onClick={() => void feelingLucky()}
          disabled={busy || luckyBusy}
          className="btn btn-sm"
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

      {/* Ряд «Избранное» (D67): горизонтальная лента отмеченных сердцем
          проектов над выдачей; пустой список — элемента нет (честность). */}
      {favoriteItems.length > 0 && (
        <FavoritesRow
          items={favoriteItems}
          pendingIds={favPending}
          onOpen={openFavorite}
          onToggle={(id) => void toggleFavorite(id)}
        />
      )}

      {!searched ? (
        // До первого ответа — скелет-плитки в ритме реальной сетки (арт-заглушка
        // + две полоски текста у низа); пустое состояние — только после поиска.
        // Своего скелета у hero нет (D65): пока строк нет — просто пусто.
        <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
          {Array.from({ length: 8 }, (_, i) => (
            <div
              key={i}
              className="flex aspect-[280/238] flex-col justify-end gap-2 rounded-xl border border-border-app bg-card p-3"
            >
              <div className="h-3 w-3/4 rounded bg-surface-2 animate-pulse" />
              <div className="h-3 w-1/2 rounded bg-surface-2 animate-pulse" />
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
        <div className="flex flex-col gap-3">
          {/* HERO «Популярное сейчас»: лид выдачи по скачиваниям, только модпаки. */}
          {featured && <HeroTile hit={featured} onOpen={() => setDetail(featured)} />}
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
            {visibleHits.map((h, idx) => (
              <CatalogTile
                key={h.project_id}
                hit={h}
                idx={idx}
                favorite={favoriteIds.includes(h.project_id)}
                favPending={favPending.has(h.project_id)}
                onOpen={() => setDetail(h)}
                onToggleFavorite={() => void toggleFavorite(h.project_id)}
              />
            ))}
          </div>
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
