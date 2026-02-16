# Infinite Brainstorm

An agent-native infinite canvas for human-AI collaboration built with Tauri v2 and Leptos.

## Overview

A desktop brainstorming app designed for seamless AI collaboration. Both humans and AI assistants (like Claude Code) work on the same canvas by editing a shared `board.json` file. The app watches for file changes and syncs updates in real-time (<100ms), enabling AI to generate content, reorganize layouts, and connect ideas while you explore visually.

Unlike traditional tools where AI is bolted on through APIs, this is **agent-native by design** — the JSON file is the API.

## Tech Stack

- **Desktop Framework**: Tauri v2
- **Frontend**: Leptos 0.8 (Rust compiled to WASM)
- **Rendering**: HTML5 Canvas
- **File Watching**: notify crate
- **Link Previews**: reqwest + scraper (Open Graph metadata)

## Quick Start

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk tauri-cli

git clone https://github.com/LucianoLupo/infinite-brainstorm.git
cd infinite-brainstorm
cargo tauri dev
```

## Key Features

- **Infinite canvas** with pan/zoom controls
- **6 node types**: text, idea, note, image, markdown, link preview
- **Directed edges** with arrowheads for relationships
- **Node metadata**: color, tags, status, group, priority
- **Real-time file sync**: external edits appear in <100ms
- **Undo/redo** with full history (Cmd+Z / Cmd+Shift+Z)
- **Image paste**: Cmd+V saves clipboard images to `./assets/`
- **Obsidian integration**: local `.md` file links render as markdown
- **Board templates**: 6 ready-to-use layouts (mind map, kanban, flowchart, SWOT, pros/cons, timeline)
- **Claude Code skill**: bundled skill with schema docs and layout algorithms

## Controls

| Action | Shortcut |
|--------|----------|
| Create node | Double-click empty space |
| Edit node | Double-click node |
| Move nodes | Drag |
| Create edge | Shift+drag from node |
| Box select | Cmd/Ctrl+drag |
| Zoom | Scroll wheel |
| Paste image | Cmd/Ctrl+V |
| Cycle type | T |
| Undo/Redo | Cmd/Ctrl+Z / Cmd/Ctrl+Shift+Z |

## License

MIT
