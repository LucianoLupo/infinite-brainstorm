<p align="center">
  <h1 align="center">Infinite Brainstorm</h1>
  <p align="center">
    <strong>An agent-native infinite canvas for human-AI collaboration</strong>
  </p>
  <p align="center">
    <a href="#features">Features</a> •
    <a href="#quick-start">Quick Start</a> •
    <a href="#claude-code-skill">Claude Code Skill</a> •
    <a href="#usage">Usage</a> •
    <a href="#contributing">Contributing</a>
  </p>
  <p align="center">
    <img src="https://img.shields.io/badge/Tauri-v2-blue?style=flat-square" alt="Tauri v2">
    <img src="https://img.shields.io/badge/Leptos-0.8-green?style=flat-square" alt="Leptos 0.8">
    <img src="https://img.shields.io/badge/Rust-WASM-orange?style=flat-square" alt="Rust WASM">
    <img src="https://img.shields.io/badge/License-MIT-yellow?style=flat-square" alt="MIT License">
  </p>
</p>

---

## Why Infinite Brainstorm?

Most productivity tools treat AI as an afterthought. **Infinite Brainstorm is different.**

It's built from the ground up so that anything a human can do on the canvas, an AI assistant can do by editing a JSON file:

```
You: "Create a mind map about machine learning"
Claude Code: *edits board.json*
Canvas: *updates instantly*

You drag nodes around. AI generates content.
You organize visually. AI does bulk operations.
Everything stays in sync. Automatically.
```

**The secret?** A simple JSON file (`board.json`) that both humans and AI can read and write. No APIs. No SDKs. Just a file.

## Features

