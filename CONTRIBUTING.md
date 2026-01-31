# Contributing to Infinite Brainstorm

Thank you for your interest in contributing! This document provides guidelines and information for contributors.

## Code of Conduct

Be respectful and constructive. We're all here to build something cool together.

## Getting Started

### Prerequisites

```bash
# Rust with WASM target
rustup target add wasm32-unknown-unknown

# Build tools
cargo install trunk tauri-cli

# Optional: wasm-bindgen if trunk gets stuck
cargo install wasm-bindgen-cli --version 0.2.108
```

### Development Workflow

1. **Fork** the repository
2. **Clone** your fork:
   ```bash
   git clone https://github.com/LucianoLupo/infinite-brainstorm.git
   cd infinite-brainstorm
   ```
3. **Run** in development mode:
   ```bash
   cargo tauri dev
   ```
4. **Make changes** — hot reload will pick up most changes
5. **Test** your changes manually and with AI assistants

## Project Structure

```
src/
├── main.rs      # Entry point
├── app.rs       # Main component, event handlers, UI logic
├── canvas.rs    # HTML5 Canvas rendering
└── state.rs     # Data types (Board, Node, Edge, Camera)

src-tauri/
├── src/
│   ├── main.rs  # Tauri entry
│   └── lib.rs   # Backend commands, file watcher
└── tauri.conf.json
```

### Where to Make Changes

| Want to... | Modify... |
|------------|-----------|
| Add a new keyboard shortcut | `src/app.rs` → `on_keydown` |
| Change node appearance | `src/canvas.rs` → `render_node` |
| Add a node property | `src/state.rs` → `Node` struct (and sync with `src-tauri/src/lib.rs`) |
| Add a backend command | `src-tauri/src/lib.rs` |
| Change file watching behavior | `src-tauri/src/lib.rs` → `setup_file_watcher` |

## Coding Guidelines

### Keep It Simple

The codebase is intentionally minimal. Before adding complexity:
- Is this feature essential?
- Can it be done with less code?
- Does it maintain the "agent-native" philosophy?

### Agent-Native Philosophy

Every feature should consider AI collaboration:
- Can Claude Code trigger this via `board.json`?
- Does the JSON schema stay simple?
- Does file watching still work correctly?

### Code Style

- Standard Rust formatting (`cargo fmt`)
- Meaningful variable names
- Comments for non-obvious logic
- Keep functions focused and small

### Types Sync

The `Board`, `Node`, and `Edge` types are duplicated in:
- `src/state.rs` (frontend)
- `src-tauri/src/lib.rs` (backend)

**Keep them in sync!** Changes to one must be reflected in the other.

## Submitting Changes

### Pull Request Process

1. **Create a feature branch**:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes** with clear, atomic commits

3. **Test thoroughly**:
   - Does the app still work?
   - Does file watching still sync?
   - Can you edit `board.json` externally and see changes?

4. **Update documentation** if needed:
   - `README.md` for user-facing features
   - `CLAUDE.md` for AI-relevant changes

5. **Push and create PR**:
   ```bash
   git push origin feature/your-feature-name
   ```
   Then open a Pull Request on GitHub.

### PR Description Template

```markdown
## What does this PR do?
Brief description of changes.

## Why?
Motivation for the change.

## How to test
Steps to verify the changes work.

## Checklist
- [ ] Tested manually
- [ ] Tested with AI editing board.json
- [ ] Updated docs if needed
- [ ] Code formatted with `cargo fmt`
```

## Good First Issues

Looking for something to work on? Here are beginner-friendly tasks:

- **Add keyboard shortcuts** — Simple additions to `on_keydown`
- **Improve node colors** — Tweak the color palette in `canvas.rs`
- **Add tooltips** — Show hints on hover
- **Improve error messages** — Better feedback when things go wrong

## Feature Requests

Have an idea? Open an issue with:
- **What** you want to build
- **Why** it's useful
- **How** it maintains the agent-native philosophy

## Questions?

Open an issue with the `question` label. We're happy to help!

---

Thanks for contributing! 🎉
