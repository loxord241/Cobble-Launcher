// Безопасный рендер подмножества Markdown (D39): тело страницы проекта
// Modrinth приходит из сети, поэтому никакого dangerouslySetInnerHTML —
// весь текст идёт через JSX-эскейпинг, сырой HTML остаётся обычным текстом.
// URL ссылок/картинок — только http/https: javascript:/data: и прочие
// схемы превращают конструкцию обратно в текст.
//
// Поддержаны reference-ссылки `[текст][1]` + определения `[1]: url` — авторы
// Modrinth пишут ими почти каждое второе предложение; без них тело рендерится
// сырыми скобками. Необъявленная ссылка показывается просто текстом (без
// «[16]» в середине предложения).
import type { ReactNode } from "react";

/** Разрешены только веб-URL; относительные и не-http схемы — текстом. */
function safeUrl(url: string): string | null {
  return /^https?:\/\//i.test(url) ? url : null;
}

/** Определение reference-ссылки: `[1]: https://…` (возможен `<url>` и title). */
const REF_DEF_RE = /^\s{0,3}\[([^\]]+)\]:\s*<?([^>\s]+)>?\s*(?:"([^"]*)")?\s*$/;

/** Собрать определения ссылок и выкинуть их строки из тела (вне ```-заборов). */
function collectRefs(lines: string[]): { body: string[]; defs: Map<string, string> } {
  const defs = new Map<string, string>();
  const body: string[] = [];
  let inFence = false;
  for (const line of lines) {
    if (FENCE_RE.test(line)) {
      inFence = !inFence;
      body.push(line);
      continue;
    }
    if (!inFence) {
      const d = line.match(REF_DEF_RE);
      if (d) {
        const url = safeUrl(d[2]);
        if (url) defs.set(d[1].trim().toLowerCase(), url);
        continue; // строка-определение в рендер не идёт
      }
    }
    body.push(line);
  }
  return { body, defs };
}

/**
 * `[текст][ключ]` → `[текст](url)` по определениям. Неизвестный ключ — просто
 * текст ссылки без хвоста `[ключ]`: сырая сноска в середине предложения
 * читаемости не добавляет.
 */
function resolveRefs(text: string, defs: Map<string, string>): string {
  if (defs.size === 0) return text;
  return text.replace(
    /\[([^\]]+)\]\[([^\]]+)\]/g,
    (_whole, label: string, ref: string) => {
      const url = defs.get(ref.trim().toLowerCase());
      return url ? `[${label}](${url})` : label;
    },
  );
}

type InlineKind = "image-link" | "image" | "link" | "bold" | "italic" | "code";

