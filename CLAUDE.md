# Infinite Brainstorm

An **agent-native** infinite canvas brainstorming app. The core design principle: **anything a human can do, Claude Code can do faster by editing `board.json` directly**.

Built with Tauri v2 + Leptos (Rust WASM). File watching enables real-time sync between the UI and Claude Code edits.

## Agent-Native Design Philosophy

This app is optimized for AI collaboration:

1. **JSON is the API** - No complex IPC or REST endpoints. Claude Code reads/writes `board.json` directly.
2. **File watching = instant sync** - Edit the file, the canvas updates in <100ms.
3. **Simple schema** - Nodes have `id`, `x`, `y`, `width`, `height`, `text`, `node_type`. Edges connect nodes by ID.
4. **Predictable layouts** - Grid is 50px. Nodes default to 200x100. Easy to calculate positions programmatically.
5. **Directory-based projects** - Each folder can have its own `board.json`, like how git repos work.

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

## Common Claude Code Operations

### Read the current board state
```bash
cat ./board.json
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
│                        Frontend (Leptos/WASM)                       │
│  src/app.rs      - Main component, mouse/wheel handlers            │
│  src/canvas.rs   - HTML5 Canvas rendering (nodes, edges, grid)     │
│  src/state.rs    - Board/Node/Edge types, Camera transforms        │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ Tauri IPC (invoke/listen)
┌──────────────────────────────┴──────────────────────────────────────┐
│                        Backend (Tauri/Rust)                         │
│  src-tauri/src/lib.rs - Commands: load_board, save_board,          │
│                         fetch_link_preview, paste_image            │
│                       - File watcher (notify crate)                 │
│                       - Emits "board-changed" event on file modify  │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
┌──────────────────────────────┴──────────────────────────────────────┐
│                          Data Layer                                 │
│  Tauri mode: ./board.json (current working directory)               │
│  Browser mode: localStorage["infinite-brainstorm-board"]            │
└─────────────────────────────────────────────────────────────────────┘
```

**Key architectural decisions:**

- **File watching enables AI collaboration**: The app watches `board.json` for external changes. When Claude Code edits this file, the canvas updates immediately.
- **Skip-reload flag**: Prevents file watcher from reloading after the app's own saves (avoids feedback loops).
- **Camera transforms**: Screen coordinates ↔ world coordinates via `Camera.screen_to_world()` / `world_to_screen()`. Zoom is centered on cursor position.
- **Leptos signals**: Reactive state updates trigger canvas re-render via `Effect::new`.

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
infinite-brainstorm/
├── src/                      # Frontend (Leptos WASM)
│   ├── main.rs              # Entry point, mounts App component
│   ├── app.rs               # Main component with all interactions
│   ├── canvas.rs            # Canvas rendering functions
│   ├── history.rs           # Undo/redo history stack
│   └── state.rs             # Data types: Board, Node, Edge, Camera
├── src-tauri/               # Backend (Tauri Rust)
│   ├── src/
│   │   ├── main.rs          # Tauri entry point
│   │   └── lib.rs           # Commands + file watcher
│   ├── capabilities/        # Tauri permissions
│   │   └── default.json     # fs:allow-*, event:allow-*
│   ├── Cargo.toml           # Backend deps (tauri, notify, serde, reqwest, scraper)
│   └── tauri.conf.json      # App config, window settings
├── scripts/
│   └── brainstorm           # CLI launcher script
├── .claude/skills/infinite-brainstorm/  # Claude Code skill + board templates
│   ├── SKILL.md             # Skill instructions (schema, layouts, operations)
│   └── templates/           # 6 board templates (mind-map, kanban, flowchart, swot, pros-cons, timeline)
├── Cargo.toml               # Frontend deps (leptos, web-sys, uuid)
├── Trunk.toml               # WASM build config (ignores board.json)
└── assets/                  # Pasted images (auto-created on first paste)
```

## board.json Schema

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
      "to_node": "target-node-id"
    }
  ]
}
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
- Types duplicated in frontend and backend (no shared crate) - keep in sync manually
- Canvas rendering in `canvas.rs`, interactions in `app.rs`, types in `state.rs`

### State Management

- Leptos signals for reactive state: `let (board, set_board) = signal(...)`
- Use `get_untracked()` in event handlers to avoid re-triggering effects
- Save to disk after every user action via `invoke("save_board", ...)`

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
| Delete/Backspace | Delete selected nodes or edge |
| Cmd/Ctrl+Z | Undo last action |
| Cmd/Ctrl+Shift+Z | Redo last undone action |
| Escape | Clear selection, cancel editing |

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
- ✅ Undo/redo (Cmd+Z / Cmd+Shift+Z) - Max 100 history entries
- ✅ Local .md links as markdown - Link nodes pointing to local `.md` files render as markdown (read-only) for seamless Obsidian vault integration

**Not Yet Implemented:**
- **Multi-board** - Multiple board files, board switcher
- **CRDT (Loro)** - Real-time collaboration

**Agent-Native Features:**
- ✅ **Node metadata** - Optional `color`, `tags`, `status`, `group`, `priority` fields for agent categorization
- ✅ **Directed edges** - Arrows with arrowheads, lines clip to node borders
- ✅ **Auto-layout algorithms** - Layout math documented in skill for Claude Code (grid, tree, radial, kanban, flowchart, timeline, clustering)
- ✅ **Board templates** - 6 template JSON files in `templates/` (mind-map, kanban, flowchart, swot, pros-cons, timeline)
- **Semantic zoom** - Show node summaries when zoomed out

## Troubleshooting

### Trunk stuck on "Downloading wasm-bindgen..."

Install manually:
```bash
cargo install wasm-bindgen-cli --version 0.2.108
```

### App restarts on every interaction

Make sure `Trunk.toml` ignores `board.json`:
```toml
[watch]
ignore = ["./src-tauri", "./board.json"]
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
