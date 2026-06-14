# Infinite Brainstorm — Backlog Roadmap

**Date:** 2026-06-14
**Status:** 2 items implemented on branches (in worktrees, NOT merged) · 6 design sketches awaiting decisions

This roadmap covers the current backlog. Two execute items were built and adversarially
reviewed but are sitting on branches in worktrees — review them before merging. Six items are
plan-only design sketches that need product/architecture decisions before any code is written.

---

## 1. Implemented (review before merge)

> These are on local branches inside `.claude/worktrees/`. They are **NOT merged to `main`**.
> Each was reviewed by an adversarial verifier (findings below were independently reproduced).

### 1a. launcher-fix — ✅ MERGE-READY

| | |
|---|---|
| **Branch** | `feat/launcher-fix` |
| **Worktree** | `/Users/lucianolupo/projects/infinite-brainstorm/.claude/worktrees/wf_e2884213-80b-12` |
| **HEAD** | `da510a2` — `fix(launcher): pass known subcommands through to the binary` |
| **Build** | ✅ `buildOk: true` |
| **Tests** | ✅ `testOk: true` |
| **Review verdict** | **mergeReady: true** — no blockers, no majors |
| **Files** | `scripts/brainstorm`, `CLAUDE.md`, `README.md`, `.claude/skills/infinite-brainstorm/SKILL.md` |

**What it does:** The launcher (`scripts/brainstorm`) now forwards known subcommands
(`validate`/`query`, plus `--help`/`-h`/`--version`) to the release binary instead of treating
the first argument as a directory name. This is what makes `brainstorm validate` / `brainstorm query`
actually reach the CLI (previously they hit "Directory 'validate' not found").

**Verified findings — all MINOR/NIT, none blocking:**

- **(minor) `export` is in the passthrough set but has no backing subcommand.** `scripts/brainstorm`
  routes `export` to the binary, but `src-tauri/src/main.rs` only defines `Validate`/`Query` clap
  subcommands. Verified: `brainstorm export` exits 2 with clap's `unrecognized subcommand 'export'`.
  For a directory literally named `export`, this converts a previously-working GUI launch into a hard
  error with zero current benefit. Forward-compat only pays off once headless-export lands (see 1b).
- **(minor) Docs advertise `export` as a forwarded subcommand, but it only errors.** `CLAUDE.md:36`
  and `README.md:283` say the launcher forwards `validate`/`query`/`export`. Misleading until the
  subcommand exists. `SKILL.md:290-291` is accurate (omits `export`). Tied to the decision below.
- **(nit) `-V` short version flag not in passthrough; asymmetric with `-h`.** Verified `brainstorm -V`
  falls through to the directory path and errors (exit 1). Cosmetic; `--version` works.
- **(nit) No automated launcher test.** Refuted as a real gap — there's no shell-test harness anywhere
  in the repo and CI never touches the launcher. Adding bats for a one-line guard would be scope creep.
  Manual smoke tests were independently reproduced.

**Coupling note:** the `export` passthrough only becomes correct once 1b (headless-export) merges.
If 1b merges, the `export` finding resolves itself. If 1b is deferred, consider dropping `export` from
the script + the two doc lines to avoid shipping a documented-but-broken subcommand.

---

### 1b. headless-export — ⛔ NOT MERGE-READY (1 MAJOR blocker)

| | |
|---|---|
| **Branch** | `feat/headless-export` |
| **Worktree** | `/Users/lucianolupo/projects/infinite-brainstorm/.claude/worktrees/wf_e2884213-80b-13` |
| **HEAD** | `aab2a6b` — `feat: headless brainstorm export subcommand (pure-Rust SVG renderer)` |
| **Build** | ✅ `buildOk: true` |
| **Tests** | ✅ `testOk: true` (but no test covers the malicious-input case — see major) |
| **Review verdict** | **mergeReady: false** — 1 major (security), 2 minor, 1 nit |
| **Files** | `src-tauri/src/lib.rs`, `src-tauri/src/main.rs` |

