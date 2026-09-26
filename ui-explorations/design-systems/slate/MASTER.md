# Design System Master File

> **LOGIC:** When building a specific page, first check `design-system/pages/[page-name].md`.
> If that file exists, its rules **override** this Master file.
> If not, strictly follow the rules below.

---

**Project:** slate
**Generated:** 2026-09-26 20:23:03
**Category:** Developer Tool / IDE
**Design Dials:** Variance 2/10 (Centered / Minimal) | Motion 2/10 (Subtle) | Density 8/10 (Dense / Dashboard)

---

## Global Rules

### Color Palette

| Role | Hex | CSS Variable |
|------|-----|--------------|
| Primary | `#1E293B` | `--color-primary` |
| On Primary | `#FFFFFF` | `--color-on-primary` |
| Secondary | `#334155` | `--color-secondary` |
| On Secondary | `#FFFFFF` | `--color-on-secondary` |
| Accent/CTA | `#22C55E` | `--color-accent` |
| On Accent/CTA | `#0F172A` | `--color-on-accent` |
| Background | `#0F172A` | `--color-background` |
| Foreground | `#F8FAFC` | `--color-foreground` |
| Card | `#1B2336` | `--color-card` |
| Card Foreground | `#F8FAFC` | `--color-card-foreground` |
| Muted | `#272F42` | `--color-muted` |
| Muted Foreground | `#94A3B8` | `--color-muted-foreground` |
| Border | `#475569` | `--color-border` |
| Destructive | `#EF4444` | `--color-destructive` |
| On Destructive | `#000000` | `--color-on-destructive` |
| Ring | `#FFFFFF` | `--color-ring` |

**Color Notes:** Code dark + run green

### Typography

- **Heading Font:** JetBrains Mono
- **Body Font:** IBM Plex Sans
- **Mood:** code, developer, technical, precise, functional, hacker
- **Google Fonts:** [JetBrains Mono + IBM Plex Sans](https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@300;400;500;600;700&family=JetBrains+Mono:wght@400;500;600;700&display=swap)

**CSS Import:**
```css
@import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@300;400;500;600;700&family=JetBrains+Mono:wght@400;500;600;700&display=swap');
```

### Spacing Variables

*Density: 8/10 — Dense / Dashboard*

| Token | Value | Usage |
|-------|-------|-------|
| `--space-xs` | `2px` / `0.125rem` | Tight gaps |
| `--space-sm` | `4px` / `0.25rem` | Icon gaps, inline spacing |
| `--space-md` | `8px` / `0.5rem` | Standard padding |
| `--space-lg` | `12px` / `0.75rem` | Section padding |
| `--space-xl` | `16px` / `1rem` | Large gaps |
| `--space-2xl` | `24px` / `1.5rem` | Section margins |
| `--space-3xl` | `32px` / `2rem` | Hero padding |

### Shadow Depths

| Level | Value | Usage |
|-------|-------|-------|
| `--shadow-sm` | `0 1px 2px rgba(0,0,0,0.05)` | Subtle lift |
| `--shadow-md` | `0 4px 6px rgba(0,0,0,0.1)` | Cards, buttons |
| `--shadow-lg` | `0 10px 15px rgba(0,0,0,0.1)` | Modals, dropdowns |
| `--shadow-xl` | `0 20px 25px rgba(0,0,0,0.15)` | Hero images, featured cards |

---

## Component Specs

### Buttons

```css
/* Primary Button */
.btn-primary {
  background: #22C55E;
  color: white;
  padding: 12px 24px;
  border-radius: 8px;
  font-weight: 600;
  transition: all 200ms ease;
  cursor: pointer;
}

.btn-primary:hover {
  opacity: 0.9;
  transform: translateY(-1px);
}

/* Secondary Button */
.btn-secondary {
  background: transparent;
  color: #1E293B;
  border: 2px solid #1E293B;
  padding: 12px 24px;
  border-radius: 8px;
  font-weight: 600;
  transition: all 200ms ease;
  cursor: pointer;
}
```

### Cards

```css
.card {
  background: #0F172A;
  border-radius: 12px;
  padding: 24px;
  box-shadow: var(--shadow-md);
  transition: all 200ms ease;
  cursor: pointer;
}

.card:hover {
  box-shadow: var(--shadow-lg);
  transform: translateY(-2px);
}
```

### Inputs

```css
.input {
  padding: 12px 16px;
  border: 1px solid #E2E8F0;
  border-radius: 8px;
  font-size: 16px;
  transition: border-color 200ms ease;
}

.input:focus {
  border-color: #1E293B;
  outline: none;
  box-shadow: 0 0 0 3px #1E293B20;
}
```

### Modals

```css
.modal-overlay {
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(4px);
}

.modal {
  background: white;
  border-radius: 16px;
  padding: 32px;
  box-shadow: var(--shadow-xl);
  max-width: 500px;
  width: 90%;
}
```

---

## Style Guidelines

**Style:** Minimalism & Swiss Style

**Keywords:** Clean, simple, spacious, functional, white space, high contrast, geometric, sans-serif, grid-based, essential

**Best For:** Enterprise apps, dashboards, documentation sites, SaaS platforms, professional tools

**Key Effects:** Subtle hover (200-250ms), smooth transitions, sharp shadows if any, clear type hierarchy, fast loading

### Page Pattern

**Pattern Name:** FAQ/Documentation Landing

- **Conversion Strategy:** Reduce support tickets. Track search analytics. Show related articles. Contact escalation path.
- **CTA Placement:** Search bar prominent + Contact CTA for unresolved questions
- **Section Order:** Hero with search bar > Popular categories > FAQ accordion > Contact/support CTA

---

## Motion

