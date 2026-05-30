# Infinite Brainstorm — Deep Analysis & Improvement Plan (2026-05-29)

## Executive Summary

Infinite Brainstorm is a well-architected ~4,800-LOC agent-native canvas with a clean dual-storage model and a working file-watcher sync loop — but its **headline feature ("JSON is the API") is also its single biggest liability**. The frontend swallows every board.json parse error into an empty `Board::default()` (`src/app.rs:37`), and combined with non-atomic saves (`src-tauri/src/lib.rs:107-108`) and an unconditional file-watcher reload (`src/app.rs:301`), a single malformed agent edit can silently blank the canvas and then overwrite the user's only data file on the next click — total, unrecoverable data loss with no warning. On top of that, the entire backend is exposed to **stored XSS via unsanitized markdown → full-IPC compromise** (csp:null + withGlobalTauri + `**` fs scope), and there is **zero CI** guarding the two hand-duplicated type copies or the untested core round-trip. None of this is currently *broken* — the live 112-node board works fine and the code is idiomatic — but the safety net under the agent-native contract is missing. This plan sequences the critical data-safety and security fixes first (P0), then the structural refactors (shared types crate, reducer/persistence layer) that make the rest cheap and testable, then performance and UX. Overall health: **solid foundation, dangerous gaps in robustness, security, and verification.**

- Total findings analyzed: **115** (39 confirmed against code, 76 carried-forward medium/low, 0 refuted)
- The confirmed-critical cluster (F11/F48/F62/F73/F89/F111) is a single root cause described six ways: swallowed parse error + non-atomic write + unguarded reload.

---

## Themes

These eight cross-cutting themes organize all 115 findings. Most P0/P1 work resolves multiple findings at once because the confirmed-critical findings are heavily overlapping descriptions of the same few root causes.

### T1. Agent-native data-safety (the load/save/watch round-trip is not crash-safe)
The flagship workflow — agents editing board.json directly — has no safety net. Swallowed parse errors (`app.rs:37`), non-atomic writes (`lib.rs:107-108`), and an unguarded watcher reload (`app.rs:301`) chain into silent total data loss. No validation tooling, no atomic write, no backup, no "load failed" surface.
Findings: **F11, F48, F62, F73, F89, F111** (all confirmed-critical/high), plus F49, F50, F63, F64, F65, F69, F70, F91, F93, F95.

### T2. Security: untrusted board.json is a remote-code / file-read surface
board.json is explicitly untrusted input (agent/shared-writable), yet it drives unsanitized markdown rendering (stored XSS), arbitrary local file reads, and unauthenticated SSRF — all auto-triggered on load with no click. csp:null + withGlobalTauri + `**` fs scope turn any injected script into full local compromise.
Findings: **F84, F85, F86, F87, F88** (all confirmed-high), plus F90, F91, F92, F97, F98.