**What it does:** Adds `brainstorm export <board.json> --out <path.svg>` — a pure-Rust SVG renderer that
deterministically renders a board to SVG headlessly (no GUI / no WASM), marketed as SSRF-safe and
XML-safe. Supports `--camera X,Y,ZOOM`, `--fit`, `--region`.

**🚨 BLOCKER (major) — must fix before merge: SVG attribute injection via node `color`.**
`src-tauri/src/lib.rs:704` reads `node.color` and `:712` emits it into `stroke="{border}"` **without
`xml_escape`**. Every text run is escaped, but this attribute is not. `Board::validate()` does NOT
validate `color`, so an arbitrary string passes. Reproduced end-to-end: a board with
`color = red"/><script>alert(1)</script><rect x="` produces output containing a live
`<script>alert(1)</script>`, and `xmllint --noout` reports the result **VALID** (well-formed injected
markup). Because the documented use case is "attach the SVG to a PR/report/chat" where SVG can render
inline in HTML, this is script execution. The exporter's own SSRF-safe / XML-safe framing puts
untrusted/shared boards in scope, so this is an explicit guarantee gap, not a latent quirk.
**Fix is one line:** wrap with `xml_escape(border)`. Add a regression test with a malicious color
(the existing `contains_expected_content` test only asserts a valid `#ff6600` hex).

**Other verified findings:**

- **(minor) `ExportView::Camera` zoom is not clamped/validated.** The GUI clamps zoom to `0.1..=5.0`
  everywhere; the explicit-camera path does not. Verified `--camera 0,0,0` exits 0 and writes a valid
  but blank SVG (every node 0×0); `--camera 0,0,-1` mirrors content off-screen. No crash (non-finite is
  guarded), but it's a silent footgun producing a useless image with exit 0. The `--fit`/`--region`
  paths route through `fit_camera` which clamps, so only explicit `--camera` is affected. Fix: clamp to
  `0.1..=5.0` or reject `zoom <= 0`.
- **(minor) Installed `~/.local/bin/brainstorm` lacks the subcommand passthrough.** On `main` the
  launcher had NO subcommand handling at all (it `cd`'d into `$1` and `exec`'d with zero args), so
  `validate`/`query`/`export` were unreachable from the installed CLI. The passthrough that wires them
  up lives on the launcher-fix branch (1a). The currently-installed launcher still lacks it, so
  `export` (and `validate`/`query`) are broken from the installed CLI **until the user re-syncs the
  script**. Operational, not a code defect — but it means 1b's user-facing CLI depends on 1a being
  merged AND the script being re-installed.
- **(nit) `resolve_view` takes `fit: bool` only to discard it** (`let _ = fit;`). Clap's
  `group = "view"` already enforces at-most-one and `Fit` is the `None` default. Param could be dropped
  for a cleaner signature. Not load-bearing.

**Merge gate:** fix the `xml_escape(border)` blocker + add a malicious-color regression test. The two
minors (zoom clamp, re-sync) are strongly recommended before exposing the CLI to users. After that,
1b is mergeable and it retroactively justifies the `export` passthrough + docs in 1a.

---

## 2. Design sketches (need your decisions)

> Plan-only. No code written. Each has an approach, files it would touch, effort, risks, and open
> questions that need a human decision before implementation.

### 2a. Keyboard navigation (arrow-key traversal) — Effort: **S**

**Approach:** Pure frontend (Leptos/WASM); never touches `src-tauri`/CLI. Slots into the existing
canvas keydown handler at `src/app.rs:2274` (has an empty `_ => {}` arm). Add four arms
(`ArrowUp/Down/Left/Right`) that read the single selection from `selected_nodes.get_untracked()`,
compute the next node via a new **pure** helper `next_connected_node(board, from_id, dir)` (testable
host-side with `cargo test`, no WASM), set the new selection, and recenter the camera by reusing
`center_camera_on()` from `src/components/search_overlay.rs:10` (promote it next to `fit_camera` so
both call-sites share one pan-to-node primitive). Must call `ev.prevent_default()` in each arm.
Traversal model: treat edges as undirected (collect all edges where the node is `from_node` OR
`to_node`), pick the neighbor whose center-to-center direction best matches the arrow (dominant-axis
filter, nearest-by-distance tie-break).

