# Plan — Give Infinite-Brainstorm the OSINT "Gotham-ops" look

**Date:** 2026-06-12
**Goal:** Re-skin infinite-brainstorm to match osint-workbench's visual identity —
the Palantir/Blueprint "Gotham-ops" language: near-black surfaces, ONE electric-blue
accent (#4c90f0) with a soft glow, square corners, a faint two-level graticule + edge
vignette, intent colors for status only, and monospace for IDs/meta. One coherent pass
across **both** the canvas renderer and the DOM chrome.

**Decisions (locked with user 2026-06-12):**
- Accent → **full OSINT electric-blue** (#4c90f0). Brainstorm becomes a faithful Gotham twin, not green.
- Scope → **canvas + DOM chrome** in one pass.
- **Camera & grid behavior stays exactly as brainstorm has it** — keep the camera-locked,
  pan/zoom grid. This is a **colors-and-styles-only** reskin: no behavioral change to the
  canvas. The two-level (major) grid and the edge vignette below are **optional polish**, not
  required — recoloring is the mandatory part.

---

## Why this is mostly mechanical (architecture insight)

The two apps differ in stack — brainstorm is **Leptos/Rust → WASM**, painting nodes on an
HTML5 **canvas2D** context; OSINT is **React/TS** with a **WebGL** graph + DOM panels. But
"the look" is just **color, grid, corners, type, density** — and in brainstorm those live in
two well-isolated, already-centralized places:

| Layer | Where it lives today | What it controls | Theming today |
|---|---|---|---|
| **1. Canvas render** | `src/canvas.rs:12-34` — 24 `const &str` color/font constants | nodes, edges, grid, groups, selection, resize handles, node text — *the bulk of the visual identity* | green-on-black, hardcoded |
| **2. DOM chrome** | ~10 inline `style="…"` strings in `src/app.rs` + `src/components/*.rs`, plus the (unused) boilerplate `styles.css` | HUD buttons, status line, modals, minimap, error banner | green hex inline, **zero CSS classes** |

OSINT, by contrast, drives everything from **one `:root` token block** (`tauri-app/src/App.css`)
consumed by class-based CSS. There is **no token system in brainstorm yet** — this plan
introduces one and re-points both layers at it.

> Brainstorm's canvas grid is **camera-locked** (pans/zooms with the board) — that's actually
> *better* than OSINT's fixed DOM-gradient graticule. **Keep that behavior unchanged; recolor only.**
> The second (major) grid level + a DOM vignette are *optional* depth polish, not required.

---

## The canonical token set (single source of truth)

Copied verbatim from OSINT `tauri-app/src/App.css :root`. Goes into brainstorm `styles.css`
for the DOM layer, and is **mirrored as hex/rgba** in the canvas constants (canvas2D can't read
CSS vars from the 2D context, so the Rust side holds literal equivalents — keep them commented
as "= var(--x)" so the two never drift).

```
--bg #0a0e14   --bg-panel rgba(17,22,31,.94)   --bg-elev rgba(22,28,39,.85)
--bg-hover rgba(76,144,240,.10)   --bg-solid #11161f
--border rgba(122,142,173,.16)    --border-strong rgba(122,142,173,.32)
--grid rgba(122,142,173,.08)      --grid-major rgba(122,142,173,.16)
--text #c8d2e0   --text-dim #8a97a8   --text-faint #5a6676
--accent #4c90f0   --accent-bright #6ba8ff
--accent-bg rgba(76,144,240,.12)  --accent-bg-hover rgba(76,144,240,.22)
--accent-line rgba(76,144,240,.45)  --accent-glow rgba(76,144,240,.30)
--ok #32a467   --warn #ec9a3c   --danger #e76a6e   --danger-text #fad7d7
--danger-bg rgba(98,25,27,.92)   --violet #ad8af0
--radius 0   --panel-shadow 0 10px 30px rgba(0,0,0,.55)
--mono ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace
font: Inter, system-ui, Avenir, Helvetica, Arial, sans-serif
```

---

## Step 0 — Establish the token system (foundation) → verify: `trunk build` compiles

- Replace the leftover Tauri/Leptos boilerplate in `styles.css` (`.logo`, `.container`, `.row`,
  the demo `#greet-input`, the `prefers-color-scheme` block) with the OSINT `:root` token block
  above + base resets (`html,body,#root` bg, `::selection`, thin flat scrollbars — copy OSINT's
  `* { scrollbar-width:thin; … }` rules).
- Keep `styles.css` as the DOM source of truth. Canvas keeps its own mirrored constants (Step 1).
- Out of scope: any markup/logic change here — tokens + resets only.

## Step 1 — Re-skin the canvas (Layer 1, biggest visual win) → verify: build + launch + screenshot

Rewrite the constant block `src/canvas.rs:12-34` per this exact mapping:

| Constant | Current (green) | → Gotham | = token |
|---|---|---|---|
| `BG_COLOR` | `#020202` | `#0a0e14` | --bg |
| `GRID_COLOR` | `#0a1a0a` | **split** → add `GRID_MINOR "rgba(122,142,173,0.08)"` + `GRID_MAJOR "rgba(122,142,173,0.16)"` | --grid / --grid-major |
| `BORDER_COLOR` | `#44dd66` | `rgba(122,142,173,0.32)` | --border-strong |
| `BORDER_SELECTED` | `#aaffbb` | `#4c90f0` | --accent |
| `TEXT_COLOR` | `#ccffdd` | `#c8d2e0` | --text |
| `TEXT_DIM` | `#66cc88` | `#8a97a8` | --text-dim |
| `EDGE_COLOR` | `#33aa55` | `rgba(76,144,240,0.45)` | --accent-line |
| `EDGE_PREVIEW` | `#aaffbb` | `#6ba8ff` | --accent-bright |
| `SELECT_BOX_FILL` | `rgba(100,200,130,0.15)` | `rgba(76,144,240,0.12)` | --accent-bg |
| `SELECT_BOX_STROKE` | `#aaffbb` | `#4c90f0` | --accent |
| `RESIZE_HANDLE_COLOR` | `#aaffbb` | `#6ba8ff` | --accent-bright |
| `RESIZE_HANDLE_BG` | `#020202` | `#0a0e14` | --bg |
| `EDGE_LABEL_BG` | `rgba(2,2,2,0.85)` | `rgba(17,22,31,0.94)` | --bg-panel |
| `GROUP_BG` | `rgba(50,170,85,0.06)` | `rgba(76,144,240,0.06)` | accent @ 6% |
| `GROUP_BORDER` | `rgba(50,170,85,0.25)` | `rgba(76,144,240,0.25)` | accent @ 25% |
| `GROUP_LABEL_COLOR` | `#448855` | `#8a97a8` | --text-dim |

**Node-type differentiation (the 6 `NODE_BG_*`).** OSINT keeps surfaces near-uniform and encodes
*kind* via a 3px accent left-border (`node-card { border-left: 3px solid var(--accent) }`). Adopt
the same move — most faithful + most legible:
- Set all 6 `NODE_BG_*` → one base `#11161f` (--bg-solid).
- Add 6 `NODE_ACCENT_*` stripe colors, drawn as a 3px left edge in the node-draw routine
  (`canvas.rs` ~L391-402): text→`#4c90f0`, idea→`#ad8af0` (violet), note→`#ec9a3c` (warn),
  image→`#32a467` (ok), md→`#6ba8ff` (accent-bright), link→`#ad8af0`.
- **Fallback if the stripe is too much for v1:** keep 6 bgs but in the blue-gray family —
  text `#11161f`, idea `#121826`, note `#141620`, image `#0f141d`, md `#15131f`, link `#101522`.

**Grid (recolor only; keep camera-locked behavior).** Mandatory: re-point the existing grid stroke
to `GRID_MINOR "rgba(122,142,173,0.08)"`. *Optional polish:* add `GRID_MAJOR` stroked every 5×
the minor interval (OSINT ratio 44px/220px). Do **not** change how the grid pans/zooms.

**Font.** `FONT` is currently mono for *everything*. OSINT uses Inter for labels, mono only for
IDs/meta. Split into `FONT_SANS = "Inter, system-ui, sans-serif"` (node body text, group labels)
and `FONT_MONO = "ui-monospace, 'SF Mono', Menlo, Consolas, monospace"` (handles/IDs/dim meta).
⚠️ Switching node-body text to a proportional font changes `ctx.measure_text` widths — **verify the
truncation / wrap math (`truncate_filename`, line-fitting) still lays out cleanly**; if it breaks,
keep a single mono `FONT` (just swap the stack) and revisit later.

**Corners.** Confirm the node-rect draw isn't rounding corners; if it rounds, set radius 0 (Gotham
is fully square).

## Step 2 — Re-skin DOM chrome (Layer 2) → verify: build + launch + hover states glow accent

Hover/focus states **require CSS classes** (inline `style=` can't express `:hover`), so the
interactive chrome moves from inline → a small utility-class set in `styles.css`, ported from
OSINT's `canvas.css`:

- Add classes: `.hud`, `.hud-btn`, `.pill`, `.pill-ready` (accent + glow), `.status-line`,
  `.modal`, `.modal-input`, `.canvas-vignette`. Copy OSINT's button hover
  (`:hover → border-color var(--accent-line); color var(--accent-bright); background var(--bg-hover)`).
- `src/app.rs:2605` `button_style` (HUD buttons) → drop inline, emit `class="hud-btn"`.
- `src/app.rs:2631` HUD container → `class="hud"`.
- `src/app.rs:2610` canvas container `background:#020202` → `var(--bg)`.
- `src/app.rs:2642` status line (`#66cc88` mono) → `class="status-line"` (color --text-dim, --mono).
- `src/components/error_banner.rs` `#ff8888`/`#e0a0a0` → `var(--danger)`/`var(--danger-text)`,
  banner bg → `var(--danger-bg)`, border → `var(--danger-line)`.
- `src/components/markdown_modal.rs` `#66cc88` → `var(--accent-bright)`; modal frame → `.modal`
  (bg --bg-panel, 1px --border, radius 0, shadow --panel-shadow, `backdrop-filter: blur(3px)`).
- `src/components/node_editor.rs` (the two `style=format!` blocks), `markdown_overlays.rs`
  (`style=format!`), `image_modal.rs`, `search_overlay.rs`, `minimap.rs` → recolor any hex to
  tokens; modal/overlay frames → `.modal`.
- **Vignette (optional polish):** a sibling `<div class="canvas-vignette">` over the canvas
  (pointer-events:none) using OSINT's radial-gradient edge-darkening. Pure DOM overlay — no
  WASM render-loop change, no camera impact. Skip if it muddies the board.

## Step 3 — Polish & parity pass → verify: side-by-side screenshot vs OSINT + `/review-branch`

- Square corners everywhere: `styles.css` `input,button` `border-radius` 8px → 0; soft box-shadow
  → `var(--panel-shadow)`; input/button colors → tokens.
- `::selection { background: var(--accent-bg-hover) }`, thin flat scrollbars (already in Step 0).
- Launch `brainstorm` on `examples/` (or the repo `board.json`), screenshot, place next to an OSINT
  screenshot, tune any off notes (border opacity, glow strength, grid spacing).

---

## Verification loop (per CLAUDE.md — design for observability)
- After **each** step: `cargo check` (or `trunk build`) — must compile clean, no clippy regressions.
- After Steps 1 & 2: launch the app (`brainstorm <dir>`), screenshot the canvas + open a modal.
  No Storybook for Leptos, so visual verify = live app screenshot, compared against OSINT.
- Final: run `/review-branch` before committing.

## Risks / notes
- **Two palette sources** (styles.css `:root` + canvas.rs consts) must agree. Comment each canvas
  const with its `= var(--x)` equivalent. (Future option: inject computed CSS-var values into the
  canvas at boot via `getComputedStyle`, collapsing to one source — out of scope for v1.)
- **Proportional node font** is the one layout-risky change (measure_text widths) — gated behind the
  truncation-math check above; mono fallback ready.
- **Inter** isn't bundled (OSINT relies on it being system-present too). If the exact face matters,
  add an `Inter` woff2 to `public/` + `@font-face`; otherwise system fallback is fine.
- **Don't touch osint-workbench** — it's the reference.

## Out of scope
- Any functional/behavioral change, new nodes, or new panels.
- OSINT-only surfaces brainstorm doesn't have (object explorer, time axis, agent-log, map,
  face-match, resolution panel). This is a *visual-language* port, not a feature port.

## Effort
Medium. Step 1 ≈ one sitting (constants + grid + stripe). Step 2 ≈ one sitting (classes + recolor +
vignette). Step 3 ≈ short. Recommend Plan → (audit-plan) → implement Step 1, screenshot-gate, then
Step 2, screenshot-gate, then polish.
