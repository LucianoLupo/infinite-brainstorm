<p align="center">
  <h1 align="center">Infinite Brainstorm</h1>
  <p align="center">
    <strong>An agent-native infinite canvas for human-AI collaboration</strong>
  </p>
  <p align="center">
    <a href="#features">Features</a> •
    <a href="#quick-start">Quick Start</a> •
    <a href="#installation">Installation</a> •
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

Most productivity tools treat AI as an afterthought—chatbots bolted onto existing interfaces. **Infinite Brainstorm is different.**

It's built from the ground up for human-AI collaboration:

```
┌─────────────────────────────────────────────────────────────┐
│  You: "Create a mind map about machine learning"            │
│  Claude Code: *edits board.json*                            │
│  Canvas: *updates instantly*                                │
│                                                             │
│  You drag nodes around. AI generates content.               │
│  You organize visually. AI does bulk operations.            │
│  Everything stays in sync. Automatically.                   │
└─────────────────────────────────────────────────────────────┘
```

**The secret?** A dead-simple JSON file (`board.json`) that both humans and AI can read and write. No APIs. No SDKs. Just a file.

## Features

- **🎨 Infinite Canvas** — Pan and zoom without limits
- **📝 Rich Node Types** — Text, ideas, notes, images, markdown, link previews
- **⚡ Real-Time Sync** — External file changes appear instantly (<100ms)
- **🤖 Agent-Native** — AI assistants edit `board.json` directly
- **💾 Dual Storage** — Desktop (file) or browser (localStorage)
- **🔗 Link Previews** — Automatic Open Graph metadata fetching
- **📁 Directory-Based** — Each project folder gets its own board

## Quick Start

### Run from Source (Development)

```bash
# Prerequisites
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli

# Clone and run
git clone https://github.com/LucianoLupo/infinite-brainstorm.git
cd infinite-brainstorm
cargo tauri dev
```

### Use the CLI (After Building)

```bash
# Build once
cargo tauri build

# Run in any directory
brainstorm                    # Current directory
brainstorm ~/projects/ideas   # Specific directory
```

## Installation

### Prerequisites