**Files:** `src/app.rs`, `src/components/search_overlay.rs`, `CLAUDE.md`

**Risks:** arrow keys scroll by default → every arm must `prevent_default()`; directed-vs-undirected
ambiguity (some visible connections un-traversable if you only follow `from→to`); two ways to get the
canvas rect (`canvas_ref` vs `query_selector`) could drift — pick the `canvas_ref` path; behavior on 0
or >1 selection; disconnected nodes / no neighbor must no-op gracefully; tie-break rule must be defined
so the unit test is deterministic.

**Open questions (human decisions):**
- Directed or undirected traversal? (Recommend: undirected — navigate any visible connection.)
- Behavior on no-selection / multi-selection? (Recommend: empty → nearest-to-center; multi → no-op.)
- Tie-break rule when several neighbors lie in the pressed direction (nearest distance vs angular
  deviation vs strict dominant-axis-then-nearest)?
- Recenter the camera every step, only when target is off-screen, or never? (Recommend: pan only if
  off-screen.)
- Node→node only, or also node→edge / edge→edge? (Recommend: node→node for v1.)
- Should `Shift+Arrow` extend a multi-selection/path, or are bare arrows the only binding for v1?

---

### 2b. Semantic zoom (collapsed labels when zoomed out) — Effort: **M**

**Approach:** Live canvas-rendering concern (NOT the export feature). Core insight: there are **two
rendering surfaces that must agree** — Canvas 2D `draw_node` (`src/canvas.rs ~388-546`) and the
absolutely-positioned DOM layer in `MarkdownOverlays` (`src/components/markdown_overlays.rs`) used by
`md`/local-`.md`-link nodes. A naive change desyncs them (md nodes keep full markdown while text nodes
collapse). Introduce one pure helper in `brainstorm-types`:
`enum ZoomTier { Collapsed, Summary, Full }` + `ZoomTier::for_zoom(zoom) -> ZoomTier` (e.g. `<0.4`
Collapsed, `0.4..0.85` Summary, `>=0.85` Full) so both surfaces call the same breakpoints and can't
drift — mirroring the existing shared-types anti-drift discipline. Full = today's behavior. Summary =
body clamped to 1–2 lines (reuse `wrap_text_cached`, clamp `visible_lines`). Collapsed = box + one
centered label only (no body/meta/overlay). In `draw_node`, compute the tier once from `camera.zoom`
and branch; in `MarkdownOverlays`, add the same `ZoomTier::for_zoom` gate to its filter so the DOM
layer collapses with the canvas layer. The rAF loop already tracks `camera`, so zoom changes re-render
automatically. Boundary-value unit tests for `for_zoom` run native (no WASM).

**Files:** `crates/brainstorm-types/src/lib.rs`, `src/canvas.rs`,
`src/components/markdown_overlays.rs`, `.claude/skills/infinite-brainstorm/board.schema.json`,
`CLAUDE.md`, `src/app.rs`

**Risks:** two-surface desync (most likely bug — both must use `ZoomTier::for_zoom`); threshold
flicker at a boundary (pick hysteresis / space thresholds clear of the 0.85–1.0 reading zoom; verify
visually); char-boundary slicing for first-line-of-text labels (reuse `truncate_filename`); adding
`summary: Option<String>` to `Node` touches the serde model in both crates + the golden_board fixture +
schema (must keep `skip_serializing_if = Option::is_none` so old boards round-trip — there's a
`skip_serializing_empty_metadata` test); confirm `WRAP_CACHE` keying still holds; `CLAUDE.md` lists
semantic zoom under "Not Yet Implemented" and SKILL.md documents node fields — both are part of "done".

**Open questions (human decisions):**
- **Label source:** first-line-of-`text` (zero schema change, automatic) vs a new optional
  `Node.summary` field (agent-authorable, on-brand for agent-native, touches schema/fixture/docs).
  Strong lean toward `summary` with first-line fallback — **needs a yes.**
