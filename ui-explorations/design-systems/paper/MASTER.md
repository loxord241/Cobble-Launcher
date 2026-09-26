# Design System Master File

> **LOGIC:** When building a specific page, first check `design-system/pages/[page-name].md`.
> If that file exists, its rules **override** this Master file.
> If not, strictly follow the rules below.

---

**Project:** paper
**Generated:** 2026-09-26 20:23:23
**Category:** Book & Reading Tracker
**Design Dials:** Variance 2/10 (Centered / Minimal) | Motion 2/10 (Subtle) | Density 3/10 (Spacious)

---

## Global Rules

### Color Palette

| Role | Hex | CSS Variable |
|------|-----|--------------|
| Primary | `#78716C` | `--color-primary` |
| On Primary | `#FFFFFF` | `--color-on-primary` |
| Secondary | `#92400E` | `--color-secondary` |
| On Secondary | `#FFFFFF` | `--color-on-secondary` |
| Accent/CTA | `#D97706` | `--color-accent` |
| On Accent/CTA | `#000000` | `--color-on-accent` |
| Background | `#FFFBEB` | `--color-background` |
| Foreground | `#0F172A` | `--color-foreground` |
| Card | `#FFFFFF` | `--color-card` |
| Card Foreground | `#0F172A` | `--color-card-foreground` |
| Muted | `#F6F6F6` | `--color-muted` |
| Muted Foreground | `#475569` | `--color-muted-foreground` |
| Border | `#EEEDED` | `--color-border` |
| Destructive | `#DC2626` | `--color-destructive` |
| On Destructive | `#FFFFFF` | `--color-on-destructive` |
| Ring | `#78716C` | `--color-ring` |

**Color Notes:** Book brown + page amber

### Typography

- **Heading Font:** Cormorant Garamond
- **Body Font:** Crimson Pro
- **Mood:** academia, library, mahogany, parchment, brass, scholarly, prestige, antique, victorian, leather
- **Google Fonts:** [Cormorant Garamond + Crimson Pro](https://fonts.googleapis.com/css2?family=Cinzel:wght@400;500;600&family=Cormorant+Garamond:ital,wght@0,300;0,500;0,700;1,300;1,500&family=Crimson+Pro:ital,wght@0,300;0,400;0,600;1,300;1,400)

**CSS Import:**
```css
@import url('https://fonts.googleapis.com/css2?family=Cinzel:wght@400;500;600&family=Cormorant+Garamond:ital,wght@0,300;0,500;0,700;1,300;1,500&family=Crimson+Pro:ital,wght@0,300;0,400;0,600;1,300;1,400&display=swap');
```

### Spacing Variables

*Density: 3/10 — Spacious*

| Token | Value | Usage |
|-------|-------|-------|
| `--space-xs` | `4px` / `0.25rem` | Tight gaps |
| `--space-sm` | `8px` / `0.5rem` | Icon gaps, inline spacing |
| `--space-md` | `24px` / `1.5rem` | Standard padding |
| `--space-lg` | `32px` / `2rem` | Section padding |
| `--space-xl` | `48px` / `3rem` | Large gaps |
| `--space-2xl` | `64px` / `4rem` | Section margins |
| `--space-3xl` | `96px` / `6rem` | Hero padding |

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
  background: #D97706;
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
  color: #78716C;
  border: 2px solid #78716C;
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
  background: #FFFBEB;
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
  border-color: #78716C;
  outline: none;
  box-shadow: 0 0 0 3px #78716C20;
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

**Pattern Name:** Hero + Testimonials + CTA

- **Conversion Strategy:** Social proof before CTA. Use a concise set of verified testimonials with photo, name, and role. CTA after social proof. Provide previous/next and pause controls; stop rotation on focus, hover, and reduced motion; announce slide position. Previous/next buttons and keyboard controls must expose every slide without dragging.
- **CTA Placement:** Hero (sticky) + Post-testimonials
- **Section Order:** Hero > Problem statement > Solution overview > Testimonials carousel > CTA

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

- ❌ Excessive decoration

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

**Концепт:** «Бумага». **Продукт (вотмард):** «Бумага». Светлая «читательская» система: тёплый белый, чернильный текст, заголовки-антиква 28+, мягкие тени вместо границ, акцент — киноварь. Лаунчер как красиво свёрстанный журнал запусков.

**Диски (variance 2, motion 2, density 3):** воздух и крупные заголовки; motion — только мягкое поднятие карточки на hover.

**Оверрайды базы (и почему):** фон #FFFBEB заменён на бумажный #faf9f7 (бриф); янтарный акцент #D97706 заменён на киноварь #c5392b (бриф); кегль базы поднят — базовый 17px, заголовки от 28px. Serif-пара базы (Cormorant Garamond + Crimson Pro) сохранена — обе с полной кириллицей.

**Reduced-motion:** отключаются поднятие/тень на hover (остаётся смена тени на статичную рамку) и shimmer скелетона.

```css
:root{
  --bg:#faf9f7; --surface:#ffffff; --surface-2:#f3f0ea;
  --text:#24211d; --text-muted:#6b6459;
  --border:#e8e3d9; --border-strong:#d7d0c2;
  --accent:#c5392b; --on-accent:#fff6f4; --accent-soft:#f6e3df;
  --danger:#b3342a; --on-danger:#fff6f4; --ok:#3e7d54;
  --font-ui:'Crimson Pro',Georgia,serif; --font-display:'Cormorant Garamond',Georgia,serif; --font-mono:'JetBrains Mono',monospace;
  --r-s:6px; --r-m:10px; --r-l:14px;
  --s-1:4px; --s-2:8px; --s-3:12px; --s-4:16px; --s-5:24px; --s-6:32px;
  --t-fast:150ms; --t-slow:250ms; --ease:cubic-bezier(.2,0,0,1);
  --shadow-1:0 1px 3px rgba(36,33,29,.08); --shadow-2:0 10px 28px rgba(36,33,29,.12);
}
```