| Tool | Purpose | Install |
|------|---------|---------|
| **Rust** | Language & compiler | [rustup.rs](https://rustup.rs) |
| **WASM target** | Frontend compilation | `rustup target add wasm32-unknown-unknown` |
| **Trunk** | WASM bundler | `cargo install trunk` |
| **Tauri CLI** | App builder | `cargo install tauri-cli` |

<details>
<summary><strong>Troubleshooting: Trunk stuck downloading wasm-bindgen</strong></summary>

Install manually:
```bash
cargo install wasm-bindgen-cli --version 0.2.108
```
</details>

### Development Mode

```bash
cd infinite-brainstorm
cargo tauri dev
```

Opens at `http://localhost:1420` with hot reload.

### Production Build

```bash
cargo tauri build
```

**Output:**
- Binary: `target/release/infinite-brainstorm`
- macOS App: `target/release/bundle/macos/infinite-brainstorm.app`

### Install CLI Command

```bash
# Link the launcher script
mkdir -p ~/.local/bin
ln -sf "$(pwd)/scripts/brainstorm" ~/.local/bin/brainstorm

# Add to PATH (if needed)
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Verify
brainstorm --help
```

## Usage

### Controls

| Action | What it does |
|--------|--------------|
| **Click** node | Select it |
| **Double-click** empty space | Create new node |
| **Double-click** node | Edit text |
| **Double-click** image node | Full-screen preview |
| **Double-click** link node | Open URL in browser |
| **Drag** node | Move (all selected move together) |
| **Drag** empty space | Pan the canvas |
| **Scroll wheel** | Zoom in/out (centered on cursor) |
| **Shift + drag** from node | Create connection to another node |
| **Cmd/Ctrl + drag** | Box select multiple nodes |
| **Cmd/Ctrl + click** | Add/remove from selection |
| **T** | Cycle node type (text → idea → note → image → md → link) |
| **Delete / Backspace** | Delete selected nodes or edge |
| **Escape** | Clear selection, cancel editing |

### Node Types

| Type | Color | Use Case |
|------|-------|----------|
| `text` | Gray | Default, simple text |
| `idea` | Green | Highlighted concepts |
| `note` | Amber | Annotations, comments |
| `image` | Blue | Embedded images (local path or URL) |
| `md` | Purple | Rendered markdown content |
| `link` | Indigo | URL with preview card |

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
      "node_type": "idea"
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

### Working with AI Assistants

The app watches `board.json` for external changes. AI assistants can:

1. **Read** the board: `cat board.json`
2. **Add nodes** with calculated positions
3. **Create connections** between related ideas
4. **Bulk transform** content
5. **Reorganize layouts** programmatically

Changes sync to the canvas in under 100ms.

See [`CLAUDE.md`](./CLAUDE.md) for detailed AI assistant instructions.

## Architecture

```
infinite-brainstorm/
├── src/                      # Frontend (Leptos WASM)
│   ├── main.rs              # Entry point
│   ├── app.rs               # Main component, event handlers
│   ├── canvas.rs            # HTML5 Canvas rendering
│   └── state.rs             # Data types (Board, Node, Edge, Camera)
│
├── src-tauri/               # Backend (Tauri v2)
│   ├── src/
│   │   ├── main.rs          # Tauri entry
│   │   └── lib.rs           # Commands, file watcher
│   ├── capabilities/        # Permission config
│   └── tauri.conf.json      # App config
│
├── scripts/
│   └── brainstorm           # CLI launcher
│
├── board.json               # Your data (gitignored)
├── Cargo.toml               # Frontend dependencies
├── Trunk.toml               # WASM build config
├── CLAUDE.md                # AI assistant instructions
└── README.md                # You are here
```

### Key Design Decisions

| Decision | Why |
|----------|-----|
| **JSON file as API** | AI assistants can edit it directly. No complex integrations. |
| **File watching** | External changes sync instantly. Enables real-time collaboration. |
| **Current directory** | Each project folder can have its own board. Like git repos. |
| **Dual storage** | Tauri uses filesystem, browser uses localStorage. Same code. |
| **Skip-reload flag** | Prevents feedback loop when app saves trigger file watcher. |

## Contributing

Contributions are welcome! Here's how to get started:

### Development Setup

1. **Fork and clone** the repository
2. **Install prerequisites** (see [Installation](#installation))
3. **Run in dev mode**: `cargo tauri dev`
4. **Make changes** — hot reload will pick them up

### Code Structure

| File | What to modify |
|------|----------------|
| `src/app.rs` | Event handlers, interactions, UI logic |
| `src/canvas.rs` | Canvas rendering, visual appearance |
| `src/state.rs` | Data types, add new node properties |
| `src-tauri/src/lib.rs` | Backend commands, file watcher |

### Guidelines

- **Keep it simple** — The codebase is intentionally minimal
- **Test with AI** — Make sure Claude Code can still edit `board.json`
- **Update docs** — If you add features, update `CLAUDE.md` and this README

### Submitting Changes

1. Create a feature branch: `git checkout -b feature/amazing-feature`
2. Make your changes
3. Test: `cargo tauri dev`
4. Commit: `git commit -m "Add amazing feature"`
5. Push: `git push origin feature/amazing-feature`
6. Open a Pull Request

### Ideas for Contributions

- [ ] **Undo/Redo** — History stack for Ctrl+Z/Y
- [ ] **Keyboard shortcuts** — More hotkeys for power users
- [ ] **Export** — PNG, SVG, or PDF export
- [ ] **Themes** — Light mode, custom colors
- [ ] **Search** — Find nodes by text
- [ ] **Templates** — Pre-built layouts (mind map, kanban, flowchart)
- [ ] **Touch support** — Mobile/tablet gestures

## Roadmap

### Now
- ✅ Core infinite canvas
- ✅ Multiple node types
- ✅ Real-time file sync
- ✅ CLI launcher
- ✅ Browser support

### Next
- ⬜ Undo/redo
- ⬜ Keyboard navigation
- ⬜ Export functionality

### Later
- ⬜ Multi-board support
- ⬜ Real-time collaboration (CRDT)
- ⬜ Plugin system

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

## Acknowledgments

- [Tauri](https://tauri.app) for the amazing desktop framework
- [Leptos](https://leptos.dev) for reactive Rust
- The infinite canvas concept inspired by tools like Miro, Excalidraw, and tldraw

---

<p align="center">
  <strong>Built for humans and AI, working together.</strong>
</p>

<p align="center">
  <a href="https://github.com/LucianoLupo/infinite-brainstorm/issues">Report Bug</a> •
  <a href="https://github.com/LucianoLupo/infinite-brainstorm/issues">Request Feature</a>
</p>