**Scroll Reveal** (Subtle) — Trigger: scroll (viewport enter) | Duration: 300-400ms | Easing: `power1.out`

```js
gsap.from(el, { opacity: 0, y: 12, duration: 0.35, ease: 'power1.out', scrollTrigger: { trigger: el, start: 'top 90%', toggleActions: 'play none none reverse' } });
```

**Framework notes:** Requires the ScrollTrigger plugin registered once via gsap.registerPlugin(ScrollTrigger); Use matchMedia('(prefers-reduced-motion: reduce)') to skip non-essential motion and render the final state immediately

- ✅ Keep the y offset small (8-16px) so it reads as a fade, not a slide
- ❌ Don't reveal below-the-fold content needed for SEO/crawlers as invisible-by-default without a no-JS fallback
- ⚡ toggleActions 'play none none reverse' avoids re-triggering on every scroll direction change

---

## Anti-Patterns (Do NOT Use)

- ❌ Light mode default
- ❌ Slow performance

### Additional Forbidden Patterns

- ❌ **Emojis as icons** — Use SVG icons (Heroicons, Lucide, Simple Icons)
- ❌ **Missing cursor:pointer** — All clickable elements must have cursor:pointer
- ❌ **Layout-shifting hovers** — Avoid scale transforms that shift layout
- ❌ **Low contrast text** — Maintain 4.5:1 minimum contrast ratio
- ❌ **Instant state changes** — Always use transitions (150-300ms)
- ❌ **Invisible focus states** — Focus states must be visible for a11y

---

## Pre-Delivery Checklist

Before delivering any UI code, verify:

- [ ] No emojis used as icons (use SVG instead)
- [ ] All icons from consistent icon set (Heroicons/Lucide)
- [ ] `cursor-pointer` on all clickable elements
- [ ] Hover states with smooth transitions (150-300ms)
- [ ] Light mode: text contrast 4.5:1 minimum
- [ ] Focus states visible for keyboard navigation
- [ ] `prefers-reduced-motion` respected
- [ ] Responsive: 375px, 768px, 1024px, 1440px
- [ ] No content hidden behind fixed navbars
- [ ] No horizontal scroll on mobile

---

## Арт-дирекция варианта (финальные токены — источник для вёрстки)

**Концепт:** «Сланец». **Продукт (вотмард):** «Графит». Тёмный графит, монохром + один холодный голубой акцент; инстансы — списки-строки, версии моноширинным. Режим Operate: сканируемость через ровные строки и выровненные колонки.

**Диски (variance 2, motion 2, density 8):** сдержанный центр — интерфейс-инструмент; плотность 8 держит 5 инстансов и модпаки в первом экране.

**Оверрайды базы (и почему):** фон/палитра заданы брифом варианта — графит #16181c вместо сланцево-синего #0F172A; акцент зелёный #22C55E заменён на холодный голубой #6cb2fa (бриф; зелёный остаётся только статусом «работает»); шрифты базы JetBrains Mono + IBM Plex Sans заменены на Inter (бриф) + JetBrains Mono для версий и чисел.

**Reduced-motion:** отключаются transition у hover/подъём строк и shimmer скелетона; остаётся статичное затемнение .is-hover/.is-active — вся информация живёт в цвете/тексте, не в движении.

```css
:root{
  --bg:#16181c; --surface:#1d2026; --surface-2:#252a31;
  --text:#e9ebee; --text-muted:#a3abb5;
  --border:#2b313a; --border-strong:#3b424d;
  --accent:#6cb2fa; --on-accent:#0d1117; --accent-soft:rgba(108,178,250,.12);
  --danger:#f28b82; --on-danger:#1a0f0d; --ok:#5ac48f;
  --font-ui:'Inter',system-ui,sans-serif; --font-display:'Inter',system-ui,sans-serif; --font-mono:'JetBrains Mono',monospace;
  --r-s:4px; --r-m:8px; --r-l:10px;
  --s-1:4px; --s-2:8px; --s-3:12px; --s-4:16px; --s-5:24px; --s-6:32px;
  --t-fast:150ms; --t-slow:250ms; --ease:cubic-bezier(.2,0,0,1);
  --shadow-1:0 1px 2px rgba(0,0,0,.40); --shadow-2:0 8px 24px rgba(0,0,0,.45);
}
```


---

## Развитие (slate-pro): темы оформления

Файл `slate-pro.html` — развитие концепта: четвёртый экран «Настройки → Темы» (каталог по образцу
CurseForge: группы «Классические / Для дальтоников / Премиум», карточки-превью, галочка выбранной,
замки премиума) и живое переключение тем через `data-theme` на `<html>`. Базовые токены выше —
это тема «Сланец · Тёмная».

Дополнительные темы (контраст пар проверен тем же скриптом, PASS по каждой):

| Тема | Ключевые токены (отличия от базы) | Смысл |
|------|-----------------------------------|-------|
| Сланец · Светлая | bg #f4f5f7, surface #fff, text #1b1e24, muted #57616f, accent #2b6fb8, on-accent #fff, ok #2e7d51, danger #b3342a | дневной режим, акцент затемнён для 4.5:1 |
| Дальтоник · Тёмная | accent #5aa2ff, ok #5aa2ff (синий статус), danger #e58f3d (оранжевый) | зелёный/красный заменены на сине-оранжевую пару |
| Дальтоник · Светлая | светлые токены + ok #2b6fb8, danger #9a521b | то же на светлой базе |

Премиум-темы (только превью-карточки с замком, не применяются): Гроза (#23203a/#9a86ff),
Хвоя (#16211c/#4fc487), Кармин (#2a1618/#ff7a6e), Янтарь (#2a2118/#f0a52c) — свотчы лежат в
`:root` как `--pz-*`/`--pv-*` (это контент карточек, не UI-токены).