- How many tiers and at what thresholds? Proposal: Collapsed `<0.4`, Summary `0.4–0.85`, Full `>=0.85`.
  Could also be 2 tiers for simplicity. Needs a product call + visual tuning.
- At Collapsed, still draw the type indicator (`[IDEA]` etc.) and per-node border color, or just box +
  label?
- Does meta (tags/status/priority/P-badge) show at Summary or only at Full? (Probably Full-only.)
- For `md`/link-preview/image nodes at Summary: truncated preview, first heading, or fall through to
  Collapsed? (Image/remote-link likely Collapsed — no good summary text.)
- Also gate edge labels (midpoint pill) at low zoom? (Out of stated scope; confirm whether to include.)

---

### 2c. Themes / light mode — Effort: **M**

**Approach:** Two layers with very different difficulty. **DOM chrome** (HUD, status line, modals,
overlays, error banner, node editor, search) is almost entirely driven by CSS custom properties in the
single `:root` block of `styles.css` — adding a light theme is the standard pattern: define a second
token set under `[data-theme="light"]`, keep `:root` as the dark/Gotham default, toggle a `data-theme`
attribute on `<html>`. Only DOM stragglers are hardcoded `rgba(0,0,0,0.9)` modal backdrops in
`image_modal.rs`/`markdown_modal.rs` → make them a `--modal-scrim` token. **The hard layer is the
canvas**: `src/canvas.rs` can't read CSS variables (Canvas2D has no access), so the palette is mirrored
as ~20 private `const &str` literals (lines 15–41), each annotated `= var(--x)`; the minimap hardcodes
3 more. Replace these with a `Theme` struct (one field per swatch) + two const instances `GOTHAM` /
`LIGHT`, thread `theme: &Theme` through `RenderState` (already a named-field struct) into
`render_board`/`draw_node`/`draw_edge`/`draw_groups`/`draw_grid` and the minimap. Put `Theme` in
`crates/brainstorm-types` (no web deps, anti-drift home, reusable by a future SVG exporter) — but the
CSS `:root` tokens stay the human-facing source of truth, kept in sync by hand via the `= var(--x)`
contract. Toggle: a Leptos `theme` signal in `app.rs` from a new localStorage key
(`infinite-brainstorm-theme`, optionally seeded from `prefers-color-scheme`); a new HUD button cycles
it; on change write `data-theme` on `documentElement`, persist, and pick the matching `&Theme` in the
render closure (the Effect re-triggers a frame). Ship Gotham + one light behind a generalized mechanism
so a 3rd theme is just another const + token block.

**Files:** `styles.css`, `crates/brainstorm-types/src/lib.rs`, `src/canvas.rs`, `src/app.rs`,
`src/components/minimap.rs`, `src/components/image_modal.rs`, `src/components/markdown_modal.rs`,
`CLAUDE.md`

**Risks:** palette drift across 3 sources (CSS `:root`, canvas `Theme` consts, minimap rgba) — the
`= var(--x)` comments are the only enforcement, no compile-time check; light mode is genuine design,
not inversion (Gotham node BGs differ by a few channel points; edge/group alphas tuned for near-black
will be invisible on white); per-node `color` overrides are theme-agnostic (some user content looks
wrong on light, no remap without changing stored data); thread-through churn (~20 const sites + minimap
— a missed site silently keeps Gotham); markdown/pulldown-cmark inline styles/code-block colors may not
retint cleanly; selection-glow/shadow tuned for dark may look muddy on light.

**Open questions (human decisions):**
- v1 scope: Gotham + one light, or a generalized N-theme registry? (Decides boolean toggle vs
  cycling/selector, and enum-map vs two consts.)
- Where does the canonical `Theme` palette live: relocated into `brainstorm-types` (preferred for
  anti-drift + future SVG-exporter reuse, but adds palette data to a pure-geometry crate) or kept
  canvas-local (less churn, accepts drift)?
- Default + first-run: keep Gotham as hard default, or auto-detect `prefers-color-scheme` when no
  stored preference?
