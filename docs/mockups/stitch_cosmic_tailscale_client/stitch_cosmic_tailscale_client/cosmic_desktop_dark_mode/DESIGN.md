---
name: COSMIC Desktop Dark Mode
colors:
  surface: '#121316'
  surface-dim: '#121316'
  surface-bright: '#38393c'
  surface-container-lowest: '#0d0e11'
  surface-container-low: '#1a1b1e'
  surface-container: '#1f1f23'
  surface-container-high: '#292a2d'
  surface-container-highest: '#343538'
  on-surface: '#e3e2e6'
  on-surface-variant: '#becabf'
  inverse-surface: '#e3e2e6'
  inverse-on-surface: '#2f3033'
  outline: '#88948a'
  outline-variant: '#3e4941'
  surface-tint: '#79daa0'
  primary: '#79daa0'
  on-primary: '#00391f'
  primary-container: '#48a974'
  on-primary-container: '#00381f'
  inverse-primary: '#006d41'
  secondary: '#45dfa4'
  on-secondary: '#003825'
  secondary-container: '#00bd85'
  on-secondary-container: '#00452e'
  tertiary: '#bcc7dc'
  on-tertiary: '#263141'
  tertiary-container: '#8d98ac'
  on-tertiary-container: '#263041'
  error: '#ffb4ab'
  on-error: '#690005'
  error-container: '#93000a'
  on-error-container: '#ffdad6'
  primary-fixed: '#95f7bb'
  primary-fixed-dim: '#79daa0'
  on-primary-fixed: '#002110'
  on-primary-fixed-variant: '#005230'
  secondary-fixed: '#68fcbf'
  secondary-fixed-dim: '#45dfa4'
  on-secondary-fixed: '#002114'
  on-secondary-fixed-variant: '#005137'
  tertiary-fixed: '#d8e3f8'
  tertiary-fixed-dim: '#bcc7dc'
  on-tertiary-fixed: '#111c2b'
  on-tertiary-fixed-variant: '#3d4758'
  background: '#121316'
  on-background: '#e3e2e6'
  surface-variant: '#343538'
typography:
  headline-lg:
    fontFamily: Nunito Sans
    fontSize: 32px
    fontWeight: '700'
    lineHeight: 40px
    letterSpacing: -0.02em
  headline-lg-mobile:
    fontFamily: Nunito Sans
    fontSize: 26px
    fontWeight: '700'
    lineHeight: 34px
    letterSpacing: -0.015em
  headline-md:
    fontFamily: Nunito Sans
    fontSize: 24px
    fontWeight: '600'
    lineHeight: 32px
    letterSpacing: -0.01em
  headline-sm:
    fontFamily: Nunito Sans
    fontSize: 18px
    fontWeight: '600'
    lineHeight: 26px
  body-lg:
    fontFamily: Nunito Sans
    fontSize: 16px
    fontWeight: '400'
    lineHeight: 24px
  body-md:
    fontFamily: Nunito Sans
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 20px
  body-sm:
    fontFamily: Nunito Sans
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 18px
  label-lg:
    fontFamily: Nunito Sans
    fontSize: 14px
    fontWeight: '600'
    lineHeight: 20px
    letterSpacing: 0.01em
  label-md:
    fontFamily: Nunito Sans
    fontSize: 12px
    fontWeight: '600'
    lineHeight: 16px
    letterSpacing: 0.02em
  label-sm:
    fontFamily: Nunito Sans
    fontSize: 11px
    fontWeight: '700'
    lineHeight: 14px
    letterSpacing: 0.04em
rounded:
  sm: 0.25rem
  DEFAULT: 0.5rem
  md: 0.75rem
  lg: 1rem
  xl: 1.5rem
  full: 9999px
spacing:
  gutter: 1rem
  gutter-compact: 0.5rem
  margin: 1.5rem
  margin-mobile: 1rem
  space-xs: 0.25rem
  space-sm: 0.5rem
  space-md: 0.75rem
  space-lg: 1rem
  space-xl: 1.5rem
---

## Brand & Style

The design system embraces the engineered ergonomics of the modern open-source desktop workstation. Tailored for engineers, creators, and power users immersed in a distraction-free environment, the emotional response focuses on grounded stability, high density, and tactile precision. 

The aesthetic sits at the intersection of **Tactile Modernism** and **Systematic Utility**. It departs from weightless, purely flat interfaces by using layered surface tones, subtle edge highlights, and purposeful micro-depth inspired by libcosmic standards. Surfaces feel milled, quiet, and solid—yielding complete visual command to the active task while preserving structural boundaries through crisp border definition and soft geometry.

## Colors

The palette establishes an ergonomic, low-strain dark environment designed for extended periods of focused work:

- **Surface Neutral Canvas (`#1a1b1e`)**: Root desktop background, window root frames, and system base layer.
- **Surface Neutral Elevated (`#232529`)**: Core panel surfaces, sidebars, toolbars, and grouped card collections.
- **Surface Neutral Container (`#2c2e33`)**: Interactive inputs, selectable tiles, list hover states, and popovers.
- **Structural Stroke (`#373a40`)**: Low-contrast boundary lines providing geometric structure without visual noise.
- **Primary Accent (`#48a974`)**: Organic sage/emerald tone used for primary call-to-action buttons, active switches, and focused tabs.
- **Secondary Accent (`#34d399`)**: High-visibility mint highlight applied to active badges, real-time indicators, and focus rings.
- **Tertiary Neutral (`#768194`)**: Secondary typography, inactive icons, and metadata readouts.
- **Text & Glyph Hierarchy**: High-contrast text uses `#f1f3f5` (primary display), `#c1c5cd` (body readability), and `#768194` (caption and disabled states).

