# Design System Master File

> **LOGIC:** When building a specific page, first check `design-system/pages/[page-name].md`.
> If that file exists, its rules **override** this Master file.
> If not, strictly follow the rules below.

---

**Project:** glow
**Generated:** 2026-09-26 20:23:23
**Category:** Cybersecurity Platform
**Design Dials:** Variance 4/10 (Balanced / Modern) | Motion 4/10 (Standard) | Density 6/10 (Standard)

---

## Global Rules

### Color Palette

| Role | Hex | CSS Variable |
|------|-----|--------------|
| Primary | `#00FF41` | `--color-primary` |
| On Primary | `#0F172A` | `--color-on-primary` |
| Secondary | `#0D0D0D` | `--color-secondary` |
| On Secondary | `#FFFFFF` | `--color-on-secondary` |
| Accent/CTA | `#FF3333` | `--color-accent` |
| On Accent/CTA | `#000000` | `--color-on-accent` |
| Background | `#000000` | `--color-background` |
| Foreground | `#E0E0E0` | `--color-foreground` |
| Card | `#0C130E` | `--color-card` |
| Card Foreground | `#E0E0E0` | `--color-card-foreground` |
| Muted | `#181818` | `--color-muted` |
| Muted Foreground | `#94A3B8` | `--color-muted-foreground` |
| Border | `#1F1F1F` | `--color-border` |
| Destructive | `#EF4444` | `--color-destructive` |
| On Destructive | `#000000` | `--color-on-destructive` |
| Ring | `#00FF41` | `--color-ring` |

**Color Notes:** Matrix green + alert red

### Typography

- **Heading Font:** Orbitron
- **Body Font:** JetBrains Mono
- **Mood:** cyberpunk, neon, glitch, hud, sci-fi, dark, matrix green, magenta, chamfered, tactical
- **Google Fonts:** [Orbitron + JetBrains Mono](https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500&family=Orbitron:wght@700;900&display=swap)

**CSS Import:**
```css
@import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500&family=Orbitron:wght@700;900&display=swap');
```

### Spacing Variables

*Density: 6/10 — Standard*

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
  background: #FF3333;
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
  color: #00FF41;
  border: 2px solid #00FF41;
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
  background: #000000;
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
  border-color: #00FF41;
  outline: none;
  box-shadow: 0 0 0 3px #00FF4120;
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

**Style:** Cyberpunk UI

**Keywords:** Neon, dark mode, terminal, HUD, sci-fi, glitch, dystopian, futuristic, matrix, tech noir

**Best For:** Gaming platforms, tech products, crypto apps, sci-fi applications, developer tools, entertainment

**Key Effects:** Neon glow (text-shadow), glitch animations (skew/offset), scanlines (::before overlay), terminal fonts

### Page Pattern

**Pattern Name:** Real-Time / Operations Landing

- **Conversion Strategy:** Offer a demo or sandbox and show trust signals. Label telemetry as live only when backed by a current source, with update time and stale state. Provide pause/hide or update-frequency controls for tickers and previews, stop offscreen/hidden work, support keyboard controls, and render a static final snapshot under reduced motion.
- **CTA Placement:** Primary CTA in nav + After metrics
- **Section Order:** Hero (product + live preview or status) > Key metrics/indicators > How it works > CTA (Start trial / Contact)

---

## Motion

**Stagger List** (Standard) — Trigger: load or scroll | Duration: 300-450ms | Easing: `back.out(1.4)`

```js
gsap.from('.grid-item', { opacity: 0, scale: 0.92, y: 16, duration: 0.4, stagger: { each: 0.06, from: 'start', grid: 'auto' }, ease: 'back.out(1.4)' });
```

**Framework notes:** grid: 'auto' lets GSAP infer rows/columns from a CSS grid layout for a natural wave stagger; Use matchMedia('(prefers-reduced-motion: reduce)') to skip non-essential motion and render the final state immediately

- ✅ Combine with from: 'center' for a bento-grid layout to draw the eye inward first
- ❌ Don't use back.out on dense data tables; the overshoot reads as sloppy on informational UI
- ⚡ Group DOM writes; avoid interleaving layout reads (getBoundingClientRect) between staggered tweens

---

## Anti-Patterns (Do NOT Use)

- ❌ Light mode
- ❌ Poor data viz

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

**Концепт:** «Свечение». **Продукт (вотмард):** «Люмен». Глубокий чёрный + ОДИН светящийся циановый акцент: тонкие светящиеся границы у активных элементов, свечение кнопки Играть; стекло (blur) — только у модалок.

**Диски (variance 4, motion 4, density 6):** умеренная асимметрия за счёт световых акцентов, motion — мягкий glow-lift на hover и подсветка активного таба.

**Оверрайды базы (и почему):** база «Cyberpunk UI» даёт ДВА неоновых акцента (зелёный #00FF41 + красный #FF3333) — по брифу оставлен один: циан #45dcff; зелёный/красный переведены в малые статусные точки без свечения (--ok/--danger). Шрифт Orbitron не содержит кириллицы — заменён на Unbounded (ближайший по духу футуризм с кириллицей); тело — Inter, данные — JetBrains Mono (из базы).

**Reduced-motion:** отключаются пульс кнопки Играть и переходы свечения; свечение остаётся статичным (это форма, не движение).

```css
:root{
  --bg:#050608; --surface:#0b0e13; --surface-2:#11151c;
  --text:#e7edf3; --text-muted:#93a4b5;
  --border:#1c2733; --border-strong:#2b3b4b;
  --accent:#45dcff; --on-accent:#04222b; --accent-soft:rgba(69,220,255,.08);
  --accent-border:rgba(69,220,255,.35);
  --glow:0 0 14px rgba(69,220,255,.30), 0 0 2px rgba(69,220,255,.55);
  --danger:#ff7069; --on-danger:#220806; --ok:#3bd48f;
  --font-ui:'Inter',system-ui,sans-serif; --font-display:'Unbounded',system-ui,sans-serif; --font-mono:'JetBrains Mono',monospace;
  --r-s:6px; --r-m:10px; --r-l:14px;
  --s-1:4px; --s-2:8px; --s-3:12px; --s-4:16px; --s-5:24px; --s-6:32px;
  --t-fast:150ms; --t-slow:250ms; --ease:cubic-bezier(.2,0,0,1);
  --shadow-1:0 2px 8px rgba(0,0,0,.55); --shadow-2:0 12px 32px rgba(0,0,0,.65);
}
```
