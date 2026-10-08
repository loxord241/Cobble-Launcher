// Дефолтный арт инстанса (D35 → D63): у инстанса без своего арта/иконки —
// пейзаж в блочном «майнкрафтовом» стиле вместо заглушки. Выбор детерминирован
// от ключа (id инстанса): один инстанс — всегда один пейзаж, разные — разные.
// Пейзажи — оригинальная генерация (scripts-скриптом, без ассетов Mojang),
// лежат локально и бандлятся вместе с приложением (офлайн-правило владельца).

import plains from "../assets/instance-art/plains.png";
import plains2 from "../assets/instance-art/plains-2.png";
import cherry from "../assets/instance-art/cherry.png";
import cherry2 from "../assets/instance-art/cherry-2.png";
import desert from "../assets/instance-art/desert.png";
import desert2 from "../assets/instance-art/desert-2.png";
import nightLake from "../assets/instance-art/night_lake.png";
import nightLake2 from "../assets/instance-art/night_lake-2.png";
import snowPeaks from "../assets/instance-art/snow_peaks.png";
import snowPeaks2 from "../assets/instance-art/snow_peaks-2.png";
import sunsetMountains from "../assets/instance-art/sunset_mountains.png";
import sunsetMountains2 from "../assets/instance-art/sunset_mountains-2.png";

// По два варианта каждой сцены: у карточек без арта хватает разнобоя
// даже при 7+ инстансах без своих картинок.
const LANDSCAPES = [
  plains, plains2, cherry, cherry2, desert, desert2,
  nightLake, nightLake2, snowPeaks, snowPeaks2, sunsetMountains, sunsetMountains2,
];

/** FNV-1a 32 бит — тот же хэш, что в ядре (util/fnv.rs): стабильный
 * «ключ → индекс» между сессиями. */
function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** URL дефолтного пейзажа для инстанса (детерминированно от seed). */
export function defaultArtwork(seed: string): string {
  return LANDSCAPES[fnv1a(seed || "mcl") % LANDSCAPES.length];
}