- Persist per-board (like the camera key) or globally? (Global makes more sense for chrome.)
- Does the light theme need a hand-designed palette (recommended — someone must pick hexes that read
  on light) or derive-then-iterate-visually?
- Should the Export PNG / future SVG exporter respect the active theme? (PNG follows the canvas
  automatically; SVG-exporter reuse is the main reason to share `Theme` now.)

---

### 2d. Touch / tablet gestures (pan/zoom/select) — Effort: **M**

**Approach:** Grounded in actual interaction code (`src/app.rs:1671-2217`), not the export map. Today
every canvas interaction is discrete `MouseEvent` handlers (`on:mousedown/move/up/leave` + `on:wheel`);
there's NO `touch-action` CSS, NO viewport meta, and Cargo.toml enables only
`MouseEvent`/`WheelEvent`/`KeyboardEvent`. Recommended: adopt **Pointer Events** as the single unified
input path rather than a parallel TouchEvent stack. `PointerEvent` derefs to `MouseEvent`, so
`event_canvas_pos`/`event_world_pos` and the entire reducer/gesture state machine
(`DragState`/`PanState`/`ResizeState`/`EdgeCreationState`) work UNCHANGED for single-pointer — a
one-finger drag maps onto existing pan/node-drag/box-select. Rewire canvas + document continuation
listeners (app.rs:2108-2161) from `mouse*` to `pointer*`, use `set_pointer_capture(pointer_id)` on
pointerdown. **Genuinely new code:** multi-touch pinch-zoom + disambiguation (Pointer Events don't give
this free). Track active pointers in a `HashMap<i32, (f64,f64)>`; when exactly two are down, suspend the
single-pointer gesture, track centroid + distance, and apply `dist/prev_dist` to `camera.zoom` centered
on the centroid using the SAME math as `on_wheel` (app.rs:2204-2213). Two mandatory easy-to-miss
changes: (1) `touch-action: none` on the canvas (or the browser eats pan/pinch), (2) a viewport meta
tag in `index.html`. Enable `PointerEvent` in Cargo.toml. Keep `on_wheel` as-is (macOS trackpad pinch =
ctrl+wheel). Extract the pinch math (distance/centroid/zoom-from-focal-point) into a pure testable
helper.

**Files:** `src/app.rs`, `Cargo.toml`, `index.html`, `styles.css`,
`crates/brainstorm-types/src/lib.rs`, `CLAUDE.md`

**Risks:** gesture disambiguation (first finger may start a drag before the second arrives → must
cancel/roll back the in-flight single-pointer gesture, avoiding a junk undo entry); pointer-capture vs
the document continuation listeners (double-processing — pick ONE mechanism); missing
`touch-action: none` is the #1 reason "touch doesn't work" after wiring; **this is a desktop Tauri app**
— real touchscreen testing needs a touch Windows/Linux tablet (iPad is NOT a current Tauri target,
browser-mode only); macOS WKWebView may fire legacy GestureEvent and suppress pointermove during
2-finger gestures (needs on-device verification); resize handles (8px world) / edge-pick (10px/zoom)
are near-untappable on a finger; switching all desktop input to pointer events requires regressing
every desktop interaction (cmd-multi-select, shift-drag-edge, box-select, resize, off-canvas drag).

**Open questions (human decisions):**
- **What is the actual target device/runtime for "tablet"?** Touchscreen Windows/Linux laptop running
  the Tauri build, an iPad (browser/localStorage only — not a Tauri target), or just macOS trackpad?
  **This determines whether the work is real and how to test it.**
- v1 gesture scope: is pinch-zoom + one-finger-pan + tap-select enough, or also two-finger-pan,
  long-press-context, double-tap-to-edit?
- Touch has no modifiers — what gesture replaces cmd/ctrl-multi-select, cmd/ctrl-box-select, and
  shift-drag edge creation? (New gestures, or declared out of scope for touch.)
- Unified Pointer Events path (recommended, migrates desktop off raw MouseEvent) vs a separate
  TouchEvent layer (lower desktop-regression risk, more duplicated logic)? **Central architectural
  decision.**
