# Design System Master File

> **LOGIC:** When building a specific page, first check `design-system/pages/[page-name].md`.
> If that file exists, its rules **override** this Master file.
> If not, strictly follow the rules below.

---

**Project:** retro96
**Generated:** 2026-09-26 20:24:44
**Category:** Brewery/Winery
**Design Dials:** Variance 8/10 (Bold / Asymmetric) | Motion 3/10 (Subtle) | Density 7/10 (Standard)

---

## Global Rules

### Color Palette

| Role | Hex | CSS Variable |
|------|-----|--------------|
| Primary | `#7C2D12` | `--color-primary` |
| On Primary | `#FFFFFF` | `--color-on-primary` |
| Secondary | `#B91C1C` | `--color-secondary` |
| On Secondary | `#FFFFFF` | `--color-on-secondary` |
| Accent/CTA | `#A16207` | `--color-accent` |
| On Accent/CTA | `#FFFFFF` | `--color-on-accent` |
| Background | `#FEF2F2` | `--color-background` |
| Foreground | `#450A0A` | `--color-foreground` |
| Card | `#FFFFFF` | `--color-card` |
| Card Foreground | `#450A0A` | `--color-card-foreground` |
| Muted | `#ECEDF0` | `--color-muted` |
| Muted Foreground | `#475569` | `--color-muted-foreground` |
| Border | `#FECACA` | `--color-border` |
| Destructive | `#DC2626` | `--color-destructive` |
| On Destructive | `#FFFFFF` | `--color-on-destructive` |
| Ring | `#7C2D12` | `--color-ring` |

**Color Notes:** Deep burgundy + craft gold [Accent adjusted from #CA8A04]

### Typography

- **Heading Font:** Abril Fatface
- **Body Font:** Merriweather
- **Mood:** retro, vintage, nostalgic, dramatic, decorative, bold
- **Google Fonts:** [Abril Fatface + Merriweather](https://fonts.googleapis.com/css2?family=Abril+Fatface&family=Merriweather:wght@300;400;700&display=swap)

**CSS Import:**
```css
@import url('https://fonts.googleapis.com/css2?family=Abril+Fatface&family=Merriweather:wght@300;400;700&display=swap');
```

### Spacing Variables

*Density: 7/10 — Standard*

| Token | Value | Usage |
|-------|-------|-------|
| `--space-xs` | `4px` / `0.25rem` | Tight gaps |
| `--space-sm` | `8px` / `0.5rem` | Icon gaps, inline spacing |
| `--space-md` | `16px` / `1rem` | Standard padding |
| `--space-lg` | `24px` / `1.5rem` | Section padding |
| `--space-xl` | `32px` / `2rem` | Large gaps |
| `--space-2xl` | `48px` / `3rem` | Section margins |
| `--space-3xl` | `64px` / `4rem` | Hero padding |

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
  background: #A16207;
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
  color: #7C2D12;
  border: 2px solid #7C2D12;
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
  background: #FEF2F2;
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
  border-color: #7C2D12;
  outline: none;
  box-shadow: 0 0 0 3px #7C2D1220;
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

**Style:** Brutalism

**Keywords:** Raw, unpolished, stark, high contrast, plain text, default fonts, visible borders, asymmetric, anti-design

**Best For:** Design portfolios, artistic projects, counter-culture brands, editorial/media sites, tech blogs

**Key Effects:** No smooth transitions (instant), sharp corners (0px), bold typography (700+), visible grid, large blocks

### Page Pattern

**Pattern Name:** Scroll-Triggered Storytelling

- **Conversion Strategy:** Keep the narrative understandable without scroll-driven effects. Use progress indicator. Mobile: simplify animations. Keep DOM reading order complete; disable parallax and scroll-scrub under reduced motion. Pause scroll animation when offscreen or hidden and render each chapter in its final readable state under reduced motion.
- **CTA Placement:** End of each chapter (mini) + Final climax CTA
- **Section Order:** Intro hook > Chapter 1 (problem) > Chapter 2 (journey) > Chapter 3 (solution) > Climax CTA

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

- ❌ Generic product pages
- ❌ No story

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

**Концепт:** «Девяносто Шесть». **Продукт (вотмард):** «ОКНО-96». Ремикс Win95/Y2K: серебристый хром окон с бевелями, синяя тайтлбар-градиент, жёсткая клетчатая структура, точечные пиксельные акценты. Современная сетка — ретро-детали.

**Диски (variance 8, motion 3, density 7):** смелая стилизация под системный UI прошлого при рабочей плотности; motion скупой — «машинный», без плавностей.

**Прозрачность данных базы:** поисковик НЕ содержит данных по Y2K/ретро-ОС: два запроса вернули нерелевантную категорию (Brewery/Winery — «винодельня»). Палитра и типографика ниже — **fallback-дефолты арт-директора**, а не данные базы; диски при этом применены штатно (variance 8 / motion 3 / density 7).

**Reduced-motion:** анимаций почти нет (стилизация статична по определению); отключается мигание курсора поиска.

```css
:root{
  --bg:#c8c4bd; --surface:#c0c0c0; --surface-2:#d6d3ce;
  --text:#101010; --text-muted:#3f3f3f;
  --border:#808080; --border-strong:#000000;
  --accent:#000080; --on-accent:#ffffff; --accent-soft:#d3d3f7;
  --titlebar-a:#000080; --titlebar-b:#1084d0;
  --link:#0000cc;
  --danger:#8f1010;  # затемнён для 4.5:1 на сером фоне --on-danger:#ffffff; --ok:#0a7a1f;
  --font-ui:Tahoma,'Segoe UI',system-ui,sans-serif; --font-display:'Press Start 2P',Tahoma,monospace; --font-mono:'IBM Plex Mono','Courier New',monospace;
  --r-s:0px; --r-m:0px; --r-l:0px;
  --s-1:4px; --s-2:8px; --s-3:12px; --s-4:16px; --s-5:24px; --s-6:32px;
  --t-fast:80ms; --t-slow:120ms; --ease:steps(2,end);
  --shadow-1:none; --shadow-2:none;
  --bevel-out:inset -1px -1px 0 #0a0a0a, inset 1px 1px 0 #ffffff, inset -2px -2px 0 #808080, inset 2px 2px 0 #dfdfdf;
  --bevel-in:inset 1px 1px 0 #0a0a0a, inset -1px -1px 0 #ffffff, inset 2px 2px 0 #808080, inset -2px -2px 0 #dfdfdf;
}
```