## Typography

Nunito Sans serves as the universal typeface across all desktop surfaces. Its open apertures and subtle rounding counter the technical rigidity of data-heavy views without compromising tabular clarity.

- **Headlines**: Set with tightened tracking (`-0.02em` to `-0.01em`) and solid weights (`600` to `700`) to anchor views cleanly without visual drift.
- **Body Content**: Built on a strict 14px default base, tuned for rapid scanning across dual-pane navigation structures and terminal-adjacent dashboards.
- **Labels & Metas**: Uppercase variants utilize increased tracking (`0.02em` to `0.04em`) and heavier weight to deliver high-density scannability for status chips, hotkeys, and table headers.

## Layout & Spacing

The layout is grounded in a modular, multi-panel desktop architecture with fluid pane adaptability:

- **Layout Model**: Split-pane layouts with fixed-width navigation rails (240px–280px) and flexible content panes that scale to accommodate tiled window management. Columns inside data surfaces rely on 16px (`1rem`) gutters.
- **Rhythm & Padding**: Inner container gaps and component sequences strictly adhere to an 8px base rhythm (`0.5rem`, `0.75rem`, `1rem`, `1.5rem`), preserving high information density while preventing visual congestion.
- **Breakpoints**:
  - `Desktop / Wide (> 1024px)`: Multi-pane sidebars with master-detail views, 24px margins, full toolbar expansions.
  - `Tablet / Medium (768px - 1023px)`: Collapsible left navigation to icon-only docks, 16px margins, fluid split views.
  - `Mobile / Compact (< 768px)`: Stacked linear flows, 12px outer margins, sheet-based modal overlays.

## Elevation & Depth

This design system avoids blurry, floaty drop-shadows in favor of **Tonal Layering combined with Low-Contrast Outlines**:

- **Canvas Tier (Base)**: `#1a1b1e` provides the deepest desktop substrate.
- **Structural Tier (Surfaces & Sidebars)**: `#232529` borders windows and docked panels. Separation is enforced by a 1px solid stroke of `#373a40`.
- **Card & Component Tier (Floating / Cards)**: `#2c2e33` defines interactive cards, elevated popovers, dropdown lists, and contextual menus.
- **Drop Shadows**: Shadows are utilized solely for transient overlay components (such as floating menus and dialog modals). They feature a compact, tight footprint: `0 8px 24px rgba(0, 0, 0, 0.45)`, reinforced by an immediate 1px border of `#373a40` to retain an architectural boundary against deep dark backgrounds.
- **Active Focus & Selection**: Outlined with a 2px ring using `#34d399` at `40%` alpha with a 1px offset, preserving crisp keyboard navigability.

## Shapes

The shape system is strictly bound to the `ROUND_TWELVE` (12px) standard characteristic of modern libcosmic applications:

- **Base Components (0.5rem / 8px)**: Segmented controls, badge chips, and nested inner list elements.
- **Containers and Cards (0.75rem / 12px - `ROUND_TWELVE`)**: Applied across standard buttons, text inputs, cards, context menus, and dialog containers.
- **Outer Shells & App Windows (1rem / 16px)**: Floating application shells, major modal sheets, and system overlay bounds.
- **Strict Geometric Uniformity**: Concentric radii are observed; inner elements nestled within cards use 6px or 8px radii to mirror the surrounding 12px container corner.

## Components

- **Buttons**:
  - *Primary*: Solid `#48a974` background, `#1a1b1e` high-contrast bold typography, 12px border radius, with a subtle hover transition to `#34d399`.
  - *Secondary / Neutral*: `#2c2e33` fill with a 1px `#373a40` stroke, `#f1f3f5` typography, shifting to `#373a40` fill on hover.
  - *Ghost / Flat*: Transparent background, `#c1c5cd` text, taking a `#232529` tint on hover.
- **Input Fields**:
  - Filled `#232529` background with a 1px `#373a40` perimeter stroke. 
  - Active focus transitions the border to `#48a974` with an accompanying subtle outline. Placeholder text is rendered in `#768194`. Height standardized to 38px with a 12px corner radius.
- **Cards**:
  - Flat `#232529` surface bounded by a 1px `#373a40` stroke. Inner interactive regions use `#2c2e33`. Padding adheres cleanly to 16px (`1rem`).
- **Chips & Tags**:
  - Pill or 8px-rounded compact elements. Status indicators pair a 6px solid circular dot (`#34d399` for active/connected) against a `#2c2e33` container with `#c1c5cd` metadata labels.
- **Checkboxes & Radios**:
  - 18px base squares (checkbox) or circles (radio) with a 1.5px `#373a40` idle border. Checked state fills with `#48a974` carrying an interior `#1a1b1e` mark.
- **Lists & Tree Views**:
  - Row heights standardized to 36px (compact) or 44px (default). Hover state applies a `#2c2e33` fill with an 8px corner radius. Selected rows feature a 3px vertical `#48a974` left indicator tab.
- **Segmented Controls**:
  - Enclosed `#1a1b1e` track holding 8px-rounded sliding tabs. Selected tab adopts `#2c2e33` with an active 1px border of `#373a40` and crisp `#f1f3f5` text.