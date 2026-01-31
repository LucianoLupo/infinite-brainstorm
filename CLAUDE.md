# Infinite Brainstorm

An infinite canvas brainstorming app built with Tauri v2 + Leptos (Rust WASM). Designed for AI-assisted collaboration where Claude Code can directly edit the canvas state via JSON.

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
│  src-tauri/src/lib.rs - Commands: load_board, save_board           │
│                       - File watcher (notify crate)                 │
│                       - Emits "board-changed" event on file modify  │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
┌──────────────────────────────┴──────────────────────────────────────┐
│                          Data Layer                                 │
│  ~/Library/Application Support/com.lucianolupo.infinite-brainstorm/ │
│  └── board.json  ← Claude Code can edit this directly               │
└─────────────────────────────────────────────────────────────────────┘
```

**Key architectural decisions:**

- **File watching enables AI collaboration**: The app watches `board.json` for external changes. When Claude Code edits this file, the canvas updates immediately.
- **Camera transforms**: Screen coordinates ↔ world coordinates via `Camera.screen_to_world()` / `world_to_screen()`. Zoom is centered on cursor position.
- **Leptos signals**: Reactive state updates trigger canvas re-render via `Effect::new`.

## Getting Started

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

### Running

```bash
cd ~/projects/infinite-brainstorm
cargo tauri dev
```

Frontend builds via Trunk at `http://localhost:1420`, Tauri wraps it in a native window.

## Code Organization

```
infinite-brainstorm/
├── src/                      # Frontend (Leptos WASM)
│   ├── main.rs              # Entry point, mounts App component
│   ├── app.rs               # Main component with all interactions
│   ├── canvas.rs            # Canvas rendering functions
│   └── state.rs             # Data types: Board, Node, Edge, Camera
├── src-tauri/               # Backend (Tauri Rust)
│   ├── src/
│   │   ├── main.rs          # Tauri entry point
│   │   └── lib.rs           # Commands + file watcher
│   ├── capabilities/        # Tauri permissions
│   │   └── default.json     # fs:allow-*, event:allow-*
│   ├── Cargo.toml           # Backend deps (tauri, notify, serde)
│   └── tauri.conf.json      # App config, window settings
├── public/
│   └── board.json           # Template (not used at runtime)
├── Cargo.toml               # Frontend deps (leptos, web-sys, uuid)
└── Trunk.toml               # WASM build config
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
      "node_type": "text"  // "text" | "idea" | "note"
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

**Node types and colors:**
- `"idea"` → Purple background (`#4a4a8a`)
- `"note"` → Red background (`#8a4a4a`)
- `"text"` → Gray background (`#3a3a5a`) - default

**File location:** `~/Library/Application Support/com.lucianolupo.infinite-brainstorm/board.json`

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
| Drag node | Move node, saves on release |
| Drag canvas | Pan the view |
| Scroll wheel | Zoom (centered on cursor) |
| Double-click | Create new node at cursor |

## Common Tasks

### Add a node via Claude Code

Edit `~/Library/Application Support/com.lucianolupo.infinite-brainstorm/board.json`:

```json
{
  "nodes": [
    // ... existing nodes ...
    {
      "id": "my-new-node",
      "x": 500.0,
      "y": 300.0,
      "width": 180.0,
      "height": 80.0,
      "text": "Added by Claude",
      "node_type": "idea"
    }
  ],
  "edges": []
}
```

The app will detect the change and re-render immediately.

### Add an edge (connection)

Add to the `edges` array with valid node IDs:

```json
{
  "id": "edge-1",
  "from_node": "source-node-id",
  "to_node": "target-node-id"
}
```

### Run in release mode

```bash
cargo tauri build
```

Output in `target/release/bundle/`.

## Future Ideas (Not Implemented)

- **CRDT (Loro)**: Real-time collaboration between multiple users
- **Text editing**: In-node text editing instead of fixed strings
- **Multi-board**: Multiple board files, board switching
- **Undo/redo**: History stack for actions

## Troubleshooting

### Trunk stuck on "Downloading wasm-bindgen..."

Install manually:
```bash
cargo install wasm-bindgen-cli --version 0.2.108
```

### board.json not found

The app auto-creates it at first run in `~/Library/Application Support/com.lucianolupo.infinite-brainstorm/`.

### File watcher not detecting changes

- Check if the file watcher thread started (look for "Failed to watch" in console)
- The watcher has a 500ms poll interval + 100ms debounce delay
- Ensure the parent directory exists
