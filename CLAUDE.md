# Infinite Brainstorm

An **agent-native** infinite canvas brainstorming app. The core design principle: **anything a human can do, Claude Code can do faster by editing `board.json` directly**.

Built with Tauri v2 + Leptos (Rust WASM). File watching enables real-time sync between the UI and Claude Code edits.

## Agent-Native Design Philosophy

This app is optimized for AI collaboration:

1. **JSON is the API** - No complex IPC or REST endpoints. Claude Code reads/writes `board.json` directly.
2. **File watching = instant sync** - Edit the file, the canvas updates in <100ms.
3. **Simple schema** - Nodes have `id`, `x`, `y`, `width`, `height`, `text`, `node_type`. Edges connect nodes by ID. A JSON Schema is the single source of truth: `.claude/skills/infinite-brainstorm/board.schema.json`.
4. **Predictable layouts** - Grid is 50px. Nodes default to 200x100. Easy to calculate positions programmatically.
5. **Directory-based projects** - Each folder can have its own `board.json`, like how git repos work.
6. **Validate before commit** - `brainstorm validate ./board.json` checks structure (duplicate/dangling/invalid data); `brainstorm query <expr>` inspects the board headlessly. The agent loop is write → validate → commit.

**When to use the UI vs Claude Code:**
- UI: Exploring, panning, zooming, quick manual edits
- Claude Code: Bulk operations, generating content, reorganizing, connecting ideas

## Quick Start for Claude Code

**Launch the app in current directory:**
```bash
brainstorm
# or
brainstorm /path/to/project
```

