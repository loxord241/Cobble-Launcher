// D63: панорама Minecraft как дефолтный арт. Ядро отдаёт data-URL PNG-грани
// куба (1024×1024) из собственных ассетов игры; здесь — кроп 4:3 в JPEG
// через canvas (карточка 4:3, объект ~50 КБ вместо 0.5 МБ) и кэш на сессию.

import { api } from "../api/client";

const ART_W = 640;
const ART_H = 480;
/** D64: отрицательный кэш (панорамы нет) живёт 30 с — ассеты игры приезжают
 * позже (установка/первый запуск), и закэшированный навсегда null не дал бы
 * панораме появиться без перезапуска лаунчера. */
const NEGATIVE_TTL_MS = 30_000;

interface CacheEntry {
  promise: Promise<string | null>;
  /** Резолв стал null (или упал) — запись «отрицательная», протухает по TTL. */
  negative: boolean;
  ts: number;
}

const cache = new Map<string, CacheEntry>();

function cropToCard(dataUrl: string): Promise<string | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      try {
        // Панорама — квадрат 1024×1024; кроп 4:3 по ЦЕНТРУ ПО ВЕРТИКАЛИ:
        // ширина целиком, высота = ширина × 3/4. (Кроп «по бокам» давал бы
        // srcW > naturalWidth и чёрный pillarbox справа — грабля первой
        // итерации.)
        const srcW = img.naturalWidth;
        const srcH = Math.round(srcW * (ART_H / ART_W));
        const sy = Math.max(0, Math.round((img.naturalHeight - srcH) / 2));
        const canvas = document.createElement("canvas");
        canvas.width = ART_W;
        canvas.height = ART_H;
        const ctx = canvas.getContext("2d");
        // D64: пустой контекст (getContext может дать null) — это фолбэк
        // пиксель-пейзажем, а не серый пустой арт.
        if (!ctx) {
          resolve(null);
          return;
        }
        ctx.drawImage(img, 0, sy, srcW, srcH, 0, 0, ART_W, ART_H);
        resolve(canvas.toDataURL("image/jpeg", 0.85));
      } catch {
        resolve(null); // tainted canvas и прочее — фронт покажет пиксель-пейзаж
      }
    };
    img.onerror = () => resolve(null);
    img.src = dataUrl;
  });
}

function requestPanorama(id: string): CacheEntry {
  // negative/ts — прощупывается при резолве: TTL отрицательного кэша считается
  // от завершения запроса, успешный результат живёт вечно.
  const entry: CacheEntry = { promise: Promise.resolve(null), negative: true, ts: Date.now() };
  entry.promise = api
    .panoramaArt(id)
    .then((url) => (url ? cropToCard(url) : null))
    .then((art) => {
      entry.negative = art === null;
      entry.ts = Date.now();
      return art;
    })
    .catch(() => {
      entry.negative = true;
      entry.ts = Date.now();
      return null;
    });
  return entry;
}

/** Готовый арт-URL инстанса: панорама (если игра скачана) или null.
 * D64: mcVersion входит в ключ кэша — смена версии инстанса даёт другую
 * панораму. */
export function instancePanorama(id: string, mcVersion?: string): Promise<string | null> {
  const key = `${id}@${mcVersion ?? ""}`;
  const memo = cache.get(key);
  // Успех кэшируется навсегда; null/ошибка — на 30 с, потом пробуем снова.
  if (memo && !(memo.negative && Date.now() - memo.ts > NEGATIVE_TTL_MS)) return memo.promise;
  const entry = requestPanorama(id);
  cache.set(key, entry);
  return entry.promise;
}