// Порядок важен: ** раньше одиночного * — иначе жирный «съедается» курсивом.
// image-link — фирменные бейджи Modrinth `[![alt](img)](target)`: без него
// внешний линк рисует сырой `![alt](url)` текстом вместо картинки.
const INLINE: { re: RegExp; kind: InlineKind }[] = [
  { re: /\[!\[([^\]]*)\]\(([^)\s]+)\)\]\(([^)\s]+)\)/, kind: "image-link" },
  { re: /!\[([^\]]*)\]\(([^)\s]+)\)/, kind: "image" },
  { re: /\[([^\]]+)\]\(([^)\s]+)\)/, kind: "link" },
  { re: /\*\*([^*]+)\*\*/, kind: "bold" },
  { re: /\*([^*\s][^*]*)\*/, kind: "italic" },
  { re: /`([^`]+)`/, kind: "code" },
];

/** Строка состоит только из картинок/бейджей (для склейки их в один ряд). */
function imageOnlyLine(line: string): boolean {
  const t = line.trim();
  if (!t) return true;
  const stripped = t
    .replace(/\[!\[[^\]]*\]\([^)\s]*\)\]\([^)\s]*\)/g, "")
    .replace(/!\[[^\]]*\]\([^)\s]*\)/g, "");
  return stripped.trim() === "";
}

/** Инлайн-разметка → элементы; рекурсия даёт вложенность (жирный в ссылке). */
function renderInline(text: string, keyBase: string, defs: Map<string, string>): ReactNode[] {
  const out: ReactNode[] = [];
  let rest = resolveRefs(text, defs);
  let n = 0;
  while (rest) {
    let best: { index: number; match: RegExpMatchArray; kind: InlineKind } | null = null;
    for (const { re, kind } of INLINE) {
      const m = rest.match(re);
      if (m && m.index !== undefined && (best === null || m.index < best.index)) {
        best = { index: m.index, match: m, kind };
      }
    }
    if (!best) { out.push(rest); break; } // дальше разметки нет — хвост текстом
    if (best.index > 0) out.push(rest.slice(0, best.index));
    const k = `${keyBase}-${n++}`;
    const [, inner, rawUrl] = best.match;
    const url = safeUrl(rawUrl);
    if (best.kind === "image-link") {
      // Бейдж-ссылка: картинка (m[2]) внутри ссылки (m[3]) — цель ссылки берём
      // именно из m[3], а не из URL картинки. Запрещённый URL картинки
      // превращает конструкцию в текст; запрещённый/отсутствующий target —
      // картинка остаётся, но без ссылки.
      const img = safeUrl(best.match[2]);
      const target = safeUrl(best.match[3]);
      out.push(
        target && img ? (
          <a key={k} href={target} target="_blank" rel="noreferrer" className="inline-block">
            <img src={img} alt={inner} loading="lazy" className="rounded bg-surface-2" />
          </a>
        ) : img ? (
          <img key={k} src={img} alt={inner} loading="lazy" className="rounded bg-surface-2" />
        ) : (
          <span key={k}>{best.match[0]}</span>
        ),
      );
    } else if (best.kind === "image") {
      // Запрещённый URL — конструкция остаётся текстом, ничего не грузим.
      out.push(
        url ? (
          <img key={k} src={url} alt={inner} loading="lazy" className="rounded bg-surface-2" />
        ) : (
          <span key={k}>{best.match[0]}</span>
        ),
      );
    } else if (best.kind === "link") {
      out.push(
        url ? (
          <a key={k} href={url} target="_blank" rel="noreferrer" className="text-accent underline">
            {renderInline(inner, k, defs)}
          </a>
        ) : (
          <span key={k}>{best.match[0]}</span>
        ),
      );
    } else if (best.kind === "bold") {
      out.push(<strong key={k}>{renderInline(inner, k, defs)}</strong>);
    } else if (best.kind === "italic") {
      out.push(<em key={k}>{renderInline(inner, k, defs)}</em>);
    } else {
      out.push(<code key={k} className="rounded bg-surface-2 px-1 font-mono text-[13px]">{inner}</code>);
    }
    rest = rest.slice(best.index + best.match[0].length);
  }
  return out;
}

// Якоря блочных конструкций; `---` и `***` — горизонтальный разделитель.
const FENCE_RE = /^\s*```/;
const HEADING_RE = /^(#{1,6})\s+(.+)$/;
const HR_RE = /^\s*(?:-{3,}|\*{3,})\s*$/;
const LIST_RE = /^\s*([-*]|\d+\.)\s+(.+)$/;
const QUOTE_RE = /^\s*>\s?(.*)$/;

// Классы заголовков h1..h6 (индекс = уровень − 1).
const HEADING_CLS = [
  "mt-4 text-xl font-bold",
  "mt-4 text-lg font-bold",
  "mt-3 text-base font-bold",
  "mt-3 text-sm font-bold",
  "mt-2 text-sm font-semibold",
  "mt-2 text-[13px] font-semibold",
];

/** Потоковый разбор построчно: каждый блок — свой элемент со стабильным ключом. */
export function Markdown({ text }: { text: string }) {
  const { body: lines, defs } = collectRefs(text.split("\n"));
  const blocks: ReactNode[] = [];
  // Подряд идущие «параграфы из одних картинок» (бейджи авторов) клеим в один
  // горизонтальный ряд: в столбик они выглядят чужеродными кнопками.
  let badgeRow: ReactNode[] | null = null;
  let seq = 0;

  const flushBadges = () => {
    if (badgeRow !== null) {
      const items = badgeRow;
      badgeRow = null;
      blocks.push(
        <div key={`badges-${seq++}`} className="mt-3 flex flex-wrap items-center gap-2 first:mt-0">
          {items}
        </div>,
      );
    }
  };

  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const k = `md-${seq++}`;

    // Блок кода: до закрывающей решётки — дословно, без инлайн-разметки.
    if (FENCE_RE.test(line)) {
      flushBadges();
      const buf: string[] = [];
      i++;
      while (i < lines.length && !FENCE_RE.test(lines[i])) buf.push(lines[i++]);
      i++; // закрывающую решётку пропускаем
      blocks.push(
        <pre key={k} className="mt-3 overflow-x-auto rounded-lg border border-border-app bg-bg p-3 font-mono text-[13px] leading-relaxed">
          <code>{buf.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    if (HR_RE.test(line)) {
      flushBadges();
      blocks.push(<hr key={k} className="mt-4 border-border-app" />);
      i++;
      continue;
    }

    const head = line.match(HEADING_RE);
    if (head) {
      flushBadges();
      const lvl = head[1].length;
      const Tag = `h${lvl}` as "h1";
      blocks.push(
        <Tag key={k} className={HEADING_CLS[lvl - 1]}>
          {renderInline(head[2], `${k}-i`, defs)}
        </Tag>,
      );
      i++;
      continue;
    }

    // Цитата: подряд идущие строки «> …» — один blockquote; переводы строк
    // сохраняем, чтобы авторские перечисления не слипались.
    if (QUOTE_RE.test(line)) {
      flushBadges();
      const buf: string[] = [];
      for (; i < lines.length; i++) {
        const q = lines[i].match(QUOTE_RE);
        if (!q) break;
        buf.push(q[1]);
      }
      blocks.push(
        <blockquote key={k} className="mt-3 whitespace-pre-wrap border-l-2 border-border-strong pl-3 text-text-muted">
          {renderInline(buf.join("\n"), `${k}-i`, defs)}
        </blockquote>,
      );
      continue;
    }

    // Список: подряд идущие пункты одного типа — ul/ol; смена маркера закрывает
    // список (вложенность не эмулируем — Modrinth-тела её почти не используют).
    const li0 = line.match(LIST_RE);
    if (li0) {
      flushBadges();
      const ordered = /\d/.test(li0[1]);
      const items: string[] = [];
      for (; i < lines.length; i++) {
        const li = lines[i].match(LIST_RE);
        if (!li || /\d/.test(li[1]) !== ordered) break;
        items.push(li[2]);
      }
      const ListTag = ordered ? "ol" : "ul";
      blocks.push(
        <ListTag key={k} className={`mt-3 space-y-1 pl-5 ${ordered ? "list-decimal" : "list-disc"}`}>
          {items.map((it, idx) => (
            <li key={`${k}-${idx}`}>{renderInline(it, `${k}-${idx}`, defs)}</li>
          ))}
        </ListTag>,
      );
      continue;
    }

    // Пустая строка — просто разделитель блоков.
    if (line.trim() === "") {
      i++;
      continue;
    }

    // Абзац: копим строки до пустой строки или начала другого блока; мягкие
    // переносы склеиваем пробелом (как в каноничном markdown).
    const buf: string[] = [];
    for (; i < lines.length; i++) {
      const cur = lines[i];
      if (
        cur.trim() === "" ||
        FENCE_RE.test(cur) ||
        HEADING_RE.test(cur) ||
        HR_RE.test(cur) ||
        LIST_RE.test(cur) ||
        QUOTE_RE.test(cur)
      )
        break;
      buf.push(cur.trim());
    }

    // Параграф из одних картинок/бейджей — копим в общий ряд, не в <p>.
    if (buf.every(imageOnlyLine)) {
      if (badgeRow === null) badgeRow = [];
      badgeRow.push(...renderInline(buf.join(" "), `${k}-i`, defs));
      continue;
    }
    flushBadges();
    blocks.push(
      <p key={k} className="mt-3 leading-relaxed first:mt-0">
        {renderInline(buf.join(" "), `${k}-i`, defs)}
      </p>,
    );
  }
  flushBadges();

  return <div className="text-sm leading-relaxed text-text">{blocks}</div>;
}
