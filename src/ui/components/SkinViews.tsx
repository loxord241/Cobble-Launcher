// Отрисовка скинов Minecraft (D34): голова — CSS-кроп текстуры, тело —
// canvas (пиксель-в-пиксель, без сглаживания). Скин без текстуры (офлайн)
// получает дефолтную 64×64 текстуру в духе классического Стива — единая
// узнаваемая палитра для всех (решение владельца 2026-10-03: раньше была
// случайная палитра от имени, выглядели пёстро). Наша пиксельная генерация,
// не текстуры Mojang.
import { useCallback, useEffect, useRef } from "react";
import type { SkinInfo, SkinModel } from "../../api/types";
import { t } from "../../i18n";

/** Палитра дефолтного скина: классический Стив (тёмные волосы, бирюзовая
 * футболка, синие штаны). */
const STEVE = {
  hair: "#3b2a17",
  skin: "#bd8b72",
  eye: "#4a3fa0",
  shirt: "#0e8585",
  pants: "#4040aa",
  shoe: "#5f5f5f",
};

// PERF#9 (аудит 2026-10-03): текстура дефолта одна на всех — после D54 она
// не зависит от имени, кэш Map на каждый ник дублировал одинаковый dataURL.
let defaultSkinUrl: string | null = null;

/** Дефолтная текстура 64×64 (canvas → data URL): Стив, одинаковый для всех. */
function defaultSkinTexture(): string {
  if (defaultSkinUrl) return defaultSkinUrl;

  const canvas = document.createElement("canvas");
  canvas.width = 64;
  canvas.height = 64;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";
  const px = (x: number, y: number, w: number, hgt: number, color: string) => {
    ctx.fillStyle = color;
    ctx.fillRect(x, y, w, hgt);
  };

  // Голова: верх/бока — волосы, лицо — кожа + глаза/рот (перед 8×8 в (8,8)).
  px(8, 0, 8, 8, STEVE.hair); // верх головы
  px(0, 8, 32, 8, "transparent"); // бока головы — ниже
  px(0, 8, 8, 8, STEVE.skin);
  px(16, 8, 8, 8, STEVE.skin);
  px(24, 8, 8, 8, STEVE.skin);
  px(8, 8, 8, 8, STEVE.skin); // лицо
  px(8, 8, 8, 2, STEVE.hair); // чёлка
  px(10, 11, 2, 2, "#ffffff"); // белки
  px(14, 11, 2, 2, "#ffffff");
  px(11, 11, 1, 2, STEVE.eye); // радужка
  px(14, 11, 1, 2, STEVE.eye);
  px(11, 14, 6, 1, shade(STEVE.skin, -45)); // рот
  // Тело (перед 8×12 в (20,20)): бирюзовая футболка.
  px(20, 20, 8, 12, STEVE.shirt);
  // Руки (перед 4×12 в (44,20)): рукав 2px, дальше кожа.
  px(44, 20, 4, 12, STEVE.skin);
  px(44, 20, 4, 3, STEVE.shirt);
  // Ноги (перед 4×12 в (4,20)): синие штаны + серые ботинки 2px.
  px(4, 20, 4, 12, STEVE.pants);
  px(4, 30, 4, 2, STEVE.shoe);
  // Левая рука/нога в современном layout 64×64: передние грани (36,52)/(20,52).
  // Без них SkinBody при modern=true читает прозрачность — фигура «однорукая»
  // (нашёл судья, 2026-10-03).
  px(20, 52, 4, 12, STEVE.pants);
  px(20, 62, 4, 2, STEVE.shoe);
  px(36, 52, 4, 12, STEVE.skin);
  px(36, 52, 4, 3, STEVE.shirt);

  const url = canvas.toDataURL("image/png");
  defaultSkinUrl = url;
  return url;
}

