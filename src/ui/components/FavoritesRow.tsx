// Ряд «Избранное» на Главной (D67): горизонтальная лента GD-плиток каталога
// (тот же арт-словарь: полный арт/ambient — мелкую иконку не растягиваем,
// постоянная плашка имени) для проектов, отмеченных сердцем. Данные приходят
// из HomePage (favoritesList + метаданные батчем) — здесь только отрисовка;
// честность: ряд рисуется, только когда список непуст (проверяет вызывающий).
//
// HeartToggle — сердце на верхнем правом углу плитки, ВИДНО ВСЕГДА (запрет
// hover-зависимости — правило владельца), клик не открывает проект
// (stopPropagation — плитка кликабельна целиком). Переиспользуется и плитками
// каталога в HomePage.tsx.
import { useState } from "react"
import { Heart } from "lucide-react"
import { t } from "../../i18n"
import type { ProjectMeta } from "../../api/types"

/** Элемент ряда: id из favoritesList + метаданные (null — ещё едут). */
export interface FavoriteItem {
  projectId: string
  meta: ProjectMeta | null
}

/**
 * Кнопка-сердце избранного. Позицию (правый верх плитки) несёт сама — оба
 * места использования (плитка каталога и ряд избранного) кладут её в один
 * угол, рядом с chip-mono версии (он слева). Заполненное сердце — в избранном.
 */
export function HeartToggle({
  projectId,
  title,
  active,
  disabled,
  onToggle,
}: {
  projectId: string
  /** Имя проекта — в aria-label/подсказку («Добавить „X“ в избранное»). */
  title: string
  active: boolean
  disabled?: boolean
  onToggle: (projectId: string) => void
}) {
  const label = active
    ? t("home.favorites.remove", { name: title })
    : t("home.favorites.add", { name: title })
  return (
    <button
      type="button"
      disabled={disabled}
      aria-pressed={active}
      aria-label={label}
      title={label}
      onClick={(e) => {
        // Плитка целиком открывает окно проекта — сердце только переключает.
        e.stopPropagation()
        e.preventDefault()
        onToggle(projectId)
      }}
      className={`absolute right-2.5 top-2.5 z-10 grid size-7 place-items-center rounded-full border backdrop-blur-sm transition-colors disabled:cursor-wait disabled:opacity-60 ${
        active
          ? "border-accent/70 bg-black/60 text-accent"
          : "border-white/25 bg-black/55 text-white/85 hover:border-accent hover:text-accent"
      }`}
    >
      {/* fill-current: контурное сердце lucide становится заполненным. */}
      <Heart size={14} aria-hidden className={active ? "fill-current" : ""} />
    </button>
  )
}

/**
 * Мелкая иконка (натуральная ширина < 220px) не тянетcя на плитку — та же
 * планка качества, что у каталога (D65). Три строки дублированы намеренно:
 * хук живёт в HomePage рядом с галерейным кэшем, а тянуть его сюда можно
 * только циклическим импортом.
 */
function useIconTooSmall(): [boolean, (e: { currentTarget: HTMLImageElement }) => void] {
  const [small, setSmall] = useState(false)
  return [small, (e) => setSmall(e.currentTarget.naturalWidth < 220)]
}

/** Плитка избранного: тот же GD-словарь, что CatalogTile, без кнопок установки
 * — клик по плитке открывает окно проекта, как в каталоге. */
function FavoriteTile({
  meta,
  idx,
  pending,
  onOpen,
  onToggle,
}: {
  meta: ProjectMeta
  idx: number
  pending: boolean
  onOpen: () => void
  onToggle: (projectId: string) => void
}) {
  const [iconSmall, onIconLoad] = useIconTooSmall()
  const bigArt = meta.iconUrl && !iconSmall ? meta.iconUrl : null
  return (
    <div
      role="button"
      tabIndex={0}
      // Каскад появления — как у плиток каталога (кэп 20 строк на партию).
      style={{ animationDelay: `${(idx % 20) * 22}ms` }}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return // Enter/Space на сердце внутри
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault()
          onOpen()
        }
      }}
      className="anim-fade-up group relative aspect-[280/238] w-44 flex-none cursor-pointer overflow-hidden rounded-xl border border-border-app bg-surface-2 shadow-md shadow-black/20 transition-all duration-200 hover:-translate-y-0.5 hover:border-accent/70 hover:shadow-lg hover:shadow-black/25"
    >
      {bigArt ? (
        <img
          src={bigArt}
          alt=""
          loading="lazy"
          onLoad={onIconLoad}
          className="absolute inset-0 size-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
        />
      ) : meta.iconUrl ? (
        // Ambient: мелкая иконка — размытой подложкой + чёткой копией в центре.
        <>
          <div
            aria-hidden
            className="absolute inset-0 scale-125 bg-cover bg-center blur-lg brightness-[.4]"
            style={{ backgroundImage: `url(${meta.iconUrl})` }}
          />
          <img
            src={meta.iconUrl}
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
          {meta.title.slice(0, 1).toUpperCase()}
        </span>
      )}
      <div
        aria-hidden
        className="absolute inset-0 bg-gradient-to-b from-black/10 via-transparent to-black/80"
      />
      {/* В ряду сердце всегда заполнено — плитка здесь и есть избранное;
          клик убирает (оптимистично, плитка исчезает сразу). */}
      <HeartToggle
        projectId={meta.projectId}
        title={meta.title}
        active
        disabled={pending}
        onToggle={onToggle}
      />
      {/* Постоянная плашка имени — как у каталога (видна без hover). */}
      <div className="absolute inset-x-3 bottom-2.5 min-w-0">
        <h4 className="truncate text-[13.5px] font-semibold leading-tight text-white drop-shadow">
          {meta.title}
        </h4>
      </div>
    </div>
  )
}

/** Скелет-плитка: метаданные проекта ещё не доехали (ритм скелетов страницы). */
function FavoriteSkeleton() {
  return (
    <div
      aria-hidden
      className="flex aspect-[280/238] w-44 flex-none flex-col justify-end gap-2 rounded-xl border border-border-app bg-card p-3"
    >
      <div className="h-3 w-3/4 animate-pulse rounded bg-surface-2" />
      <div className="h-3 w-1/2 animate-pulse rounded bg-surface-2" />
    </div>
  )
}

/** Горизонтальный ряд «Избранное»: заголовок в ритме секций страницы, лента
 * GD-плиток со скроллом; плитки без метаданных — скелетами. */
export default function FavoritesRow({
  items,
  pendingIds,
  onOpen,
  onToggle,
}: {
  items: FavoriteItem[]
  /** Переключения в полёте — сердца этих проектов погашены. */
  pendingIds: Set<string>
  onOpen: (meta: ProjectMeta) => void
  onToggle: (projectId: string) => void
}) {
  if (items.length === 0) return null
  return (
    <section aria-label={t("home.favorites.title")} className="anim-fade-up flex flex-col gap-2">
      {/* Заголовок — тем же словарём, что «Каталог» (text-lg/semibold). */}
      <h3 className="text-lg font-semibold tracking-tight">{t("home.favorites.title")}</h3>
      <div className="flex gap-3 overflow-x-auto pb-1">
        {items.map((item, idx) =>
          item.meta ? (
            <FavoriteTile
              key={item.projectId}
              meta={item.meta}
              idx={idx}
              pending={pendingIds.has(item.projectId)}
              onOpen={() => onOpen(item.meta as ProjectMeta)}
              onToggle={onToggle}
            />
          ) : (
            <FavoriteSkeleton key={item.projectId} />
          ),
        )}
      </div>
    </section>
  )
}
