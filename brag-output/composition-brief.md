# Hyperframes Composition Brief: Cobble Launcher hero loop

## Objective

Создать бесшовно лупящийся кинематографичный hero-ролик (фон главного экрана лаунчера): один процедурный воксельный мир Minecraft-стиля на рассвете, пролёт камеры через него, возврат в первый кадр. Премиально, тихо, без текста и звука.

## Output

- Composition directory: `brag-output/composition/`
- Rendered video: `brag-output/brag.mp4` (+ веб-копия для лаунчера в `src/assets/hero/`)
- Format: landscape — 1920×1080 (16:9)
- Duration: 10.0 s, 30 fps, без аудиодорожки

## Source Material

- Project root: `C:\Users\Admin\.zcode\workspace\default\mc-launcher-v2`
- Primary files read: `README.md`, `package.json`, `src/theme/tokens.css`, `src/ui/pages/HomePage.tsx`
- Product name: Cobble Launcher
- Tagline / strongest claim: «Быстрый и честный лаунчер Minecraft для Windows»
- Key visual moment: не UI, а сам мир Minecraft (бриф владельца: без текста/UI/лого)
- Copy that must appear verbatim: **нет** — текст в ролике запрещён брифом

## Creative Direction

- Tone preset: cinematic
- Creative direction: «тихий премиальный игровой фильм», voxel-diorama в золотой час
- Interpretation: темп задаётся камерой (покой → полёт → покой), не количеством событий; длинные непрерывные движения; ноль глифов
- Angle: мир продукта = сам Minecraft; ролик погружает в атмосферу «пора запускать», живя в палитре лаунчера «Сланец»
- Hook: вайд-панорама рассвета над озером, почти статичная камера начинает дыхание цикла
- Outro: нет — последний кадр тождествен первому (loop)
- Avoid:
  - Любой текст, логотипы, UI-элементы, водяные знаки
  - Звук любого рода
  - «Дешёвый AI-видео»-look: плавные неразличимые формы, морфинг, непоследовательный мир
  - Перегруз событиями; стампед животных, взрывы, толпы

## Visual Identity

- Background (Сланец): `#16181c` → зенит неба
- Accent (голубой): `#6cb2fa` → средний тон неба
- Success (зелёный): `#5ac48f` → трава, листва
- Warning (тёплый): `#f0a52c` → горизонт, солнце, свет факелов
- Display/Body font: нет (текст запрещён)
- Visual references: воксельная эстетика Minecraft (только кубы), «Сланец»-палитра лаунчера; НЕ копировать ассеты Mojang — всё генерируется процедурно

## Storyboard

Use the storyboard in `brag-output/brag-plan.md` as the creative contract.

Scene summary (мир един, все шоты в нём, сид фиксирован):

1. «Рассвет над долиной» — 3.0s — вайд с высоты: озеро-зеркало, отражение солнца, деревня вдали, снежный хребет; медленный push из покоя. FOV 42.
2. «Низкий пролёт» — 3.0s — срез на 2–3 блока над водой через озеро, набор высоты над кронами к деревне; пик скорости. FOV 55.
3. «Деревня и раскрытие» — 4.0s — проход мимо домика/грядок/фигурки, крейн-подъём и отлёт по дуге, замирание в покое ровно на позе кадра 0. FOV 48→42.

Loop-контракт: все периодические анимации — целое число периодов на T=10; облака — суммарный дрейф ровно один период решётки с фейдом у краёв; камера — точное равенство позы и нулевая скорость на шве.

## Audio

- Audio role: intentional silence (бриф владельца: «Без звука»)
- Music: none — отключено владельцем
- SFX: none
- Аудио-механики /brag (beat sync, audio-reactive, SFX) не применяются

## Hyperframes Instructions

Load the composition-building Hyperframes domain skills — done (`hyperframes-core`, `hyperframes-animation`, `hyperframes-creative`, `hyperframes-keyframes`, `hyperframes-cli`).

Runtime: **three.js adapter** (`hyperframes-animation/adapters/three.md`) — full-bleed canvas, `hf-seek` driven, `data-duration="10"` on root (обязателен: three-адаптер не выводит длительность сам), `renderer.setSize(1920,1080,false)`, `pixelRatio 1`. Тяжёлую процедурную генерацию мира зарегистрировать через `window.__hf.buildReady`. three.js подключить **локально** (скачать в `assets/three/`), чтобы рендер не зависел от сети. Сид-PRNG (mulberry32), ноль `Math.random`/`Date.now`/`performance.now`. Каталог DOM-эффектов не применим (движение — не DOM, а bespoke WebGL-сцена; каталожные блоки не покрывают three.js-пролёты).

Requirements:

- Мир: террейн ~128×128 блоков (fBm), озеро, снежный хребет, ~60–80 деревьев, деревня-виньетка (дом, грядки, факелы, дым), фигурка персонажа, плоские облака, птицы, пылинки в контровом свете.
- Свет: низкое тёплое солнце + hemisphere + тёплый туман + мягкие тени (PCFSoft), ACESFilmic tone mapping.
- Небо: градиентный sky-dome шейдер с дизерингом (против бандинга), солнце-спрайт.
- Камера: 3 шота, кат-точки на кадрах 90/180, согласованность вектора движения на кате, финальная поза кадра 0 с нулевой скоростью.
- Ни одного текстового глифа, логотипа, UI в кадре.
- `npx hyperframes check` — единственный гейт перед рендером.
- Рендер: `--quality delivery`, 30 fps, 1920×1080, без аудио.
- Пост-QA по чеклисту brag-plan.md (луп-шов, артефакты, стилистика) — владелец требует перегенерацию при существенных проблемах.

## Deliverables

- `brag-output/brag.mp4` — мастервидео
- `brag-output/brag.jpg` — постер = кадр 0
- `src/assets/hero/hero-loop.mp4` + `hero-loop.jpg` — веб-оптимизированная копия для UI лаунчера (H.264, yuv420p, +faststart, без звука)
- `brag-output/share-copy.txt`