- **Infinite Canvas** — Pan and zoom without limits
- **6 Node Types** — Text, ideas, notes, images, markdown, link previews
- **Directed Graph** — Edges render as arrows with arrowheads clipped to node borders
- **Node Metadata** — Color, tags, status, group, and priority fields for categorization
- **Real-Time Sync** — External file changes appear instantly (<100ms)
- **Agent-Native** — AI assistants edit `board.json` directly, with a bundled [Claude Code skill](#claude-code-skill)
- **Undo/Redo** — Full history stack (Cmd+Z / Cmd+Shift+Z)
- **Image Paste** — Cmd+V pastes clipboard images into `./assets/`
- **Node Resizing** — Drag corner handles (min 50x30)
- **Link Previews** — Open Graph metadata fetching for URL nodes
- **Obsidian Integration** — Link nodes pointing to local `.md` files render as markdown
- **Dual Storage** — Desktop app uses filesystem, browser uses localStorage
- **Directory-Based** — Each project folder gets its own board
- **Board Templates** — 6 ready-to-use layouts (mind map, kanban, flowchart, SWOT, pros/cons, timeline)

## Quick Start

### Run from Source

```bash
# Prerequisites
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli

# Clone and run
git clone https://github.com/LucianoLupo/infinite-brainstorm.git
cd infinite-brainstorm
cargo tauri dev
```

### Build and Install

```bash
# Build release binary
cargo tauri build

# Install the CLI launcher
mkdir -p ~/.local/bin
ln -sf "$(pwd)/scripts/brainstorm" ~/.local/bin/brainstorm

# Add to PATH (if not already)
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Run from any directory
brainstorm                    # Current directory
brainstorm ~/projects/ideas   # Specific directory
```

<details>
<summary><strong>Troubleshooting: Trunk stuck downloading wasm-bindgen</strong></summary>

Install manually:
```bash
cargo install wasm-bindgen-cli --version 0.2.108
```
</details>

## Claude Code Skill

The repo includes a [Claude Code skill](https://code.claude.com/docs/en/skills) at `.claude/skills/infinite-brainstorm/` with:

- **Full board.json schema** — node types, metadata fields, edge format
- **7 layout algorithms** — grid, tree, radial, kanban, flowchart, timeline, clustering
- **6 board templates** — ready-to-use JSON files for common layouts
- **Common operations** — step-by-step instructions for reading, creating, reorganizing boards

### Install the skill for global access

The skill auto-loads when you use Claude Code inside this repo. To use it from **any directory**, symlink it:

```bash
ln -sf "$(pwd)/.claude/skills/infinite-brainstorm" ~/.claude/skills/infinite-brainstorm
```

Now Claude Code can brainstorm with you anywhere — just say "create a mind map about X" or "set up a kanban board".

### Templates

| Template | Use Case |
|----------|----------|
| `mind-map.json` | Central topic with radial branches and sub-ideas |
| `kanban.json` | Status columns (To Do, In Progress, Review, Done) |
| `flowchart.json` | Sequential steps with decision branches |
| `swot.json` | Strengths, Weaknesses, Opportunities, Threats |
| `pros-cons.json` | Two-column decision analysis |
| `timeline.json` | Horizontal phases with milestones |

Templates live in `.claude/skills/infinite-brainstorm/templates/`. Claude Code reads them automatically when creating boards.

## Usage

### Controls

| Action | What it does |
|--------|--------------|
| **Double-click** empty space | Create new node |
| **Double-click** node | Edit text (or open modal for image/md/link) |
| **Click** node | Select it |
| **Cmd/Ctrl + click** | Add/remove from selection |
| **Drag** node | Move all selected nodes |
| **Drag** corner handle | Resize node (min 50x30) |
| **Drag** empty space | Pan the canvas |
| **Cmd/Ctrl + drag** | Box select multiple nodes |
| **Shift + drag** from node | Create directed edge to target |
| **Scroll wheel** | Zoom (centered on cursor) |
| **Cmd/Ctrl + V** | Paste clipboard image at cursor |
| **T** | Cycle node type on selected nodes |
| **Delete / Backspace** | Delete selected nodes or edge |
| **Cmd/Ctrl + Z** | Undo |
| **Cmd/Ctrl + Shift + Z** | Redo |
| **Escape** | Clear selection, cancel editing |

### Node Types

| Type | Color | Use Case |
|------|-------|----------|
| `text` | Dark green | Default, simple text |
| `idea` | Bright green | Highlighted concepts |
| `note` | Amber | Annotations, comments |
| `image` | Blue | Embedded images (local path or URL) |
| `md` | Purple | Rendered markdown content |
| `link` | Indigo | URL with OG preview card, or local `.md` path rendered as markdown |

### Data Format

All data lives in `board.json`:

```json
{
  "nodes": [
    {
      "id": "unique-id",
      "x": 0.0,
      "y": 0.0,
      "width": 200.0,
      "height": 100.0,
      "text": "Your content here",
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
      "id": "edge-id",
      "from_node": "source-node-id",
      "to_node": "target-node-id"
    }
  ]
}
```

**Edges are directed** — rendered as arrows from `from_node` to `to_node` with arrowheads at the target.

**Node metadata** (all optional):

| Field | Type | Description |
|-------|------|-------------|
| `color` | `string` | Custom border color (hex, e.g. `"#ff6600"`) |
| `tags` | `string[]` | Freeform tags for categorization |
| `status` | `string` | Workflow status (e.g. `"todo"`, `"in-progress"`, `"done"`) |
| `group` | `string` | Group ID for clustering related nodes |
| `priority` | `number` | Priority level 1-5 (renders as P1-P5) |

### Working with AI Assistants

The app watches `board.json` for external changes. Any AI assistant can:

1. **Read** the board: `cat board.json`
2. **Add nodes** with calculated positions
3. **Create directed edges** between related ideas
4. **Categorize** with metadata (tags, status, priority, color)
5. **Reorganize layouts** programmatically
6. **Apply templates** for common board structures

Changes sync to the canvas in under 100ms.

See [`CLAUDE.md`](./CLAUDE.md) for detailed AI integration docs, or install the [Claude Code skill](#claude-code-skill) for the best experience.

## Architecture

```
infinite-brainstorm/
├── src/                          # Frontend (Leptos WASM)
│   ├── main.rs                  # Entry point
│   ├── app.rs                   # Main component, event handlers, state
│   ├── canvas.rs                # HTML5 Canvas rendering, arrowheads, clipping
│   ├── state.rs                 # Data types (Board, Node, Edge, Camera)
│   ├── history.rs               # Undo/redo history stack
│   └── components/              # Extracted UI components
│       ├── image_modal.rs       # Full-screen image preview
│       ├── markdown_modal.rs    # Markdown editor modal
│       ├── markdown_overlays.rs # Markdown rendering in nodes
│       └── node_editor.rs       # Inline text editor
│
├── src-tauri/                   # Backend (Tauri v2)
│   └── src/lib.rs               # IPC commands, file watcher
│
├── .claude/skills/              # Claude Code skill
│   └── infinite-brainstorm/
│       ├── SKILL.md             # Schema, layouts, operations
│       └── templates/           # 6 board templates
│
├── scripts/brainstorm           # CLI launcher
├── board.json                   # Your data (gitignored)
├── CLAUDE.md                    # AI assistant reference
└── README.md
```

### Key Design Decisions

| Decision | Why |
|----------|-----|
| **JSON file as API** | AI assistants edit it directly. No complex integrations needed. |
| **File watching** | External changes sync instantly. Enables real-time AI collaboration. |
| **Directed edges** | Arrows with arrowheads represent flows, dependencies, hierarchies. |
| **Node metadata** | Optional fields (color, tags, status, group, priority) enable agent-driven categorization without schema changes. |
| **Current directory** | Each project folder gets its own board, like git repos. |
| **Dual storage** | Tauri uses filesystem, browser uses localStorage. Same code path. |
| **Skill + templates** | Claude Code skill bundles schema docs and layout templates for any-directory access. |

## Contributing

Contributions are welcome!

### Development Setup

1. **Fork and clone** the repository
2. **Install prerequisites**: Rust, WASM target, Trunk, Tauri CLI
3. **Run in dev mode**: `cargo tauri dev`
4. **Check both crates**: `cargo check` (frontend) and `cd src-tauri && cargo check` (backend)

### Code Structure

| File | What to modify |
|------|----------------|
| `src/app.rs` | Event handlers, interactions, UI logic |
| `src/canvas.rs` | Canvas rendering, visual appearance |
| `src/state.rs` | Data types, add new node properties |
| `src/history.rs` | Undo/redo behavior |
| `src-tauri/src/lib.rs` | Backend commands, file watcher |

**Note:** Types are duplicated between `src/state.rs` (frontend) and `src-tauri/src/lib.rs` (backend). Keep them in sync when modifying.

### Guidelines

- **Keep it simple** — The codebase is intentionally minimal (~4,200 LOC)
- **Test with AI** — Make sure Claude Code can still edit `board.json` after your changes
- **Update docs** — If you add features, update `CLAUDE.md`, the skill, and this README
- **Both crates must compile** — `cargo check` from root (frontend) and `src-tauri/` (backend)

### Ideas for Contributions

- [ ] **Export** — PNG, SVG, or PDF export of the canvas
- [ ] **Search/filter** — Find nodes by text, tags, status
- [ ] **Minimap** — Small overview for navigation
- [ ] **Edge labels** — Text on edges to describe relationships
- [ ] **Keyboard navigation** — Arrow keys to traverse connected nodes
- [ ] **Group backgrounds** — Visual rectangles around nodes sharing a `group`
- [ ] **Themes** — Light mode, custom color schemes
- [ ] **Touch support** — Mobile/tablet gestures
- [ ] **Multi-board** — Multiple board files per directory, board switcher
- [ ] **Real-time collaboration** — CRDT-based multi-user editing

## Tech Stack

| Component | Technology |
|-----------|------------|
| Desktop framework | [Tauri v2](https://tauri.app) |
| Frontend framework | [Leptos 0.8](https://leptos.dev) |
| Rendering | HTML5 Canvas |
| Language | Rust (compiled to WASM) |
| File watching | [notify](https://docs.rs/notify) |
| Link previews | [scraper](https://docs.rs/scraper) + [reqwest](https://docs.rs/reqwest) |

## License

MIT License — see [LICENSE](./LICENSE) for details.

---

<p align="center">
  <strong>Built for humans and AI, working together.</strong>
</p>

<p align="center">
  <a href="https://github.com/LucianoLupo/infinite-brainstorm/issues">Report Bug</a> •
  <a href="https://github.com/LucianoLupo/infinite-brainstorm/issues">Request Feature</a>
</p>