**Headless CLI subcommands (no GUI):**
```bash
brainstorm validate [path]   # validate a board.json; exits non-zero on structural errors
brainstorm query <expr>      # read-only query, prints the result to stdout
```
With no subcommand, `brainstorm` launches the desktop app. The `validate`/`query` commands let agents inspect a board without opening the window — see [CLI: validate & query](#cli-validate--query).

**File location:** `./board.json` in current working directory

**Browser mode:** Data stored in `localStorage` under key `infinite-brainstorm-board`

**Minimal node:**
```json
{"id": "uuid", "x": 0, "y": 0, "width": 200, "height": 100, "text": "Hello", "node_type": "text"}
```

**Node with metadata:**
```json
{"id": "uuid", "x": 0, "y": 0, "width": 200, "height": 100, "text": "Hello", "node_type": "idea", "color": "#ff6600", "tags": ["urgent"], "status": "todo", "group": "g1", "priority": 1}
```

**Node types:** `"text"` (gray), `"idea"` (green), `"note"` (amber), `"image"` (blue), `"md"` (purple), `"link"` (indigo)

**Edge:** `{"id": "uuid", "from_node": "node-id-1", "to_node": "node-id-2"}`

**Edge with label:** `{"id": "uuid", "from_node": "node-id-1", "to_node": "node-id-2", "label": "depends on"}`

**Auto-sized node (agent shorthand):** `{"id": "uuid", "x": 0, "y": 0, "text": "Hello", "node_type": "text"}` — width/height default to 0, app auto-sizes on load.

## Common Claude Code Operations

### Read the current board state
```bash
cat ./board.json
```

### CLI: validate & query

Two headless subcommands turn the agent loop into **write → validate → commit** — no GUI needed.

**Validate** (`brainstorm validate [path]`, defaults to `./board.json`):
```bash
brainstorm validate              # validate ./board.json
brainstorm validate other.json   # validate a specific file
```
Checks for: duplicate node ids, duplicate edge ids, dangling edges (referencing a missing node), non-finite coordinates/dimensions (NaN/inf), and out-of-range `priority` (outside 1-5). Exits **non-zero** on any structural error and prints each problem to stderr. Unknown top-level keys and a future schema `version` are forward-compat **warnings** only and never fail the command. Run this after every edit before committing.

**Query** (`brainstorm query <expr>`, `--path` optional):
```bash
brainstorm query count           # total node + edge counts
brainstorm query nodes           # list nodes
brainstorm query edges           # list edges
brainstorm query node:<id>       # one node by id
brainstorm query type:idea       # nodes of a given node_type
brainstorm query tag:urgent      # nodes carrying a tag
```

### Add multiple nodes at once
Read the file, parse JSON, append nodes with calculated positions, write back. Use grid math:
- Column layout: `x = col * 250` (200 width + 50 gap)
- Row layout: `y = row * 150` (100 height + 50 gap)

### Create a mind map from a topic
1. Create central node at (0, 0)
2. Generate related ideas
3. Position children in a radial or tree layout
4. Connect with edges

### Reorganize/cluster nodes
1. Read all nodes
2. Group by `node_type` or semantic similarity
3. Recalculate positions for each cluster
4. Write back

### Bulk rename or transform
1. Read board
2. Map over nodes, transform text
3. Write back

### Connect related ideas
1. Read board
2. Analyze node texts for relationships
3. Add edges array entries
4. Write back

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                   Shared types (crates/brainstorm-types)            │
│  Board, Node, Edge, NodeType (enum), LinkPreview, Camera,           │
│  ResizeHandle, ValidationError, geometry helpers, CURRENT_BOARD_VERSION │
│  → re-exported by BOTH crates below (type drift is a compile error) │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
┌──────────────────────────────┴──────────────────────────────────────┐
│                        Frontend (Leptos/WASM)                       │
│  src/app.rs         - Main component, mouse/wheel/keyboard handlers │
│  src/interaction.rs - DOM-free reducer: BoardAction + reduce()      │
│  src/canvas.rs      - HTML5 Canvas rendering (rAF coalescer, culling)│
│  src/state.rs       - Re-exports brainstorm-types; camera persistence│
│  src/components/     - error_banner, minimap, search_overlay, modals │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ Tauri IPC (invoke/listen)
┌──────────────────────────────┴──────────────────────────────────────┐
│                        Backend (Tauri/Rust)                         │
│  src-tauri/src/main.rs - clap CLI: `validate` / `query` / GUI       │
│  src-tauri/src/lib.rs  - Commands: load_board, save_board (atomic),  │
│                          fetch_link_preview (SSRF-hardened),        │
│                          read_image_base64 / read_markdown_file     │
│                          (scoped + size-capped), paste_image        │
│                        - File watcher (notify) w/ content-hash       │
│                          self-write suppression + deferred reloads   │
│                        - Emits "board-changed" event on file modify  │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
┌──────────────────────────────┴──────────────────────────────────────┐
│                          Data Layer                                 │
│  Tauri mode: ./board.json (atomic save via .tmp + rename, .bak copy)│
│  Browser mode: localStorage["infinite-brainstorm-board"]            │
└─────────────────────────────────────────────────────────────────────┘
```

**Key architectural decisions:**

- **Shared types crate**: `crates/brainstorm-types` owns the data model and geometry. Both the frontend (`src/state.rs`) and backend (`src-tauri/src/lib.rs`) depend on and re-export it, so the two no longer drift — a mismatch is a compile error, not a silent bug.
- **Reducer layer (`src/interaction.rs`)**: All board mutations are expressed as a `BoardAction` and applied by a pure `reduce(board, action) -> (Board, side_effects)`. The mutation logic is DOM-free and unit-tested; history is snapshotted in one place (`apply()`), so text/markdown edits and selection are captured by undo.
- **Atomic save**: Saves write `board.json.tmp`, fsync, then rename over `board.json` (never a partial write); the prior contents are copied to `board.json.bak`. On-disk format is compact JSON.
- **Non-destructive load**: A `board.json` parse error no longer blanks the board — the app keeps the current board and shows a dismissible error banner (`LoadOutcome::{Loaded, Absent, ParseError}`).
- **File watching enables AI collaboration**: The app watches `board.json` for external changes and updates the canvas immediately. Self-saves are suppressed via content-hash matching (replaces the old single-shot skip flag); external reloads are deferred while the user is mid-interaction (drag/resize/edit) so they aren't clobbered.
- **Centralized persistence**: One debounced (~220ms), dirty-tracked sink (`request_save`) replaces the ~17 scattered save calls.
- **Camera transforms**: Screen coordinates ↔ world coordinates via `Camera.screen_to_world()` / `world_to_screen()`. Zoom is centered on cursor position. Pan/zoom persist per-board to localStorage and restore on reopen.
- **Leptos signals**: Reactive state updates trigger canvas re-render, coalesced through a `requestAnimationFrame` render loop with viewport culling.

### Security

- **CSP**: `tauri.conf.json` ships a restrictive content-security policy (was `csp: null`): `default-src 'self'`; `script-src 'self'`; `img-src 'self' data: asset: https: blob:`; `connect-src 'self' ipc: https:`; `style-src 'self' 'unsafe-inline'`; `object-src 'none'`; `frame-src 'none'`.
- **Markdown sanitization**: Raw HTML in markdown nodes is escaped (pulldown-cmark `Html`/`InlineHtml` events), so a `board.json` md node cannot inject stored XSS.
- **Scoped file reads**: `read_image_base64` / `read_markdown_file` are restricted to the board directory (plus `$HOME` for the Obsidian-vault feature), with a 25MB size cap and magic-byte MIME sniffing (the file extension is not trusted). Tauri `fs`/`assetProtocol` scopes were narrowed (no more `**`/`$HOME/**` globs) and `core:event:allow-emit` was dropped.
- **SSRF-hardened link previews**: `fetch_link_preview` rejects loopback / link-local (169.254/16) / RFC1918 / CGNAT / ULA at the resolved-IP level on every redirect hop (DNS-rebinding safe), caps redirects (3) and the response body (~2MB). Auto-fetch is gated to public hosts only.

## Development Setup

### Prerequisites

```bash
# Rust toolchain
rustup target add wasm32-unknown-unknown

# Trunk for WASM bundling
cargo install trunk

# Tauri CLI
cargo install tauri-cli

# wasm-bindgen (if trunk gets stuck downloading)
cargo install wasm-bindgen-cli --version 0.2.108
```

### Running in Development

```bash
cd ~/projects/infinite-brainstorm
cargo tauri dev
```

Frontend builds via Trunk at `http://localhost:1420`, Tauri wraps it in a native window.

### Testing

```bash
cargo test                                          # workspace host tests (shared types + frontend logic)
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1   # backend (single-threaded)
```

- `crates/brainstorm-types` (89 tests) and the frontend UI crate `infinite_brainstorm_ui` (133 tests) run on the host via `cargo test` from the root — the `interaction.rs` reducer is DOM-free, so its logic is unit-tested natively without WASM.
- Backend (`src-tauri`, ~69 tests) = lib unit tests (48) + integration tests under `src-tauri/tests/` (`atomic_save`, `board_roundtrip`, `watcher`, using a `golden_board.json` fixture).
- CI (`.github/workflows/ci.yml`) runs fmt + clippy (informational), tests for both crates, and a `wasm32-unknown-unknown` build. `rust-toolchain.toml` pins the channel + wasm32 target; `Cargo.lock` is committed.

### Building for Release

```bash
cargo tauri build
```

Binary output: `target/release/infinite-brainstorm`

### Installing the CLI

After building:
```bash
# The brainstorm script is already set up at:
~/.local/bin/brainstorm

# Make sure ~/.local/bin is in your PATH
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Now use from any directory:
brainstorm
brainstorm /path/to/project
```

## Code Organization

```
infinite-brainstorm/                # Cargo workspace
├── crates/
│   └── brainstorm-types/    # Shared data model + geometry (no deps on frontend/backend)
│       └── src/lib.rs       # Board, Node, Edge, NodeType, Camera, ValidationError, ...
├── src/                      # Frontend (Leptos WASM)
│   ├── main.rs              # Entry point, mounts App component
│   ├── app.rs               # Main component with all interactions + event handlers
│   ├── interaction.rs       # DOM-free reducer: BoardAction + reduce() + apply()
│   ├── canvas.rs            # Canvas rendering (rAF coalescer, viewport culling, HiDPI)
│   ├── history.rs           # Undo/redo history (bounded VecDeque)
│   ├── state.rs             # Re-exports brainstorm-types + camera persistence
│   └── components/          # ErrorBanner, Minimap, SearchOverlay, image/markdown modals, NodeEditor
├── src-tauri/               # Backend (Tauri Rust)
│   ├── src/
│   │   ├── main.rs          # clap CLI (validate/query/GUI) + Tauri entry point
│   │   └── lib.rs           # Commands + atomic save + file watcher
│   ├── tests/               # Integration tests: atomic_save, board_roundtrip, watcher (+ golden_board.json)
│   ├── capabilities/        # Tauri permissions
│   │   └── default.json     # Narrowed fs/clipboard/event scopes
│   ├── Cargo.toml           # Backend deps (tauri, notify, serde, reqwest, scraper, clap)
│   └── tauri.conf.json      # App config, restrictive CSP, window settings
├── .github/workflows/ci.yml # fmt + clippy + tests (both crates) + wasm32 build
├── scripts/
│   └── brainstorm           # CLI launcher script
├── .claude/skills/infinite-brainstorm/  # Claude Code skill + board templates
│   ├── SKILL.md             # Skill instructions (schema, layouts, operations)
│   ├── board.schema.json    # JSON Schema — single source of truth for board.json
│   └── templates/           # 6 board templates (mind-map, kanban, flowchart, swot, pros-cons, timeline)
├── Cargo.toml               # Workspace root + frontend deps; size-optimized [profile.release]
├── Cargo.lock               # Committed (workspace builds binaries)
├── rust-toolchain.toml      # Pins channel + wasm32 target
├── Trunk.toml               # WASM build config (watch whitelist: src, crates, …)
└── assets/                  # Pasted images (auto-created on first paste, gitignored)
```

## board.json Schema

The canonical machine-readable schema lives at `.claude/skills/infinite-brainstorm/board.schema.json` (JSON Schema draft-07). Validate any board with `brainstorm validate ./board.json`.

```json
{
  "nodes": [
    {
      "id": "unique-uuid",
      "x": 100.0,
      "y": 200.0,
      "width": 200.0,
      "height": 100.0,
      "text": "Node content",
      "node_type": "text"
    },
    {
      "id": "image-uuid",
      "x": 350.0,
      "y": 200.0,
      "width": 200.0,
      "height": 150.0,
      "text": "/Users/me/photos/diagram.png",
      "node_type": "image"
    },
    {
      "id": "md-uuid",
      "x": 600.0,
      "y": 200.0,
      "width": 300.0,
      "height": 200.0,
      "text": "# Title\n\n- Item 1\n- Item 2\n\n**Bold** and *italic*",
      "node_type": "md"
    },
    {
      "id": "link-uuid",
      "x": 950.0,
      "y": 200.0,
      "width": 280.0,
      "height": 200.0,
      "text": "https://github.com/anthropics/claude-code",
      "node_type": "link"
    },
    {
      "id": "metadata-example-uuid",
      "x": 100.0,
      "y": 400.0,
      "width": 200.0,
      "height": 100.0,
      "text": "A categorized node",
      "node_type": "idea",
      "color": "#ff6600",
      "tags": ["urgent", "pricing"],
      "status": "in-progress",
      "group": "cluster-a",
      "priority": 2
    }
  ],
  "edges": [
    {
      "id": "edge-uuid",
      "from_node": "source-node-id",
      "to_node": "target-node-id",
      "label": "depends on"
    }
  ]
}
```

### Board version

`Board` carries an optional `version` field (defaults to `CURRENT_BOARD_VERSION = 1`) for future migrations. A board with no `version` key is treated as the current version — old files keep loading unchanged and re-serialize without gaining a `version` key. A board declaring a version newer than this build still loads, surfacing a non-fatal forward-compat warning.

### Node types (enum, forward-compatible)

`node_type` is a Rust enum serialized lowercase (`"text"`, `"idea"`, `"note"`, `"image"`, `"md"`, `"link"`). An unrecognized value deserializes to a neutral `Unknown` fallback (`#[serde(other)]`) rather than failing — agents can write a future node type and the board round-trips without dropping the node. `status` remains a freeform string.

### Validation

`Board::validate()` returns every structural problem found (empty == clean): duplicate node/edge ids, dangling edges (an endpoint references a missing node), non-finite coordinates/dimensions, out-of-range `priority` (outside 1-5), and a future schema `version` (warning only). It's pure and reused everywhere — the `brainstorm validate` CLI, and the file-watcher reload path (which logs warnings and drops dangling edges non-destructively, leaving the on-disk file untouched until the next save).

### Node Auto-size

`width` and `height` default to `0` when omitted from JSON. On load, the app auto-sizes any node with `width == 0 || height == 0` based on text content. The computed dimensions persist on next save. This means agents can create nodes without specifying dimensions:

```json
{"id": "uuid", "x": 0, "y": 0, "text": "Just the text", "node_type": "idea"}
```

### Edge Labels (optional)

Edges support an optional `label` field rendered at the midpoint of the edge line. Useful for expressing relationship types (e.g., "depends on", "blocks", "related to"):

```json
{"id": "uuid", "from_node": "n1", "to_node": "n2", "label": "blocks"}
```

### Group Containers

Nodes sharing the same `group` value are visually enclosed in a translucent bounding box with the group name as a label. No extra schema — just set the existing `group` field on nodes:

```json
{"id": "n1", "x": 0, "y": 0, "width": 200, "height": 100, "text": "Task A", "node_type": "text", "group": "sprint-1"}
```

### Node Metadata (optional)

All metadata fields are optional and backward-compatible. Existing `board.json` files without these fields work unchanged. Fields are omitted from JSON when empty/None.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `color` | `string?` | null | Custom border color override (hex, e.g. `"#ff6600"`) |
| `tags` | `string[]` | `[]` | Freeform tags for categorization |
| `status` | `string?` | null | Workflow status (e.g. `"todo"`, `"in-progress"`, `"done"`) |
| `group` | `string?` | null | Group ID for clustering related nodes |
| `priority` | `number?` | null | Priority level (1-5) |

**Visual rendering:**
- `color` overrides the node border color (both selected and unselected states)
- `tags` render as comma-separated text at the bottom-left of the node
- `status` renders as a small badge at the top-right corner
- `priority` renders as `P1`-`P5` next to the type indicator

**Agent usage examples:**
```bash
# Add tags to categorize nodes
jq '.nodes[] | select(.text | contains("pricing")) | .tags = ["pricing", "v2"]' board.json

# Set workflow status
jq '.nodes[] | select(.id == "node-id") | .status = "done"' board.json

# Color-code by group
jq '.nodes[] | select(.group == "cluster-a") | .color = "#ff6600"' board.json
```

**Node types and colors:**
- `"text"` → Dark green background (`#040804`) - default
- `"idea"` → Slightly brighter green (`#041004`)
- `"note"` → Amber-green (`#0a0a04`)
- `"image"` → Dark blue (`#040408`) - displays image thumbnail, double-click opens 90% modal
- `"md"` → Dark purple (`#080408`) - renders markdown content
- `"link"` → Dark indigo (`#040410`) - displays URL preview card with og:image, title, description

**Image node:** Set `text` field to image path (local) or URL. Local paths are auto-converted to `asset://localhost/` URLs.

**Markdown node:** Set `text` field to markdown content. Rendered HTML displays in the node.

**Link node:** Set `text` field to a URL. For HTTP/HTTPS URLs, fetches Open Graph metadata and displays preview image. Click copies URL to clipboard, double-click opens in browser.

**Local .md links:** Link nodes with paths to local `.md` files render as read-only markdown:
- Absolute path: `/Users/me/vault/note.md`
- file:// URL: `file:///Users/me/vault/note.md`
- Home-relative: `~/Documents/note.md`

## Conventions

### Rust

- Standard Rust naming (snake_case functions, PascalCase types)
- Data types live in the shared `crates/brainstorm-types` crate, re-exported by both `src/state.rs` (frontend) and `src-tauri/src/lib.rs` (backend) — no manual sync, type drift is a compile error
- Canvas rendering in `canvas.rs`, interactions in `app.rs`, pure mutation logic in `interaction.rs`, types in `crates/brainstorm-types`

### State Management

- Leptos signals for reactive state: `let (board, set_board) = signal(...)`
- Use `get_untracked()` in event handlers to avoid re-triggering effects
- Mutations go through `interaction::reduce` via `apply()`, which snapshots undo history once then routes persistence through the centralized debounced `request_save` sink (no scattered `save_board` calls)

### Camera/Coordinate System

- World coordinates: Where nodes actually are (infinite canvas space)
- Screen coordinates: Pixel position on the canvas element
- `Camera.x/y`: Top-left corner of visible world area
- `Camera.zoom`: Scale factor (0.1 to 5.0)

## User Interactions

| Action | Behavior |
|--------|----------|
| Click node | Select node (green border) |
| Click link node | Select + copy URL to clipboard |
| Click edge | Select edge (glowing line) |
| Ctrl/Cmd+click | Toggle node in multi-selection |
| Drag node | Move all selected nodes, saves on release |
| Drag corner handle | Resize selected node (min 50x30) |
| Drag canvas | Pan the view |
| Ctrl/Cmd+drag canvas | Box select nodes |
| Scroll wheel | Zoom (centered on cursor) |
| Double-click empty | Create new node, enter edit mode |
| Double-click node | Edit node text inline |
| Double-click image | Open image in 90% viewport modal |
| Double-click md | Open markdown editor modal |
| Double-click link | Open URL in browser (or view-only modal for local .md files) |
| Shift+drag from node | Create edge to target node |
| Cmd/Ctrl+C | Copy selected nodes (and edges between them) |
| Cmd/Ctrl+V | Paste copied nodes at cursor (or paste image from clipboard) |
| T | Cycle type on selected nodes (text→idea→note→image→md→link) |
| Cmd/Ctrl+A | Select all nodes |
| Cmd/Ctrl+F | Open search overlay (filter by text/tags/status; Enter recenters first match) |
| F | Fit all nodes to view |
| Cmd/Ctrl+0 | Reset zoom to 1.0 (keeps viewport center) |
| Delete/Backspace | Delete selected nodes or edge |
| Cmd/Ctrl+Z | Undo last action |
| Cmd/Ctrl+Shift+Z | Redo last undone action |
| Escape | Clear selection, cancel editing, close active modal |

Drags use pointer-capture / document listeners so they continue off-canvas (no cut-off at the window edge). On drag release, node positions snap to the 50px grid. A minimap (bottom-right) gives an overview and recenters the camera on click. An "Export PNG" affordance saves the current viewport via `canvas.to_data_url`.

## Future Ideas

**Implemented:**
- ✅ Text editing (double-click to edit inline)
- ✅ Edge creation (shift+drag)
- ✅ Multi-select (ctrl+click, box select)
- ✅ Edge deletion (click edge to select, delete key)
- ✅ Image nodes (thumbnail + modal preview)
- ✅ Markdown nodes (rendered HTML + edit modal)
- ✅ Link nodes (OG preview card, click to copy, double-click to open)
- ✅ Directory-based projects (board.json per folder)
- ✅ CLI launcher (`brainstorm` command)
- ✅ Dual storage (Tauri filesystem + browser localStorage)
- ✅ Node resizing (drag corner handles, min 50x30)
- ✅ Image paste (Cmd+V pastes clipboard image to ./assets/ folder)
- ✅ Undo/redo (Cmd+Z / Cmd+Shift+Z) - Bounded history; text/markdown edits and selection captured
- ✅ Local .md links as markdown - Link nodes pointing to local `.md` files render as markdown (read-only) for seamless Obsidian vault integration
- ✅ Search (Cmd+F overlay, filter by text/tags/status, Enter recenters first match)
- ✅ Fit-to-view (F), reset zoom (Cmd+0), select-all (Cmd+A)
- ✅ Minimap (bottom-right overview, click-to-recenter)
- ✅ PNG export (current viewport via `canvas.to_data_url`)
- ✅ Snap-to-grid on drag release (50px); off-canvas drags via pointer-capture
- ✅ Camera pan/zoom persists per-board to localStorage and restores on reopen

**Not Yet Implemented:**
- **Multi-board** - Multiple board files, board switcher
- **CRDT (Loro)** - Real-time collaboration
- **Semantic zoom** - Show node summaries when zoomed out

**Agent-Native Features:**
- ✅ **Node metadata** - Optional `color`, `tags`, `status`, `group`, `priority` fields for agent categorization
- ✅ **Directed edges** - Arrows with arrowheads, lines clip to node borders
- ✅ **Edge labels** - Optional `label` field on edges, rendered at midpoint with background pill
- ✅ **Group containers** - Nodes with same `group` value get a visual bounding box
- ✅ **Node auto-size** - Agents can omit `width`/`height`; app auto-sizes on load based on text content
- ✅ **Auto-layout algorithms** - Layout math documented in skill for Claude Code (grid, tree, radial, kanban, flowchart, timeline, clustering)
- ✅ **Board templates** - 6 template JSON files in `templates/` (mind-map, kanban, flowchart, swot, pros-cons, timeline)
- ✅ **CLI validate/query** - `brainstorm validate` (structural checks, non-zero exit) and `brainstorm query` (count/nodes/edges/node:/type:/tag:) for headless agent loops
- ✅ **JSON Schema** - `board.schema.json` is the single source of truth for the board format
- ✅ **Atomic save + non-destructive load** - No partial writes; a parse error preserves the board and shows a banner instead of blanking it

## Troubleshooting

### Trunk stuck on "Downloading wasm-bindgen..."

Install manually:
```bash
cargo install wasm-bindgen-cli --version 0.2.108
```

### App restarts on every interaction

Fixed: `Trunk.toml` now uses a `watch` **whitelist** of source paths instead of watching the whole project root. Previously, Trunk's default rebuilt and force-reloaded the webview whenever the app wrote runtime data (`board.json`, its atomic-save siblings `.tmp`/`.bak`, or a pasted image in `./assets`), remounting the app and resetting the viewport. The whitelist scopes the dev watcher to source only:
```toml
[watch]
watch = ["src", "crates", "index.html", "styles.css", "public", "Cargo.toml"]
```

### File watcher not detecting external changes

- The watcher has a 500ms poll interval + 100ms debounce delay
- Ensure the parent directory exists
- Check for "Failed to watch" errors in console

### brainstorm command not found

```bash
# Ensure ~/.local/bin is in PATH
echo $PATH | grep -q '.local/bin' || echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```
