// D63: панорама Minecraft как дефолтный арт. Ядро отдаёт data-URL PNG-грани
// куба (1024×1024) из собственных ассетов игры; здесь — кроп 4:3 в JPEG
// через canvas (карточка 4:3, объект ~50 КБ вместо 0.5 МБ) и кэш на сессию.

import { api } from "../api/client";

const ART_W = 640;
const ART_H = 480;
const cache = new Map<string, Promise<string | null>>();

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
        canvas.getContext("2d")?.drawImage(img, 0, sy, srcW, srcH, 0, 0, ART_W, ART_H);
        resolve(canvas.toDataURL("image/jpeg", 0.85));
      } catch {
        resolve(null); // tainted canvas и прочее — фронт покажет пиксель-пейзаж
      }
    };
    img.onerror = () => resolve(null);
    img.src = dataUrl;
  });
}

/** Готовый арт-URL инстанса: панорама (если игра скачана) или null. */
export function instancePanorama(id: string): Promise<string | null> {
  const memo = cache.get(id);
  if (memo) return memo;
  const p = api
    .panoramaArt(id)
    .then((url) => (url ? cropToCard(url) : null))
    .catch(() => null);
  cache.set(id, p);
  return p;
}