- Enlarge resize/edge hit areas for coarse pointers in this change, or defer? (Affects day-one
  usability.)
- Is on-device manual testing acceptable as the acceptance gate (no touch simulation in
  `cargo test`/Storybook), and on which device?

---

### 2e. Multi-board / board switcher — Effort: **L**

**Approach:** Add Tauri managed state for an active board path, thread it through every backend leaf,
and re-point the file watcher so the active board becomes runtime-mutable instead of recomputed from
cwd.

**Files:** `src-tauri/src/lib.rs`, `src/app.rs`, `styles.css`, `src-tauri/tests/watcher.rs`,
`CLAUDE.md`, `.claude/skills/infinite-brainstorm/SKILL.md`

**Risks:** watcher re-target (lib.rs:1015 is a detached thread with no handle — an un-repointed switch
silently kills live-sync); global self-write hash (lib.rs:26 — interleaved saves across two boards
mis-attribute, miss reloads, or loop); `board_dir` scope (lib.rs:699 follows the active board —
validate the path stays inside the launch root); saves debounce ~220ms (force-flush before swapping or
lose the outgoing board's edits); camera restored once at startup, never by the watcher (a switch must
replicate it or land at default); keep `brainstorm` / `brainstorm DIR` / `validate` / `query` unchanged
including the src-tauri-parent case at lib.rs:285.

**Open questions (human decisions):**
- Board discovery model: any JSON that parses, a naming convention, or a manifest?
- Same-directory boards vs cross-directory boards?
- Browser-mode multi-board, or Tauri-only for v1?
- Can the switcher create/rename/delete boards in v1?
- Persist the last-active board and add a CLI `board` flag?
- Watcher re-target via a control channel or a shared filename mutex?

---

### 2f. CRDT (Loro) real-time multi-client collaboration — Effort: **XL**

**Approach:** Hard truth: today's sync is fundamentally incompatible with "real-time multi-client" and
the bulk of the work is NOT the CRDT library — it's replacing a whole-file last-write-wins pipeline and
inventing a transport tier that does not exist. Every save serializes the entire `Board`
(`write_board_atomic`), the `notify` watcher fires `board-changed`, and the frontend reloads the whole
board (`reload_board_into` → `set_board.set`); `LAST_SELF_WRITE_HASH` exists to stop self-reload. Two
concurrent writers clobber each other. The design: make a Loro document the source of truth, derive
`Board` from it for rendering, and translate the `BoardAction` reducer (already discrete, DOM-free,
unit-tested in `src/interaction.rs`) into Loro mutations — they map ~1:1 (board = `LoroMap` of nodes +
`LoroMap` of edges; `MoveNodes`/`ResizeNode` → per-field register sets; create/delete → map ops;
`EditText`/`EditMarkdown` → register set OR per-node `LoroText` for true concurrent merging).
`to_loro`/`from_loro` lives in `brainstorm-types` (or a new `brainstorm-crdt` crate) so both halves
share one schema. **Transport is the genuinely new/large part:** no server/websocket/peer layer exists.
Options cheapest-first: (1) single-machine multi-process via the existing file (store the Loro snapshot,
import-and-merge on watcher events instead of overwrite — smallest step, kills the local LWW clobber,
not network multi-client); (2) a small relay server (Tauri sidecar or standalone Rust + tokio + ws,
browser over WASM ws); (3) full P2P (libp2p/iroh — likely out of scope). Recommend shipping (1) as
Phase 1, then (2) behind a feature flag. **board.json stays a regenerated JSON projection** (keeps
agents + CLI + schema working) with the Loro oplog/snapshot in a **sidecar** (`board.loro`); an agent's
direct edit to board.json is imported as a structural diff → Loro ops, not a wholesale replace.
Presence/awareness must be kept OUT of the persisted document.

