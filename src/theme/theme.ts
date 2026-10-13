/*
 * theme.ts — система тем «Сланец»: тема = CSS-переменные на <html data-theme="…">.
 * Идентификаторы и палитры — из исторического HTML-прототипа «Сланца».
 * Приоритет источника: ?theme= в URL > localStorage > dark (спека фазы A).
 */
export type ThemeId = "dark" | "light" | "cb-dark" | "cb-light" | "mc";

export type ThemeGroup = "classic" | "colorblind" | "game";

export interface ThemeMeta {
  id: ThemeId;
  /** Ключ i18n с названием темы. */
  nameKey: string;
  group: ThemeGroup;
  /** Свотчи для карточки темы (bg/линия/акцент) — 1-в-1 из превью slate-pro. */
  swatches: [string, string, string];
}

const STORAGE_KEY = "theme";

const VALID: readonly ThemeId[] = ["dark", "light", "cb-dark", "cb-light", "mc"];

/** Список тем для страницы «Настройки → Темы» (группы как в CurseForge). */
export const THEMES: ThemeMeta[] = [
  { id: "dark", nameKey: "themes.dark", group: "classic", swatches: ["#1d2026", "#3b424d", "#6cb2fa"] },
  { id: "light", nameKey: "themes.light", group: "classic", swatches: ["#ffffff", "#c3cad3", "#2b6fb8"] },
  { id: "cb-dark", nameKey: "themes.cbDark", group: "colorblind", swatches: ["#1d2026", "#3b424d", "#5aa2ff"] },
  { id: "cb-light", nameKey: "themes.cbLight", group: "colorblind", swatches: ["#ffffff", "#c3cad3", "#2b6fb8"] },
  { id: "mc", nameKey: "themes.mc", group: "game", swatches: ["#1e2023", "#3c4148", "#7cbd4b"] },
];

export function isThemeId(v: string): v is ThemeId {
  return (VALID as readonly string[]).includes(v);
}

function apply(id: ThemeId) {
  const root = document.documentElement;
  if (id === "dark") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", id);
  let meta = document.querySelector<HTMLMetaElement>('meta[name="color-scheme"]');
  if (!meta) {
    meta = document.createElement("meta");
    meta.name = "color-scheme";
    document.head.appendChild(meta);
  }
  meta.content = id === "light" || id === "cb-light" ? "light" : "dark";
}

export function getTheme(): ThemeId {
  const attr = document.documentElement.getAttribute("data-theme");
  return isThemeId(attr ?? "") ? (attr as ThemeId) : "dark";
}

/** Применить тему и сохранить в localStorage. */
export function setTheme(id: ThemeId) {
  apply(id);
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {
    /* приватный режим — тема просто не переживёт перезапуск */
  }
}

/** Источник при старте: ?theme= > localStorage > dark. */
export function initialTheme(): ThemeId {
  const q = new URLSearchParams(window.location.search).get("theme");
  if (q && isThemeId(q)) return q;
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && isThemeId(stored)) return stored;
  } catch {
    /* нет доступа к localStorage */
  }
  return "dark";
}

/** Вызвать синхронно до createRoot — без мигания. */
export function initTheme() {
  apply(initialTheme());
}