function shade(hex: string, delta: number): string {
  const n = parseInt(hex.slice(1), 16);
  const r = Math.min(255, Math.max(0, (n >> 16) + delta));
  const g = Math.min(255, Math.max(0, ((n >> 8) & 0xff) + delta));
  const b = Math.min(255, Math.max(0, (n & 0xff) + delta));
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, "0")}`;
}

/** Текстура для отрисовки: скин профиля или общий дефолт. */
function textureOf(_name: string, skin?: SkinInfo | null): string {
  return skin?.dataUrl ?? defaultSkinTexture();
}

/**
 * Голова персонажа: CSS-кроп передней грани (8,8) + слой шапки/волос (40,8).
 * 8×8 пикселей текстуры → size px (целочисленный масштаб против «мыла»).
 */
export function SkinHead({
  name,
  skin,
  size = 32,
  className,
}: {
  name: string;
  skin?: SkinInfo | null;
  size?: number;
  className?: string;
}) {
  const tex = textureOf(name, skin);
  const scale = Math.max(1, Math.round(size / 8));
  const px = 8 * scale;
  const layer = (x: number): React.CSSProperties => ({
    position: "absolute",
    inset: 0,
    backgroundImage: `url(${tex})`,
    backgroundSize: `${64 * scale}px ${64 * scale}px`,
    backgroundPosition: `${-x * scale}px ${-8 * scale}px`,
    imageRendering: "pixelated",
  });
  return (
    <span
      aria-hidden
      className={`relative inline-block shrink-0 overflow-hidden rounded-[3px] ${className ?? ""}`}
      style={{ width: px, height: px, backgroundColor: "rgba(128,128,128,0.15)" }}
    >
      <span style={layer(8)} />
      <span style={layer(40)} />
    </span>
  );
}

/**
 * Тело персонажа анфас (canvas 16×32 px-логики, масштаб 4). Поддерживает и
 * современные 64×64, и легаси 64×32 текстуры (руки/ноги легаси зералим).
 */
export function SkinBody({
  name,
  skin,
  className,
}: {
  name: string;
  skin?: SkinInfo | null;
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const tex = textureOf(name, skin);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const img = new Image();
    img.onload = () => {
      const S = 4; // масштаб: 16×32 логики → 64×128 px
      canvas.width = 16 * S;
      canvas.height = 32 * S;
      ctx.imageSmoothingEnabled = false;
      // Легаси-тексты 64×32 не имеют отдельных левых рук/ног — рисуем правые
      // зеркально (так делает и сама игра).
      const modern = img.height >= 64;
      // Координаты передних граней (x, y, w, h) в текстуре.
      const head = [8, 8, 8, 8];
      const hat = [40, 8, 8, 8];
      const body = [20, 20, 8, 12];
      const armR = [44, 20, 4, 12];
      const armL = modern ? [36, 52, 4, 12] : armR;
      const legR = [4, 20, 4, 12];
      const legL = modern ? [20, 52, 4, 12] : legR;
      const draw = (
        [sx, sy, sw, sh]: number[],
        dx: number,
        dy: number,
        mirror = false,
      ) => {
        ctx.save();
        ctx.translate((dx + (mirror ? sw : 0)) * S, dy * S);
        if (mirror) ctx.scale(-1, 1);
        ctx.drawImage(img, sx, sy, sw, sh, 0, 0, sw * S, sh * S);
        ctx.restore();
      };
      // Анфас: левая рука игрока — слева от зрителя.
      draw(armL, 0, 8, !modern);
      draw(body, 4, 8);
      draw(armR, 12, 8);
      draw(legL, 4, 20, !modern);
      draw(legR, 8, 20);
      draw(head, 4, 0);
      draw(hat, 4, 0);
    };
    img.src = tex;
  }, [tex]);

  return (
    <canvas
      ref={ref}
      aria-hidden
      className={className ?? ""}
      style={{ imageRendering: "pixelated", width: 64, height: 128 }}
    />
  );
}

// ---------- D59: полный персонаж с вращением (страница «Скины») ----------

/** Прямоугольник в текстуре 64×64: [sx, sy, sw, sh]. */
type TexRect = [number, number, number, number];

/** Четыре боковые грани бокса в развёртке скина (плюс верх/низ не рисуем — камера прямо). */
interface BoxFaces {
  front: TexRect;
  back: TexRect;
  /** Щека ПРАВОЙ руки персонажа (у зрителя слева анфас), регион (0,8)-семейства. */
  right: TexRect;
  left: TexRect;
}

const HEAD_FACES: BoxFaces = {
  front: [8, 8, 8, 8],
  back: [24, 8, 8, 8],
  right: [0, 8, 8, 8],
  left: [16, 8, 8, 8],
};
const HAT_FACES: BoxFaces = {
  front: [40, 8, 8, 8],
  back: [56, 8, 8, 8],
  right: [32, 8, 8, 8],
  left: [48, 8, 8, 8],
};
const BODY_FACES: BoxFaces = {
  front: [20, 20, 8, 12],
  back: [32, 20, 8, 12],
  right: [16, 20, 4, 12],
  left: [28, 20, 4, 12],
};
const ARM_R_Y = 20;
const ARM_L_Y = 52;
const LEG_R_FACES: BoxFaces = {
  front: [4, 20, 4, 12],
  back: [12, 20, 4, 12],
  right: [0, 20, 4, 12],
  left: [8, 20, 4, 12],
};
const LEG_L_FACES: BoxFaces = {
  front: [20, 52, 4, 12],
  back: [28, 52, 4, 12],
  right: [16, 52, 4, 12],
  left: [24, 52, 4, 12],
};

/** Развёртка руки: [front][right][left][back] подряд по x (slim — 3 px). */
function armFaces(slim: boolean, y: number): BoxFaces {
  const w = slim ? 3 : 4;
  const frontX = y === ARM_L_Y ? 36 : 44;
  return {
    front: [frontX, y, w, 12],
    back: [frontX + 2 * w, y, w, 12],
    right: [frontX - w, y, w, 12],
    left: [frontX + w, y, w, 12],
  };
}

interface BoxPart {
  /** Центр бокса по горизонтали, лог. px (0 — ось персонажа; право персонажа — минус). */
  x: number;
  /** Верх бокса от макушки, лог. px. */
  top: number;
  w: number;
  h: number;
  d: number;
  faces: BoxFaces;
  /** Легаси 64×32: левую конечность зералим из правой (как в SkinBody). */
  mirror?: boolean;
}

/**
 * Полный персонаж с вращением (D59): псевдо-3D проекция боксов на canvas.
 * Математика UV — из SkinBody (те же регионы передних граней; легаси 64×32
 * зералим), добавлены бока/спина/шапка. Поворот перетаскиванием по X вокруг
 * вертикальной оси; ортографическая проекция без наклона камеры, поэтому
 * каждая грань остаётся прямоугольником — рисуем её drawImage без искажений.
 */
export function FullSkinPreview({
  name,
  skin,
  model = "classic",
  scale = 8,
  className,
}: {
  name: string;
  skin?: SkinInfo | null;
  model?: SkinModel;
  /** Экранных пикселей на логический px текстуры. */
  scale?: number;
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const yawRef = useRef(0);
  const dragRef = useRef<{ pointerId: number; startX: number; startYaw: number } | null>(null);
  const tex = textureOf(name, skin);
  // Персонаж 16×32 лог.px; запас по ширине — чтобы голова при повороте не резалась.
  const W = 20;
  const H = 33;

  const draw = useCallback(() => {
    const canvas = ref.current;
    const img = imgRef.current;
    if (!canvas || !img) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const S = scale;
    canvas.width = W * S;
    canvas.height = H * S;
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    // Легаси-текстуры 64×32 не имеют отдельных левых рук/ног — зералим правые.
    const modern = img.height >= 64;
    const slim = model === "slim";
    const parts: BoxPart[] = [
      { x: -(4 + (slim ? 1.5 : 2)), top: 8, w: slim ? 3 : 4, h: 12, d: 4, faces: armFaces(slim, ARM_R_Y) },
      { x: 4 + (slim ? 1.5 : 2), top: 8, w: slim ? 3 : 4, h: 12, d: 4, faces: armFaces(slim, ARM_L_Y), mirror: !modern },
      { x: 0, top: 8, w: 8, h: 12, d: 4, faces: BODY_FACES },
      { x: -2, top: 20, w: 4, h: 12, d: 4, faces: LEG_R_FACES },
      { x: 2, top: 20, w: 4, h: 12, d: 4, faces: modern ? LEG_L_FACES : LEG_R_FACES, mirror: !modern },
      { x: 0, top: 0, w: 8, h: 8, d: 8, faces: HEAD_FACES },
      { x: 0, top: 0, w: 8, h: 8, d: 8, faces: HAT_FACES },
    ];

    const yaw = yawRef.current;
    const cos = Math.cos(yaw);
    const sin = Math.sin(yaw);
    const cx = (W / 2) * S;
    const margin = Math.round(S);

    // Художник: сначала дальние боксы. z детали после поворота: -x·sin (z центров = 0).
    const ordered = parts
      .map((p, i) => ({ p, i, depth: -p.x * sin }))
      .sort((a, b) => a.depth - b.depth || a.i - b.i);

    for (const { p } of ordered) {
      // Грани с наружной нормалью; для каждой — горизонтальный отрезок (x1,z1)→(x2,z2),
      // начало отрезка совпадает с u=0 развёртки (проверено по шаблону скина).
      const hw = p.w / 2;
      const hd = p.d / 2;
      const faces: { nz: number; nx: number; rect: TexRect; seg: [number, number, number, number] }[] = [
        { nz: 1, nx: 0, rect: p.faces.front, seg: [-hw, hd, hw, hd] },
        { nz: -1, nx: 0, rect: p.faces.back, seg: [hw, -hd, -hw, -hd] },
        { nz: 0, nx: -1, rect: p.faces.right, seg: [-hw, -hd, -hw, hd] },
        { nz: 0, nx: 1, rect: p.faces.left, seg: [hw, hd, hw, -hd] },
      ];
      const topPx = margin + Math.round(p.top * S);
      const hPx = Math.round(p.h * S);
      // Легаси: левую конечность зералим целиком (регионы правой).
      const mirror = p.mirror === true;
      for (const f of faces) {
        // Видимость: повернутая нормаль смотрит на зрителя.
        const facing = f.nz * cos - f.nx * sin;
        if (facing <= 0.01) continue;
        const px1 = cx + (f.seg[0] * cos + f.seg[1] * sin) * S;
        const px2 = cx + (f.seg[2] * cos + f.seg[3] * sin) * S;
        const left = Math.round(Math.min(px1, px2));
        const width = Math.round(Math.max(px1, px2)) - left;
        if (width <= 0) continue;
        const [sx, sy, sw, sh] = f.rect;
        if (mirror) {
          ctx.save();
          ctx.translate(left + width, 0);
          ctx.scale(-1, 1);
          ctx.drawImage(img, sx, sy, sw, sh, 0, topPx, width, hPx);
          ctx.restore();
        } else {
          ctx.drawImage(img, sx, sy, sw, sh, left, topPx, width, hPx);
        }
      }
    }
  }, [model, scale]);

  useEffect(() => {
    const img = new Image();
    img.onload = () => {
      imgRef.current = img;
      draw();
    };
    img.src = tex;
  }, [tex, draw]);

  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    dragRef.current = { pointerId: e.pointerId, startX: e.clientX, startYaw: yawRef.current };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;
    yawRef.current = drag.startYaw + (e.clientX - drag.startX) * 0.012;
    draw();
  };
  const onPointerEnd = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (dragRef.current?.pointerId === e.pointerId) dragRef.current = null;
  };

  return (
    <div className={className}>
      <canvas
        ref={ref}
        aria-hidden
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerEnd}
        onPointerCancel={onPointerEnd}
        style={{
          imageRendering: "pixelated",
          width: W * scale,
          height: H * scale,
          touchAction: "none",
          cursor: "grab",
        }}
        className="mx-auto block"
      />
      <div className="mt-2 text-center text-xs text-text-muted">{t("skins.rotateHint")}</div>
    </div>
  );
}