### T3. Frontend/backend type duplication (no shared crate, no parity enforcement)
Board/Node/Edge/LinkPreview are hand-duplicated across two crates with identical serde attributes but already-divergent derives (frontend has PartialEq, backend doesn't). Nothing enforces parity; 38 struct-literal sites must be hand-synced per field.
Findings: **F1, F15** (confirmed), plus F75, F9, F71.

### T4. Zero CI / untested core seams
No CI of any kind. The two highest-risk seams — cross-crate type parity and the agent round-trip — have zero automated coverage. 128 in-file tests cover only pure helpers and happy-path serde. The file-watcher + SKIP_NEXT_EMIT guard is completely untested.
Findings: **F72, F73, F74, F75** (confirmed), plus F76, F77, F78, F79, F80, F81, F82, F83, F9.

### T5. God-component architecture (app.rs is 1526 lines, all mutation logic inline & untestable)
`App()` owns ~18 signals, 6 Effects, and every handler as 145-175-line inline closures mixing hit-testing, history, persistence, and clipboard. No mutation logic is DOM-free testable. History is owned by the wrong layer (text edits bypass it entirely → undo silently loses typed text). Persistence is a 17-site fire-and-forget scatter.
Findings: **F2, F3, F4** (confirmed-high), plus F5, F6, F7, F8, F52, F53, F54, F109, F114, F115.

### T6. Canvas performance at scale (full immediate-mode redraw, no batching, O(E·N) edges)
Every signal change triggers a full synchronous repaint of all nodes+edges with no requestAnimationFrame coalescing and no viewport culling. Edge rendering does two O(N) linear scans per edge per frame. Per-frame text re-measurement and font-string allocation. Fine at 112 nodes; collapses at 1k+.
Findings: **F34, F35, F36, F37, F38** (confirmed), plus F10, F21, F30, F39, F40, F41, F42, F43, F44, F45, F46, F47, F61.

### T7. Rust idiom & robustness hygiene (panics, unwraps, byte-slicing, cwd coupling)
Byte-slice truncation panics the renderer on multibyte filenames (`canvas.rs:317`). `Camera::default()` yields zoom=0.0 (latent divide-by-zero). Stringly-typed `node_type`. Scattered `unwrap()` in WASM, `.expect()` in the watcher thread, cwd-derived paths.
Findings: **F12, F13, F14** (confirmed), plus F16, F17, F18, F22, F23, F24, F25, F27, F28, F29, F31, F33, F57, F59, F60, F90, F94, F96.

### T8. UX & feature completeness (navigation, recovery, discoverability)
At 112 nodes there is no search, no fit-to-view, no minimap, no select-all, no camera persistence, no desktop export, no Escape-to-close on modals. The mouseleave-as-mouseup wiring cuts off drags at screen edges.
Findings: **F99, F100, F102, F103** (confirmed-medium), plus F101, F104, F105, F106, F107, F108, F110, F112, F113, F20, F58.

---

## Prioritized Roadmap

Steps use the format `[Step] -> verify: [check]`.

---

### P0 — Critical data-safety & security (do first; these can lose data or compromise the machine)

#### P0.1 — Stop swallowing parse errors into an empty board (the data-loss root cause)
- **Resolves:** F11, F48, F62, F73 (in part), F89 (frontend half), F111
- **Files:** `src/app.rs:34-45` (`load_board_storage`), `src/app.rs:278` (initial load), `src/app.rs:301` (watcher reload), the localStorage branch `app.rs:42-43`
- **Effort:** M · **Impact:** Critical (closes silent total data loss on the primary workflow)
- **Steps:**
  1. Change `load_board_storage` to return `Result<Board, LoadError>` (or an enum `Loaded(Board) | Absent | ParseError(String)`) instead of `unwrap_or_default()` -> verify: `cargo check` from root passes; no remaining `unwrap_or_default()` on the load path (`grep -n unwrap_or_default src/app.rs` returns nothing).
  2. At both call sites (`app.rs:278` initial, `app.rs:301` watcher), on `ParseError` keep the existing `board` signal and do NOT call `set_board.set(...)`; only `Absent`/empty yields `Board::default()` -> verify: manual repro — open app, `echo '{' > board.json`, confirm canvas does NOT blank.
  3. Mirror the exact same change on the localStorage branch (`app.rs:42-43`, currently `serde_json::from_str(...).ok()...unwrap_or_default()`) -> verify: browser-mode smoke test (corrupt localStorage value) keeps prior board.
  4. Add a non-blocking error banner component (new `src/components/error_banner.rs`) wired to a `load_error: RwSignal<Option<String>>` signal, shown when parse fails -> verify: banner renders the serde message; clears on next successful load.
- **Verification (done when):** With the app open, writing malformed JSON to board.json shows the banner and leaves the canvas intact; the next user action does NOT overwrite the on-disk file. Add a `wasm-bindgen-test` asserting a malformed payload does not replace a non-empty `board` signal (ties into P1.5).

#### P0.2 — Atomic save_board (temp file + fsync + rename)
- **Resolves:** F26, F89 (backend half), F48 (mid-write race), F111 (partial)
- **Files:** `src-tauri/src/lib.rs:93-110` (`save_board`)
- **Effort:** S · **Impact:** Critical (closes truncation-on-crash and mid-write-read corruption)
- **Steps:**
  1. Write serialized JSON to `board.json.tmp` in the same directory, `file.sync_all()` (fsync), then `fs::rename(tmp, board.json)` -> verify: `cargo check` from `src-tauri/` passes; backend test writes then kills mid-write equivalent — the target file is never observed truncated.
  2. Optionally write the prior contents to `board.json.bak` before rename -> verify: `.bak` exists and round-trips after a save.
  3. Keep `SKIP_NEXT_EMIT` arming but move the `store(true)` to immediately before the `rename` (the atomic commit point), not before the tmp write -> verify: file-watcher does not emit a spurious reload for the app's own atomic save (manual: save in-app, confirm no flash).
- **Verification (done when):** A new `src-tauri/tests/atomic_save.rs` (using `tempfile::tempdir`, ties to P1.5) asserts the temp file is created and renamed, and that an external reader never sees partial content. `cargo test --manifest-path src-tauri/Cargo.toml` passes.

#### P0.3 — Sanitize markdown HTML + restrictive CSP (stored XSS → full compromise)
- **Resolves:** F86, F87 (CSP half)
- **Files:** `src/app.rs:144-149` (`parse_markdown`), `src/components/markdown_overlays.rs:54`, `src/components/markdown_modal.rs:145`, `src-tauri/tauri.conf.json:24` (csp), `Cargo.toml` (add `ammonia`)
- **Effort:** M · **Impact:** High (closes RCE-equivalent: injected script can call invoke load_board/save_board/read_markdown_file/fetch_link_preview)
- **Steps:**
  1. Add `ammonia` as a frontend dependency and post-process `parse_markdown`'s `push_html` output through an allowlist sanitizer before it reaches any `inner_html` -> verify: `cargo check` passes; a node with text `<img src=x onerror=alert(1)>` renders inert (no `onerror` attribute in output). NOTE: ask the user before `cargo add ammonia` per the install-permission rule.
  2. Set a restrictive CSP in `tauri.conf.json:24`: `default-src 'self'; script-src 'self'; img-src 'self' data: asset: https:; connect-src 'self' ipc: https:; object-src 'none'; frame-src 'none'` -> verify: app still loads and renders; DevTools console shows no CSP violations on normal use; an inline `<script>` in a md node does not execute.
  3. Confirm both inner_html sinks (overlays + modal) receive only sanitized HTML -> verify: `grep -n inner_html src/components/` — every site traces back to the sanitized `parse_markdown`.
- **Verification (done when):** A md node containing `<script>` / `<img onerror>` neither executes nor injects active markup, and CSP blocks inline script as defense-in-depth.

#### P0.4 — Scope filesystem-reading IPC commands to the board directory + size cap
- **Resolves:** F85, F88, F98
- **Files:** `src-tauri/src/lib.rs:210-240` (`read_image_base64`), `src-tauri/src/lib.rs:242-261` (`read_markdown_file`), `src-tauri/capabilities/default.json:17-26` (fs scope), `src-tauri/tauri.conf.json:25-28` (assetProtocol scope)
- **Effort:** M · **Impact:** High (closes arbitrary local file read of e.g. ~/.ssh/id_rsa via crafted board.json)
- **Steps:**
  1. In both read commands, reuse the existing `delete_asset` canonicalize + `starts_with` pattern (`lib.rs:271-279`) to reject paths resolving outside the board directory + an explicit allowlist (the vault path) -> verify: a board.json image node pointing at `/etc/passwd` returns Err, not base64.
  2. Add a byte-size cap (e.g. 25 MB) in `read_image_base64` before `fs::read` -> verify: reading an oversized file returns Err without allocating the full buffer.
  3. Validate magic bytes (via `image::guess_format` / `infer`) and derive MIME from detected format, not extension (`lib.rs:222-234`) -> verify: a `.png`-named text file is rejected.
  4. Tighten `fs:scope` (`default.json:24`) and `assetProtocol.scope` (`tauri.conf.json:27`): drop the `**`, `$HOME/**`, `/Users/**` globs; scope to the board/project dir + narrow allowlist; add a deny list for `$HOME/.ssh`, `$HOME/.aws`, keychains -> verify: app still loads local images from the board dir; reading outside the scope is denied by the permission layer.
- **Verification (done when):** `read_markdown_file`/`read_image_base64` refuse paths outside the approved root, and the Tauri scope no longer grants whole-filesystem access. Manual: crafted board.json cannot exfiltrate a file outside the board dir.

#### P0.5 — SSRF guard on fetch_link_preview (IP-range policy at connect time + redirect cap)
- **Resolves:** F84, F91
- **Files:** `src-tauri/src/lib.rs:118-187` (`fetch_link_preview`), `src/app.rs:396-422` (auto-fetch effect)
- **Effort:** M · **Impact:** High (closes no-click SSRF to cloud metadata / RFC1918 on board load, with read-back oracle)
- **Steps:**
  1. Reject loopback, link-local (169.254.0.0/16, fe80::/10), RFC1918, CGNAT (100.64/10), ULA at the connected-IP level (custom resolver / connect callback), applied on every redirect hop — NOT a one-time hostname parse (DNS-rebinding safe) -> verify: `fetch_link_preview("http://169.254.169.254/...")` returns Err; a public URL that 302s to an internal host is rejected on the hop.
  2. Set `reqwest` `.redirect(Policy::limited(3))` and re-validate host on each hop; normalize/reject decimal/hex/IPv6-mapped IP literals -> verify: `http://2130706433/` and `http://[::ffff:127.0.0.1]/` are rejected.
  3. Cap response body (stream with a running byte total, ~2 MB) before `Html::parse_document` (folds in F91) -> verify: a large/streaming body is truncated, not OOM.
  4. Gate auto-fetch (`app.rs:402-411`) behind explicit opt-in for non-allowlisted hosts, or only fetch on user interaction -> verify: opening a board with a link node to an internal host does not auto-fetch.
- **Verification (done when):** Loading a board containing `{"node_type":"link","text":"http://169.254.169.254/..."}` issues no internal request; redirects to private ranges are blocked.

---

### P1 — Structural foundations (make the rest cheap, testable, and safe)

#### P1.1 — Extract a shared `brainstorm-types` workspace crate
- **Resolves:** F1, F15, F75 (structurally), F71 (Rust half), F9 (parity half)
- **Files:** new `crates/types/` (or `brainstorm-types/`), `src/state.rs:15-167`, `src-tauri/src/lib.rs:15-65`, both `Cargo.toml`s, root workspace `members`
- **Effort:** M · **Impact:** High (eliminates the highest-maintenance coupling; drift becomes a compile error)
- **Steps:**
  1. Create the crate with Board/Node/Edge/LinkPreview/Camera/ResizeHandle + serde derives + `default_node_type` + geometry (`auto_size`, `contains_point`, `resize_handle_at`, `screen_to_world`, `world_to_screen`) + constants (`RESIZE_HANDLE_SIZE`, `MIN_NODE_*`) moved verbatim -> verify: the crate compiles standalone (`cargo check -p brainstorm-types`).
  2. Derive `PartialEq` unconditionally on the shared types (frontend needs it for Leptos signal equality; harmless on backend) -> verify: frontend signal `Effect`s still compile.
  3. `pub use brainstorm_types::*;` in `state.rs` and re-export from `lib.rs` so all call sites are unchanged -> verify: both crates `cargo check` (root + src-tauri) with zero call-site edits.
  4. Move the ~38 struct-literal test fixtures into the shared crate's tests -> verify: `cargo test -p brainstorm-types` passes; per-crate serde tests reduced.
- **Verification (done when):** Adding a field to the shared Node and forgetting nothing is impossible — there is one definition. Both crates build; `grep -rn "struct Node" src/ src-tauri/src/` shows the type defined only in the shared crate.

#### P1.2 — Centralize persistence behind one debounced, dirty-tracked sink
- **Resolves:** F3, F37, F22 (save-path unwrap), part of F49 (dirty guard)
- **Files:** `src/app.rs:47-58` (`save_board_storage`), the 17 call sites (`app.rs:833,863,897,982,1009,1019,1035,1067,1129,1149,1220,1253` + `node_editor.rs:37,59,97,120` + `markdown_modal.rs:66`), `src-tauri/src/lib.rs:107` (compact vs pretty)
- **Effort:** M · **Impact:** High (collapses 17 duplicated spawn_local blocks; gives one place for the dirty guard; O(time) saves not O(actions))
- **Steps:**
  1. Add a `request_save()` that marks `dirty` and schedules a single trailing-edge debounced write (~200-250ms via `gloo-timers`), coalescing bursts -> verify: dragging 5 nodes then releasing produces exactly one disk write (instrument with a console counter).
  2. Replace all 17 direct `save_board_storage` call sites with `request_save()`; remove the `use crate::app::save_board_storage` imports from components -> verify: `grep -rn save_board_storage src/components/` returns nothing; `cargo check` passes.
  3. Switch the on-disk format to compact `serde_json::to_string` (the file is machine-read by agents per CLAUDE.md) at `lib.rs:107` -> verify: board.json is valid JSON, smaller; round-trips.
  4. Set a `local_edit_pending` flag the watcher reload checks before clobbering (feeds P1.4) -> verify: an in-flight save is not overwritten by a watcher reload.
- **Verification (done when):** One debounced sink owns all saves; components dispatch instead of importing app internals; rapid edits coalesce to one write.

#### P1.3 — Introduce a reducer/interaction layer + fix the undo ownership bug
- **Resolves:** F2, F4, F52, F109, F53, F54, F114, F115 (selection capture)
- **Files:** new `src/interaction.rs` (`BoardAction` enum + `reduce`/`apply`), `src/app.rs:544-1164` (handlers), `src/history.rs`, `src/components/node_editor.rs`, `src/components/markdown_modal.rs`
- **Effort:** L · **Impact:** High (makes mutation logic DOM-free testable; one place for history.push; fixes undo silently dropping text edits)
- **Steps:**
  1. Define `BoardAction` (MoveNodes, ResizeNode, CreateEdge, CreateNode, DeleteSelected, CycleType, PasteNodes, EditText, EditMarkdown) and a pure `reduce(board, action) -> (board, Vec<SideEffect>)` where SideEffect = {DeleteAsset(path), RequestSave} -> verify: unit tests in `interaction.rs` exercise each action with no DOM (`cargo test` reaches them).
  2. Route every mutation through one `apply(action)` that pushes history ONCE, runs reduce, sets the signal, and schedules side effects -> verify: the 9 scattered `history.borrow_mut().push()` sites collapse to one; `grep -c "history.borrow_mut().push" src/app.rs` drops to ~1.
  3. Migrate the keyboard handler first (delete/copy/paste/type-cycle — most self-contained; model delete's asset cleanup as a SideEffect from day one), then drag/resize -> verify: existing behavior unchanged via manual smoke + new action tests.
  4. Add the `EditText`/`EditMarkdown` actions so `node_editor.rs` and `markdown_modal.rs` dispatch through `apply` and thus snapshot history (fixes F52/F109) -> verify: type into a node, blur, Cmd+Z restores the pre-edit text as a discrete undo step (previously it skipped the edit).
  5. Change history to `History<(Board, HashSet<String> selection)>` so undo/redo restore selection instead of clearing it (`app.rs:1006-1007/1016-1017`) -> verify: undo a multi-node move re-selects those nodes.
  6. Defer the history snapshot to the first move/resize (not mouse-down) so a plain click doesn't create a junk undo entry (F53/F114) -> verify: click a node 5× without dragging, Cmd+Z once reaches the real prior action.
- **Verification (done when):** Mutation logic is unit-tested without WASM; text/markdown edits are independently undoable; undo restores selection; no phantom undo entries.

#### P1.4 — Guard the file-watcher reload against in-flight edits + fix SKIP_NEXT_EMIT
- **Resolves:** F49, F50, F93
- **Files:** `src/app.rs:284-309` (watcher Effect), `src-tauri/src/lib.rs:13,105,397-411` (SKIP_NEXT_EMIT + debounce)
- **Effort:** M · **Impact:** High (closes lost-update clobbering during drag/resize and spurious reloads after own saves)
- **Steps:**
  1. In the watcher reload, skip/defer `set_board.set` when `drag_state.is_dragging || resize_state.is_resizing || edge_creation.is_creating || editing_node.is_some() || local_edit_pending` (the flag from P1.2); re-run after the interaction ends -> verify: start dragging a node, have an external process append a node, confirm the appended node survives mouse-up (no clobber).
  2. Replace the single-shot `SKIP_NEXT_EMIT` bool with content-hash suppression: store the last-saved content hash on save; the watcher skips emit when on-disk content matches the last self-write hash -> verify: the app's own save never triggers a reload regardless of how many notify events fire; an external edit always does.
  3. At minimum (interim) record `last_emit = Some(now)` even on the skip branch so the debounce covers trailing duplicate events (`lib.rs:399`) -> verify: a single save producing 2 notify events does not double-emit.
- **Verification (done when):** External edits during an active drag are not clobbered; the app's own atomic save produces zero spurious reloads. New backend test for the should-emit decision (P1.5).

#### P1.5 — Add CI + extract testable core + round-trip/watcher tests
- **Resolves:** F72, F73, F74, F75, F76, F78, F79, F80, F81, F82, F83, F9
- **Files:** new `.github/workflows/ci.yml`, new `rust-toolchain.toml`, new `src-tauri/tests/board_roundtrip.rs`, new `src-tauri/tests/watcher.rs`, `.gitignore` (Cargo.lock line), `src-tauri/src/lib.rs` (extract `load_board_at(path)`, `should_emit_change(...)`)
- **Effort:** M · **Impact:** High (the only mechanism that makes the manual-sync contract and the round-trip safe)
- **Steps:**
  1. CI workflow on push/PR with separate jobs: `cargo fmt --all --check`; `cargo clippy --all-targets -- -D warnings` for both crates; `cargo test` (frontend) and `cargo test --manifest-path src-tauri/Cargo.toml` (backend); `rustup target add wasm32-unknown-unknown` + `trunk build` -> verify: CI is green on a clean checkout; a deliberately divergent type change makes a job fail.
  2. Add `rust-toolchain.toml` pinning the channel + wasm32 target so the wasm-bindgen 0.2.108 pin can't drift; add `--locked` to CI test/build -> verify: CI uses the pinned toolchain.
  3. Remove `Cargo.lock` from `.gitignore` (it is tracked but listed as ignored — contradictory) -> verify: `git check-ignore Cargo.lock` returns nothing.
  4. Refactor `load_board`/`save_board` so the IO core is `pub(crate) fn load_board_at(path: &Path) -> Result<Board,String>` (path derives from cwd via `get_board_path`, no AppHandle needed) -> verify: a `tests/board_roundtrip.rs` using `tempfile::tempdir` writes a known board, loads it, asserts equality; saves, asserts identical re-deserialize; a negative test asserts malformed JSON returns Err.
  5. Extract `should_emit_change(skip, last_emit, now, debounce) -> bool` and unit-test swap-consume-once + debounce; add a tempdir watcher integration test (own-save = no emit, external write = emit), driving emissions through an injected channel -> verify: `cargo test --manifest-path src-tauri/Cargo.toml` covers the watcher decision.
  6. Add a committed golden JSON fixture (all metadata fields + edge label) and assert it serializes byte-identically in BOTH crates' test suites; add `PartialEq` to backend structs (subsumed once P1.1 lands) -> verify: a serde-attribute divergence in either crate fails CI.
  7. Replace fixed-name temp files in `lib.rs:868-916` tests with `tempfile::tempdir()` -> verify: parallel `cargo test` no longer collides/leaks.
- **Verification (done when):** CI runs fmt+clippy+test for both crates + a wasm build on every PR; the agent round-trip and watcher guard have tests; type parity is enforced.

---

### P2 — Performance, robustness hygiene, and high-value UX

#### P2.1 — requestAnimationFrame render coalescer + viewport culling
- **Resolves:** F34, F36 (hot-path half), F40, F43, F45, F46
- **Files:** `src/app.rs:495-542` (render Effect), `src/canvas.rs:37-78` (`render_board`)
- **Effort:** M · **Impact:** High (single highest-ROI perf change: bounds redraws to refresh rate regardless of event rate)
- **Steps:**
  1. On signal change, set a `dirty` flag + schedule a single rAF callback (web_sys `request_animation_frame` + `Closure`, guarded by a `Cell<bool>` "scheduled"); render at most once per frame -> verify: dragging fires one render per frame, not per mousemove (instrument a frame counter).
  2. Add viewport culling: skip nodes/edges whose screen bounds fall outside the canvas rect -> verify: per-frame draw cost scales with on-screen elements (profile with a synthetic 2k-node board).
  3. Early-out `draw_groups` when no node has a `group` and cache group bounds by a mutation generation counter (F46) -> verify: panning a group-less board skips the group pass.
- **Verification (done when):** A 1k-node board stays at display refresh rate during drag/pan; redundant intra-frame invalidations collapse to one render.

#### P2.2 — Build a node-id HashMap per render/handler (kill O(E·N))
- **Resolves:** F35, F10, F21, F41
- **Files:** `src/canvas.rs:476-477` (`draw_edge`), `src/canvas.rs:545` (`draw_edge_preview`), `src/app.rs:648-650` (edge hit-test)
- **Effort:** S · **Impact:** High (O(E·N) → O(E+N), mechanical, mirrors existing `draw_groups` map)
- **Steps:**
  1. Build `HashMap<&str,&Node>` once at the top of `render_board` and pass it to `draw_edge`/`draw_edge_preview` for O(1) endpoint lookup -> verify: edge rendering no longer calls `nodes.iter().find` (`grep -n "iter().find" src/canvas.rs` for edges returns nothing).
  2. Build the same map once in the edge hit-test branch of `on_mouse_down` -> verify: a click on empty canvas does O(E) endpoint resolution, not O(E·N).
- **Verification (done when):** `grep -n "nodes.iter().find" src/canvas.rs` shows edge endpoints resolved via the map; a 2k-node board click/redraw profile shows no quadratic scan.

#### P2.3 — Char-safe truncation + Camera::default fix + NodeType enum
- **Resolves:** F12, F57, F90, F68 (byte-slice panic), F16, F60, F79 (Camera), F14, F18, F67 (node_type/priority)
- **Files:** `src/canvas.rs:316-321`, `src/state.rs:140-167` (Camera), `src/state.rs:31-32` (node_type), `src/state.rs:42` + `src/canvas.rs:243` (priority)
- **Effort:** M · **Impact:** Medium-High (removes a render-killing panic; removes a latent divide-by-zero; enforces the node_type invariant)
- **Steps:**
  1. Replace the byte-slice truncation with `filename.chars().count() > 20` guard + `filename.chars().take(17).collect::<String>()` -> verify: a unit test on `"abcdefghijklmnop😀extra.png"` no longer panics; `grep -rn '\[\.\.' src/ src-tauri/src/` shows no other byte-indexed str slices.
  2. Hand-implement `Default for Camera` delegating to `new()` (zoom=1.0), or remove the derive; add a `debug_assert`/clamp that zoom is finite and > 0 in `screen_to_world` -> verify: a test asserts `Camera::default().zoom == 1.0`.
  3. Introduce `enum NodeType` with `#[serde(rename_all="lowercase")]` + `#[serde(other)] Unknown` (preserves agent-forward-compat), replacing `node_type: String`; update the ~16 match sites across both crates (lives in shared crate after P1.1) -> verify: matches become exhaustive (compiler catches a missing arm); `cargo test` cycle-type test still passes. Scope to node_type only — leave `status` freeform per CLAUDE.md.
  4. Use `priority.clamp(1,5)` at `canvas.rs:243` so `priority:0` never renders `P0` -> verify: a node with `priority:0` renders P1, not P0.
- **Verification (done when):** Multibyte filenames cannot panic the renderer; `Camera::default()` is safe; node_type typos are caught at parse (or routed to Unknown) instead of silently rendering as text.

#### P2.4 — Search, fit-to-view, select-all, camera persistence (navigation recovery)
- **Resolves:** F99, F103, F105, F102
- **Files:** `src/app.rs:988-1164` (on_keydown), `src/app.rs:186/249` (set_camera), `src/state.rs:351` (node center), new search overlay component
- **Effort:** M · **Impact:** High (112-node board is currently unnavigable; agent-placed nodes can land off-screen with no recovery)
- **Steps:**
  1. Add Cmd/Ctrl+F search overlay: filter nodes by text/tags/status, highlight matches (reuse `selected_nodes`), Enter recenters camera on first match (reuse `set_camera` + `state.rs:351` center) -> verify: typing a node's text and pressing Enter pans+zooms to it.
  2. Add `F` = fit-all-nodes (camera to bounding box of all nodes with ~10% margin; zoom = min(canvas_w/bbox_w, canvas_h/bbox_h) clamped 0.1..5.0) and Cmd+0 = reset zoom -> verify: after an agent writes a node at (50000,50000), pressing F brings it into view.
  3. Add Cmd/Ctrl+A = select all node IDs -> verify: Cmd+A then Delete clears the board; Cmd+A then T cycles all types.
  4. Persist `{x,y,zoom}` to localStorage under a per-board key on pan-end/zoom-end; restore in the initial-load effect (`app.rs:278`, NOT the watcher path which already preserves camera) -> verify: relaunch via CLI restores the prior viewport.
- **Verification (done when):** A user can find any node, recover from an off-screen jump, select all, and relaunch without losing their place.

#### P2.5 — Fix mouseleave-as-mouseup (pointer capture) + modal Escape/keyboard
- **Resolves:** F20, F58, F113, F107
- **Files:** `src/app.rs:1304-1305` (mouseleave wiring), `src/app.rs:838-899` (mouse-up branches), `src/components/image_modal.rs`, `src/components/markdown_modal.rs`, `src/app.rs:991` (keydown gate)
- **Effort:** M · **Impact:** Medium (drags currently cut off at screen edges; modals can't be closed by keyboard)
- **Steps:**
  1. Use pointer capture (`setPointerCapture`) or attach mousemove/mouseup to document during an active drag so gestures continue off-canvas; give mouseleave its own handler that only resets transient cursor styling (no edge-create/box-select finalize, no save) -> verify: drag a node toward the window edge and back — the drag continues and saves once on release, not on edge-exit.
  2. Add a document-level keydown listener that closes the active modal on Escape; add a visible close (X) button to both modals -> verify: open the image/markdown modal, press Escape, it closes.
  3. Gate `on_keydown` to no-op when `modal_md.is_some() || modal_image.is_some()` (mirror the existing `editing_node` early-return at `app.rs:991`) -> verify: with a modal open, Delete/T/Cmd+Z do not mutate the hidden board.
- **Verification (done when):** Drags survive leaving the canvas; modals close on Escape and have a close button; canvas shortcuts are blocked while a modal is open.

#### P2.6 — Watcher thread resilience + path-source consistency + release profiles
- **Resolves:** F23, F95, F24, F25, F94, F39
- **Files:** `src-tauri/src/lib.rs:371,376,418-421` (watcher .expect/break), `src-tauri/src/lib.rs:67-77` (get_board_path), `src-tauri/src/lib.rs:264-285` (delete_asset), both `Cargo.toml` ([profile.release])
- **Effort:** M · **Impact:** Medium (silent sync death; misleading delete failures; 3.8MB wasm)
- **Steps:**
  1. Replace watcher `.expect()` (lib.rs:371/376) with logged Err returns + retry; on recv error `continue`/re-establish instead of `break`; emit a `watcher-down` event so the UI can warn -> verify: deleting board.json's parent dir at startup does not silently kill sync without a signal.
  2. Derive the assets dir in `delete_asset` from `get_assets_dir(&app)` (same source as `paste_image`), not cwd (lib.rs:268-269) -> verify: in the src-tauri-vs-root dev case, a paste-then-delete round-trip succeeds.
  3. Either remove the unused `AppHandle` from `get_board_path` or use Tauri path APIs; hard-error instead of falling back to `.` on `current_dir()` failure -> verify: launching from an unexpected cwd resolves board.json deterministically.
  4. Add `[profile.release] opt-level="z" lto=true codegen-units=1 strip=true panic="abort"` to both Cargo.toml -> verify: `cargo tauri build` succeeds and the wasm bundle shrinks 20-40% (compare `ls -l dist/*_bg.wasm`).
- **Verification (done when):** Watcher failures are visible and recoverable; asset deletes use the same path source as writes; release artifacts are optimized.

---

### P3 — Strategic / larger bets (sequence after foundations)

#### P3.1 — Board validator + JSON Schema + `brainstorm validate`/`query` CLI
- **Resolves:** F63, F64, F69, F77, F56, F18, F67 (validation surface), F70 (dry-run/diff)
- **Files:** new `Board::validate(&self) -> Vec<ValidationError>` in the shared crate, `src-tauri/src/main.rs` (add clap subcommands), new `.claude/skills/infinite-brainstorm/board.schema.json`, `SKILL.md`
- **Effort:** L · **Impact:** High (closes the agent contract: write-validate-commit instead of write-then-pray)
- **Steps:**
  1. Implement `Board::validate` detecting duplicate node IDs, duplicate edge IDs, edges referencing missing nodes, non-finite coords, out-of-range priority -> verify: unit tests for each invariant; a clean board returns empty.
  2. Add `brainstorm validate [path]` and `brainstorm query <expr>` clap subcommands in `main.rs` (reuse shared types) exiting non-zero with precise messages -> verify: `brainstorm validate` on a dangling-edge board exits 1 with the offending edge id.
  3. Ship a JSON Schema at `.claude/skills/infinite-brainstorm/board.schema.json` referenced from SKILL.md -> verify: a generic JSON Schema validator accepts the live board and rejects a missing-id node.
  4. Run `validate` (report + drop dangling edges with a console warning, non-destructive) on the file-watcher reload path so external edits are checked at land time -> verify: an agent edit with a dangling edge logs a warning instead of silently non-rendering.
  5. Add `version` field to Board (default current) for future migration; have validate warn (not reject) on unknown keys to preserve forward-compat -> verify: `"colour"` typo surfaces a warning.
- **Verification (done when):** An agent can `brainstorm validate` before committing a write; dangling edges/duplicate IDs/out-of-range values are reported; the schema is the documented single source of truth.

#### P3.2 — Split BoardCtx + RenderState struct + normalize_board helper
- **Resolves:** F5, F6, F7, F19, F76, F8, F32
- **Files:** `src/app.rs:181-200` (BoardCtx), `src/canvas.rs:37-49` (render_board signature), `src/app.rs:271-277/294-300` (auto-size loops)
- **Effort:** M · **Impact:** Medium (explicit coupling, type-safe render boundary, single load-normalization)
- **Steps:**
  1. Fold the duplicated auto-size loop into `Board::apply_auto_size(&mut self)` (shared crate) called by both load Effects and unit-tested -> verify: `cargo test` covers zero-dim sizing; the loop exists once.
  2. Replace `render_board`'s 11 positional args with a `RenderState<'a>` struct -> verify: the call site is one struct build; tuple-transposition is impossible (compiler enforces named fields).
  3. Split BoardCtx into SelectionCtx / EditingCtx / BoardDataCtx so components import only what they use (pairs with P1.3's dispatch fn) -> verify: ImageModal no longer depends on `md_file_cache`/board signals.
  4. Extract `event_world_pos(canvas_ref, camera, ev) -> Option<(f64,f64)>` (let-else, no unwrap) and call from all 6 handlers (folds F13/F8/F32) -> verify: `grep -c "canvas_ref.get().unwrap()" src/app.rs` drops to ~0.
- **Verification (done when):** Load normalization is one tested function; the render boundary is a named type; context coupling is explicit; coordinate boilerplate is centralized and panic-free.

#### P3.3 — Memoize markdown/wrapped-text, evict caches, command/diff undo log
- **Resolves:** F36 (memo half), F40, F47, F38, F61, F30, F44
- **Files:** `src/components/markdown_overlays.rs:33`, `src/canvas.rs:622-699` (wrap_text), `src/app.rs:311-493` (caches), `src/history.rs`
- **Effort:** L · **Impact:** Medium (per-interaction parse/measure cost; unbounded caches; full-board snapshot memory)
- **Steps:**
  1. Memoize parsed markdown HTML per (node_id, content) so pan/zoom only updates the transform, not the parse (`markdown_overlays.rs`) -> verify: panning a md-heavy board does not re-parse (instrument a parse counter).
  2. Memoize wrapped-text layout keyed on (node_id, text, width, zoom-bucket); hoist font strings per zoom level -> verify: dragging a text-heavy board does not re-run `measure_text` per frame.
  3. Evict image/link/md cache entries when their owning node is deleted; distinguish "loading" (None) from "failed" so failures can retry; LRU-bound the image cache -> verify: deleting nodes shrinks the caches; a previously-failed URL retries.
  4. Switch History `past`/`future` to `VecDeque` (O(1) trim), cap `future` at max_size, coalesce successive same-kind edits; longer-term, store diffs not full-board snapshots -> verify: `cargo test` history tests; memory profile of 100 edits drops.
- **Verification (done when):** Markdown/text layout is memoized; caches track current board membership; undo memory is bounded.

#### P3.4 — HiDPI rendering, minimap, image export, alignment/snapping, multi-board
- **Resolves:** F44, F104, F101, F106, F110, F112
- **Files:** `src/app.rs:509-518` (canvas sizing), `src/canvas.rs`, new minimap component, `src-tauri/src/lib.rs:67-77` (multi-board)
- **Effort:** L-XL · **Impact:** Medium (quality + feature completeness; sequence last)
- **Steps:**
  1. devicePixelRatio scaling (multiply backing store by dpr + `ctx.scale`) — land ONLY after P2.1 rAF/culling so the 4× pixel count doesn't worsen per-event repaint -> verify: text/strokes are crisp on Retina; FPS unchanged.
  2. Minimap (second small canvas drawing scaled node rects + viewport rect, click-to-recenter) reusing the render Effect -> verify: clicking the minimap recenters the main view.
  3. Image export: `canvas.to_data_url('image/png')` reusing the `on_download` anchor plumbing (viewport first, then full-board off-screen render) — add a Tauri-mode export/reveal button (F100, opener plugin already a dep) -> verify: Export PNG downloads a correct image; desktop mode has an export affordance.
  4. Snap-to-grid on drag release (round to 50px, matching the documented grid) + alignment guides -> verify: dragged nodes line up with agent-placed grid nodes.
  5. Multi-board (board-<name>.json + switcher) — additive, lowest priority, defer until navigation/export land -> verify: switching boards loads the right file.
- **Verification (done when):** Retina rendering is crisp, large boards have an overview, boards export to PNG, and human/agent layouts reconcile.

---

## Quick Wins (S-effort, high-impact — could ship today)

1. **Atomic save_board** (P0.2 / F26, F89) — temp file + fsync + rename in `lib.rs:93-110`. ~20 lines, closes truncation-on-crash data loss.
2. **Char-safe filename truncation** (P2.3 step 1 / F12, F57, F90) — `chars().take(17)` in `canvas.rs:316-321`. Removes a render-killing WASM panic on multibyte image paths.
3. **node-id HashMap in render_board** (P2.2 / F35, F21) — mirror the existing `draw_groups` map; O(E·N) → O(E+N) for edge rendering and hit-testing.
4. **Cmd/Ctrl+A select-all** (P2.4 step 3 / F105) — one match arm in `on_keydown` next to the copy handler; makes existing bulk ops reachable.
5. **`Camera::default()` zoom=1.0** (P2.3 step 2 / F16, F60) — hand-impl Default or drop the derive; removes a latent divide-by-zero.
6. **`priority.clamp(1,5)`** (P2.3 step 4 / F18, F67) — one-char fix at `canvas.rs:243` so `priority:0` never renders P0.
7. **Cargo.lock un-ignore** (P1.5 step 3 / F82, F96) — remove the contradictory `.gitignore` line; the file is already tracked.
8. **Release profiles** (P2.6 step 4 / F39) — `[profile.release]` block in both Cargo.toml; 20-40% smaller wasm, zero code change.
9. **Add edge `label` to SKILL.md schema** (F66) — the field exists in code and is used 23× in the live board but is missing from the agent's primary reference (`SKILL.md:35-42`).
10. **Drop `core:event:allow-emit`** (F97) — frontend only needs `allow-listen` (`capabilities/default.json:9-10`); least-privilege, blocks XSS-spoofed events.

---

## Deeper Bets (strategic / large — rationale & sequencing)

These are the items that change the project's trajectory rather than patch a defect. Sequence matters: the foundations (1, 2) make everything after them cheaper and safer.

1. **Shared `brainstorm-types` crate (P1.1).** The single highest-leverage refactor. It is a move, not a redesign (the types already agree), and it converts the entire class of "type drift silently breaks the JSON contract" findings (F1/F15/F75/F71) into a compile error. Do this BEFORE the validator and the NodeType enum so they live in one place. *Prerequisite for: P2.3 NodeType, P3.1 validator, P3.2 normalize_board.*

2. **Reducer/interaction layer (P1.3).** app.rs's god-component (F2) is the structural root cause of zero handler-test coverage AND the undo-ownership bug (F4/F52). One `apply(action)` with a pure `reduce(board, action) -> (board, side_effects)` makes mutation logic DOM-free testable, collapses the 9 scattered history pushes, and fixes text-edit undo "for free." *Pairs with P1.2 persistence sink and P3.2 context split.* This is the spine of T5.

3. **CI + testable core (P1.5).** There is no mechanism today that catches type drift, a broken round-trip, or a watcher regression — the project's three highest-risk seams. CI is non-negotiable for an app whose contract is "edit the file directly." Extracting `load_board_at(path)` and `should_emit_change(...)` as pure functions is what makes the round-trip and the SKIP_NEXT_EMIT logic testable at all. *Land alongside P1.1 so parity is enforced from day one.*

4. **Board validator + schema + CLI (P3.1).** The biggest *missing* agent affordance: there is no way for an agent to verify its output before writing. A `Board::validate` (reused by the CLI and the watcher reload) + a JSON Schema + `brainstorm validate`/`query` subcommands turn the write-then-pray loop into write-validate-commit. *Depends on the shared crate (P1.1) so validate logic isn't duplicated.* This is the natural evolution toward a future MCP tool surface.

5. **Canvas perf: rAF coalescer + viewport culling + spatial index (P2.1, P2.2, future P3.3).** The "infinite canvas" premise means bulk agent edits can grow the board without bound, and the current per-event (not per-frame) full repaint with O(E·N) edges hits a cliff well before truly huge boards. The rAF coalescer is the highest-ROI, lowest-risk first step (bound redraws to refresh rate); culling and a spatial index (grid buckets over the existing 50px model) follow for >1k-node boards. *Sequence rAF before HiDPI (P3.4) so the 4× pixel count doesn't compound the current cost.*

6. **CRDT (Loro) real-time collaboration (out of current scope, explicitly deferred).** Listed in CLAUDE.md as "Not Yet Implemented." This is the largest strategic bet and should NOT be attempted until T1 (data-safety), T3 (shared types), and T4 (CI) are solid — a CRDT layer on top of a non-atomic, untested, type-drift-prone persistence layer would be building on sand. Revisit only after P0/P1 land.

---

## Appendix: Refuted / low-confidence findings

**Refuted findings:** None. The verifier could not substantiate zero findings — every reviewed claim held up against the code (with the precision corrections noted inline in each finding's `note`).

**Low-confidence (UNVERIFIED med/low) findings carried forward, not in the main plan's critical path** — these were not independently re-verified against code at the same rigor as the 39 confirmed findings and are folded into P2/P3 where relevant; they should be re-confirmed before implementation:

- Architecture: F5 (fat BoardCtx), F6 (11-arg render_board), F7 (duplicated auto-size loop), F8/F32 (coord boilerplate), F9 (CI/parity), F10 (O(n) id lookups).
- Rust idioms: F16 (Camera Default), F17 (auto_size byte/char), F18 (priority range), F19 (auto-size dup), F20 (mouseleave handler), F21 (O(E·N) edges), F22 (serde unwrap), F23 (watcher .expect), F24 (get_board_path unused AppHandle), F25 (delete_asset cwd), F27 (PanState Default), F28 (Selector clone/re-parse), F29 (History must_use/can_undo), F30 (font format! churn), F31 (Edge import path), F33 (mut image shadowing).
- Performance: F39 (release profiles), F40 (md re-parse), F41 (O(E·N) edge hit-test), F42 (no spatial index), F43 (grid batching), F44 (HiDPI), F45 (shadow blur), F46 (group bounds rebuild), F47 (unbounded caches), F61 (full-board snapshots).
- Correctness: F51 (can't deselect last node via ctrl-click), F53 (mouse-down phantom undo), F54 (undo drops selection), F55 (image-delete + undo restores broken node), F56 (duplicate/self edges), F57 (filename panic), F58 (mouseleave finalize), F59 (auto_size bytes), F60 (Camera zoom=0).
- Agent-native: F65 (skip_serializing non-idempotent), F66 (SKILL.md missing edge label), F67 (priority unvalidated), F68 (filename panic), F69 (no version/deny_unknown_fields), F70 (no diff/dry-run/backup), F71 (3 hand-synced contract copies).
- Testing: F76 (auto-size in untestable Effects), F78 (no property tests), F79 (Camera zoom=0 untested), F80 (temp-dir test flakiness), F81 (no WASM harness), F82 (Cargo.lock ignore/track), F83 (undo/copy-paste/type-cycle untested).
- Security: F90 (filename panic DoS), F91 (unbounded link-preview body), F92 (paste_image unbounded decode + clipboard-text-as-path), F93 (SKIP_NEXT_EMIT racy bool), F94 (delete_asset cwd), F95 (watcher panic/exit), F96 (Cargo.lock), F97 (event:allow-emit), F98 (read_image_base64 MIME by extension).
- UX: F101 (no image export), F104 (no minimap), F105 (no select-all), F106 (no snapping/alignment), F107 (modals no Escape), F108 (poor discoverability), F110 (no multi-board), F112 (no a11y), F113 (mouseleave drag cutoff), F114 (mouse-down junk undo), F115 (undo drops selection).
