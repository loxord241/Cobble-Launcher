// Дефолтный арт инстанса (D35): у инстанса без своей картинки — процедурная
// пиксельная «карта» вместо заглушки-буквы. Детерминированная от имени:
// один инстанс — всегда один и тот же пейзаж, разные инстансы — разные.
// Чистый SVG без внешних ассетов (офлайн-шрифты/картинки — правило владельца).

/** FNV-1a 32 бит — тот же хэш, что в ядре (util/fnv.rs), для одинаковой
 * стабильности «имя → арт» между сессиями достаточно и фронтового. */
function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** mulberry32: компактный детерминированный PRNG от seed. */
function rng(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

type Palette = {
  /** Верхние 2 ряда — «поверхность» (трава/песок/камень…). */
  surface: [string, string, string];
  /** Нижние 4 ряда — «глубина». */
  deep: [string, string, string];
  /** Редкие вкрапления («руда») — акцент палитры. */
  ore: string;
};

const PALETTES: Palette[] = [
  // Луг
  { surface: ["#8fcc5a", "#7cbd4b", "#6aa93f"], deep: ["#7a5a3a", "#6b4d32", "#8a6a45"], ore: "#4be0d8" },
  // Пустыня
  { surface: ["#f0e0b5", "#e8d5a3", "#d9c48d"], deep: ["#c9a86a", "#b8975a", "#d4b47a"], ore: "#f4c542" },
  // Камень
  { surface: ["#9aa0aa", "#8a8f98", "#7a7f88"], deep: ["#62666e", "#565a62", "#6e727a"], ore: "#b07ce8" },
  // Незер
  { surface: ["#9a3a3a", "#8a3030", "#7a2828"], deep: ["#5a1e1e", "#4a1a1a", "#662424"], ore: "#f4c542" },
  // Океан
  { surface: ["#4a80b5", "#3a6ea5", "#2f5f96"], deep: ["#1e3f66", "#183456", "#244a76"], ore: "#f4845f" },
  // Аметист
  { surface: ["#a58ad4", "#967ac8", "#876abc"], deep: ["#5e4a80", "#523e72", "#6a5490"], ore: "#4be0d8" },
];

const COLS = 8;
const ROWS = 6;
const CELL = 32;

const cache = new Map<string, string>();

/** data-URL процедурного арта для имени инстанса (без иконки). */
export function defaultArtwork(seed: string): string {
  const memo = cache.get(seed);
  if (memo) return memo;

  const h = fnv1a(seed || "mcl");
  const rand = rng(h);
  const pal = PALETTES[h % PALETTES.length];

  const rects: string[] = [];
  for (let y = 0; y < ROWS; y++) {
    const band = y < 2 ? pal.surface : pal.deep;
    for (let x = 0; x < COLS; x++) {
      const color = band[Math.floor(rand() * band.length)];
      rects.push(
        `<rect x="${x * CELL}" y="${y * CELL}" width="${CELL}" height="${CELL}" fill="${color}"/>`,
      );
    }
  }
  // «Руда»: 4–7 акцентных клеток в глубине — половинный квадрат в центре.
  const ores = 4 + Math.floor(rand() * 4);
  for (let i = 0; i < ores; i++) {
    const x = Math.floor(rand() * COLS);
    const y = 2 + Math.floor(rand() * (ROWS - 2));
    const off = CELL / 4;
    rects.push(
      `<rect x="${x * CELL + off}" y="${y * CELL + off}" width="${CELL / 2}" height="${CELL / 2}" fill="${pal.ore}"/>`,
    );
  }
  // Лёгкий блик сверху — «глубина» картинки без градиентных фейков данных.
  rects.push(
    `<rect x="0" y="0" width="${COLS * CELL}" height="${CELL}" fill="#ffffff" fill-opacity="0.05"/>`,
  );

  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${COLS * CELL}" height="${ROWS * CELL}" shape-rendering="crispEdges">` +
    rects.join("") +
    `</svg>`;
  const url = `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
  cache.set(seed, url);
  return url;
}