**Files:** `crates/brainstorm-types/src/lib.rs`, `crates/brainstorm-types/Cargo.toml`,
`src/interaction.rs`, `src/app.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `Cargo.toml`,
`Cargo.lock`, `src-tauri/capabilities/default.json`, `src-tauri/tauri.conf.json`,
`.claude/skills/infinite-brainstorm/board.schema.json`, `src-tauri/tests/board_roundtrip.rs`,
`src-tauri/tests/watcher.rs`

**Risks:** identity crisis of board.json (the documented agent-native API + CLI surface + schema —
making a Loro binary the source of truth without a JSON projection breaks the Claude-edits-the-file
workflow, the watcher contract, and 20+ templates); no transport tier exists at all (the dominant cost,
easy to under-scope as "just add a library"); self-write suppression + whole-file reload assume
single-writer LWW (watcher must change from "reload entire board" to "import remote ops/merge");
Loro adds a non-trivial dep to a size-optimized release binary (`opt-level z` + LTO + `panic=abort`) AND
the wasm32 build (bundle size + panic=abort interaction need verification); concurrent text fidelity
(register = LWW loses merge; `LoroText` adds cost + reworks undo); undo/redo
(`src/history.rs` bounded VecDeque of snapshots) is per-client and doesn't compose with CRDT version
vectors; presence/awareness must stay out of board.json (trips `validate()`/`unknown_top_level_keys`);
security — a network transport adds auth/join/trust-of-incoming-ops (CSP `connect-src` currently only
`'self' ipc: https:` so websockets need an explicit policy change).

**Open questions (human decisions):**
- **Scope of "multi-client":** same-machine (UI + agent + multiple windows, solvable via the file as a
  merge point) vs genuine remote network clients (requires a relay/server tier)? **Radically different
  efforts — pick one for v1.**
- Where does the Loro document live relative to board.json? (Recommend: board.json stays a regenerated
  JSON projection, sidecar `board.loro` holds the oplog. Confirm, or accept breaking the agent-native
  file contract.)
- Transport if remote is in scope: embedded Tauri sidecar relay, standalone Rust websocket server
  (tokio already a dep), or P2P? And where is it hosted?
- Node text model: LWW register (simple, no concurrent-text merge) vs per-node `LoroText` (true text
  CRDT, higher cost, reworks `EditText`/`EditMarkdown` + undo)?
- How is an agent's direct board.json edit reconciled — structural diff-to-ops on the watcher event, or
  a dedicated import command?
- Multi-client undo/redo semantics: keep per-client snapshot history (and accept divergence) or
  redesign around Loro versions?
- Does the browser/localStorage build participate in real-time collaboration, or Tauri-only for v1?
  (Drives Loro WASM bundle-size + browser-websocket work.)
- Presence/awareness: in scope for this item, or strictly the document-merge layer? If in scope,
  confirm it rides an ephemeral channel separate from persisted state.

---

## 3. Recommended order

Ordered by a blend of (a) unblocking already-built work, (b) effort/ROI, and (c) dependency.

1. **Merge `feat/launcher-fix` (1a)** — clean, merge-ready now. Decide the `export` passthrough question
   in tandem with the next step.
2. **Fix + merge `feat/headless-export` (1b)** — apply the one-line `xml_escape(border)` blocker fix +
   a malicious-color regression test, then the zoom clamp + re-sync the installed launcher. Merging this
   retroactively makes the `export` passthrough/docs in 1a correct. (If you instead defer 1b, strip
   `export` from 1a's script + the two doc lines first.)
3. **Keyboard navigation (2a, S)** — smallest sketch, pure frontend, fully unit-testable, no
   cross-cutting risk. Highest ROI per unit of effort.
4. **Semantic zoom (2b, M)** — self-contained rendering improvement; main risk (two-surface desync) is
   well-understood and guarded by a single shared `ZoomTier` helper.
5. **Themes / light mode (2c, M)** — valuable and mostly mechanical, but real design work and palette
   drift across three sources. Do after 2b so the `Theme`-in-`brainstorm-types` decision can account
   for any `summary`-field schema churn.
6. **Touch / tablet (2d, M)** — **gated on answering "what is the target device?"** Don't start until
   that's resolved; the work may be partly moot (iPad isn't a Tauri target).
7. **Multi-board switcher (2e, L)** — large, touches watcher/self-write/scope invariants. Worthwhile but
   not before the M-sized wins land.
8. **CRDT / Loro (2f, XL)** — by far the largest and riskiest; reframes the persistence model and needs
   a from-scratch transport tier. Only after the scope decision (same-machine vs remote) and only if
   real-time collab is a committed product goal. Phase it (file-as-merge-point first).

---

## 4. Human decisions needed (checklist)

**Merge gating (do first):**
- [ ] Approve merging `feat/launcher-fix` (no blockers).
- [ ] Decide the `export` passthrough: merge `feat/headless-export` alongside it, OR strip `export`
      from `scripts/brainstorm` + `CLAUDE.md:36` + `README.md:283`.
- [ ] Approve the `xml_escape(border)` blocker fix (+ malicious-color regression test) on
      `feat/headless-export` before merge.
- [ ] Decide whether to also fix the explicit-`--camera` zoom clamp (`0.1..=5.0`) before merge
      (recommended).
- [ ] Confirm re-syncing the installed `~/.local/bin/brainstorm` after the launcher change (or
      `validate`/`query`/`export` stay broken from the installed CLI).

**Keyboard navigation (2a):**
- [ ] Directed vs undirected traversal (recommend undirected).
- [ ] No-selection / multi-selection behavior.
- [ ] Tie-break / direction-matching rule (for a deterministic test).
- [ ] Recenter policy (every step / off-screen-only / never).
- [ ] Node→node only, or also edges?
- [ ] `Shift+Arrow` semantics in v1?

**Semantic zoom (2b):**
- [ ] **Label source: first-line-of-`text` vs new `Node.summary` field** (recommend `summary` +
      fallback).
- [ ] Number of tiers + zoom thresholds.
- [ ] Collapsed tier: draw type indicator + border color, or box+label only?
- [ ] Meta at Summary or Full-only?
- [ ] md/link/image node treatment at Summary.
- [ ] Gate edge labels at low zoom (in or out of scope)?

**Themes / light mode (2c):**
- [ ] v1: Gotham + one light, or an N-theme registry (decides toggle shape).
- [ ] Where the canonical `Theme` palette lives (`brainstorm-types` vs canvas-local).
- [ ] Default + first-run behavior (`prefers-color-scheme` auto-detect?).
- [ ] Persist theme per-board or globally (recommend global).
- [ ] Hand-designed light palette vs derive-then-iterate.
- [ ] Should Export PNG / future SVG exporter respect the active theme?

**Touch / tablet (2d):**
- [ ] **What is the actual target device/runtime?** (Gates everything else here.)
- [ ] v1 gesture scope.
- [ ] Touch replacements for modifier-key actions (multi-select, box-select, edge creation).
- [ ] Pointer Events unified path vs separate TouchEvent layer.
- [ ] Enlarge hit areas for coarse pointers now or later.
- [ ] Acceptance gate: on-device manual testing, and on which device.

**Multi-board switcher (2e):**
- [ ] Board discovery model (any-JSON / convention / manifest).
- [ ] Same-directory vs cross-directory boards.
- [ ] Browser-mode multi-board or Tauri-only for v1.
- [ ] CRUD (create/rename/delete) in v1?
- [ ] Persist last-active board + add CLI `board` flag?
- [ ] Watcher re-target mechanism (control channel vs filename mutex).

**CRDT / Loro (2f):**
- [ ] **Scope: same-machine vs genuine remote multi-client** (radically different efforts).
- [ ] board.json as a regenerated JSON projection + sidecar `board.loro` (confirm or accept breaking
      the file contract).
- [ ] Transport choice if remote (sidecar relay / standalone ws server / P2P) + hosting.
- [ ] Node text model (LWW register vs per-node `LoroText`).
- [ ] Reconciling agent-direct board.json edits (diff-to-ops vs import command).
- [ ] Multi-client undo/redo semantics.
- [ ] Does the browser build participate, or Tauri-only for v1.
- [ ] Presence/awareness in scope, and on an ephemeral channel.
