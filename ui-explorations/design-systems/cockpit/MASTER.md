# Design System Master File

> **LOGIC:** When building a specific page, first check `design-system/pages/[page-name].md`.
> If that file exists, its rules **override** this Master file.
> If not, strictly follow the rules below.

---

**Project:** cockpit
**Generated:** 2026-09-26 20:23:23
**Category:** Gaming
**Design Dials:** Variance 7/10 (Balanced / Modern) | Motion 6/10 (Standard) | Density 7/10 (Standard)

---

## Global Rules

### Color Palette

| Role | Hex | CSS Variable |
|------|-----|--------------|
| Primary | `#7C3AED` | `--color-primary` |
| On Primary | `#FFFFFF` | `--color-on-primary` |
| Secondary | `#A78BFA` | `--color-secondary` |
| On Secondary | `#0F172A` | `--color-on-secondary` |
| Accent/CTA | `#F43F5E` | `--color-accent` |
| On Accent/CTA | `#000000` | `--color-on-accent` |
| Background | `#0F0F23` | `--color-background` |
| Foreground | `#E2E8F0` | `--color-foreground` |
| Card | `#1E1C35` | `--color-card` |
| Card Foreground | `#E2E8F0` | `--color-card-foreground` |
| Muted | `#27273B` | `--color-muted` |
| Muted Foreground | `#94A3B8` | `--color-muted-foreground` |
| Border | `#4C1D95` | `--color-border` |
| Destructive | `#EF4444` | `--color-destructive` |
| On Destructive | `#000000` | `--color-on-destructive` |
| Ring | `#7C3AED` | `--color-ring` |

**Color Notes:** Neon purple + rose action

### Typography

- **Heading Font:** Russo One
- **Body Font:** Chakra Petch
- **Mood:** gaming, bold, action, esports, competitive, energetic
- **Google Fonts:** [Russo One + Chakra Petch](https://fonts.googleapis.com/css2?family=Chakra+Petch:wght@300;400;500;600;700&family=Russo+One&display=swap)

**CSS Import:**
```css
@import url('https://fonts.googleapis.com/css2?family=Chakra+Petch:wght@300;400;500;600;700&family=Russo+One&display=swap');
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
  background: #F43F5E;
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
  color: #7C3AED;
  border: 2px solid #7C3AED;
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
  background: #0F0F23;
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
  border-color: #7C3AED;
  outline: none;
  box-shadow: 0 0 0 3px #7C3AED20;
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

**Style:** 3D & Hyperrealism

**Keywords:** Depth, realistic textures, 3D models, spatial navigation, tactile, skeuomorphic elements, rich detail, immersive

**Best For:** Gaming, product showcase, immersive experiences, high-end e-commerce, architectural viz, VR/AR

**Key Effects:** WebGL/Three.js 3D, realistic shadows (layers), physics lighting, parallax (3-5 layers), smooth 3D (300-400ms)

### Page Pattern

**Pattern Name:** Feature-Rich Showcase

- **Conversion Strategy:** Clear feature hierarchy. One key message per card. Strong CTA repetition.
- **CTA Placement:** Hero (sticky) + After features + Bottom
- **Section Order:** Hero (value prop) > Feature grid/cards (4-6) > Use cases or benefits > Social proof or logos > CTA

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

- ❌ Minimalist design
- ❌ Static assets

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

**Концепт:** «Кабина». **Продукт (вотмард):** «Кокпит». Игровая HUD-эстетика: срезанные углы (clip-path), угловые скобки-рамки, верхняя приборная полоса с телеметрией (RAM/TPS/сеть), крупная гранёная кнопка ИГРАТЬ, пульс статусов.

**Диски (variance 7, motion 6, density 7):** смелая геометрия и хореография статусов при сохранении сетки — Operate-действия всегда в одних местах.

**Оверрайды базы (и почему):** палитра Gaming из базы принята (тёмный индиго + фиолет + роза). Шрифт Chakra Petch не содержит кириллицы — заменён на IBM Plex Sans; Russo One (заголовки, есть кириллица) и телеметрия — JetBrains Mono.

**Reduced-motion:** отключаются пульс LED-точек и сдвиг граней на hover; HUD-статусы имеют текстовые значения и без анимации.

```css
:root{
  --bg:#0F0F23; --surface:#171633; --surface-2:#201e45;
  --text:#E2E8F0; --text-muted:#9aa4bd;
  --border:#2c2956; --border-strong:#4C1D95;
  --accent:#7C3AED; --on-accent:#ffffff; --accent-soft:rgba(124,58,237,.14);
  --alert:#F43F5E; --on-alert:#2a0710;  # тёмная слива: белый на #F43F5E не добирает 4.5:1
  --danger:#f28b82; --on-danger:#1c0908; --ok:#34d399;
  --font-ui:'IBM Plex Sans',system-ui,sans-serif; --font-display:'Russo One',system-ui,sans-serif; --font-mono:'JetBrains Mono',monospace;
  --r-s:2px; --r-m:4px; --r-l:8px;
  --s-1:4px; --s-2:8px; --s-3:12px; --s-4:16px; --s-5:24px; --s-6:32px;
  --t-fast:150ms; --t-slow:250ms; --ease:cubic-bezier(.2,0,0,1);
  --shadow-1:0 2px 8px rgba(0,0,0,.45); --shadow-2:0 12px 32px rgba(76,29,149,.35);
}
```
