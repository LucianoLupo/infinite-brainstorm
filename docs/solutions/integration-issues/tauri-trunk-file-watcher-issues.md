# Tauri + Trunk + File Watcher Integration Issues

```yaml
category: integration-issues
tags:
  - tauri
  - trunk
  - file-watcher
  - rust
  - wasm
  - leptos
severity: high
date_solved: 2026-01-31
affects:
  - src-tauri/src/lib.rs
  - src/app.rs
  - Trunk.toml
```

## Overview

When building a Tauri v2 + Leptos application with file watching for an "agent-native" design (where AI can edit a JSON file and the app reacts), we encountered four interconnected issues that caused development friction and runtime bugs.

## Problem 1: Wrong Path Resolution in Development

### Symptoms
- Board loaded with 0 nodes despite `board.json` existing in project root
- `load_board` returned empty board
- Works in production but fails in `cargo tauri dev`

### Root Cause
During `cargo tauri dev`, Tauri runs from the `src-tauri/` subdirectory. Using `std::env::current_dir()` returned `/path/to/project/src-tauri/` instead of `/path/to/project/`.

The original code:
```rust
fn get_board_path(_app: &AppHandle) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("board.json")  // Wrong: resolves to src-tauri/board.json
}
```

### Solution
Detect when running from `src-tauri` and go up one level:

```rust
fn get_board_path(_app: &AppHandle) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.ends_with("src-tauri") {
        cwd.parent().unwrap_or(&cwd).join("board.json")
    } else {
        cwd.join("board.json")
    }
}
```

**Location:** `src-tauri/src/lib.rs:10-17`

---

## Problem 2: Trunk Rebuilding on Data File Changes

### Symptoms
- Every change to `board.json` triggered full frontend rebuild
- 2-3 second delay between file save and UI update
- App appeared to "restart" on every interaction

### Root Cause
Trunk's file watcher monitors the entire project directory by default. When `board.json` was modified (by the app or externally), Trunk detected the change and rebuilt the WASM frontend.

### Solution
Add `board.json` to Trunk's ignore list in `Trunk.toml`:

```toml
[watch]
ignore = ["./src-tauri", "./board.json"]
```

**Location:** `Trunk.toml`

---

## Problem 3: File Watcher Feedback Loop

### Symptoms
- View would "jump" back to origin (0,0) after any user interaction
- Clicking a node would reset the camera position
- Canvas flickered after saving

### Root Cause
The file watcher was reloading `board.json` after the app's own saves:

1. User clicks node → app saves to `board.json`
2. File watcher detects change → triggers reload
3. Reload replaces board state (including camera position)
4. View resets to default

### Solution
Implement a thread-local skip flag that prevents reload after self-initiated saves:

```rust
// In src/app.rs
thread_local! {
    static SKIP_NEXT_RELOAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

async fn save_board_storage(board: &Board) {
    if is_tauri() {
        // Set flag BEFORE saving so file watcher will skip the reload
        SKIP_NEXT_RELOAD.with(|flag| flag.set(true));
        let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: board.clone() }).unwrap();
        let _ = invoke("save_board", args).await;
    } else {
        // localStorage fallback
    }
}

// In file watcher handler
let handler = Closure::new(move |_event: JsValue| {
    let should_skip = SKIP_NEXT_RELOAD.with(|flag| {
        if flag.get() {
            flag.set(false);
            true
        } else {
            false
        }
    });

    if should_skip {
        return;  // Skip reload for self-initiated saves
    }

    spawn_local(async move {
        let loaded_board = load_board_storage().await;
        set_board.set(loaded_board);
    });
});
```

**Location:** `src/app.rs:25-27` (flag definition), `src/app.rs:50-55` (save), `src/app.rs:130-145` (handler)

---

## Problem 4: Unused Import Warning

### Symptoms
- Compiler warning: "unused import: `tauri::Manager`"
- Not a runtime issue but indicates code cleanup needed

### Root Cause
The `Manager` trait was imported but never used after refactoring path resolution.

### Solution
Remove the unused import:

```rust
// Before
use tauri::{AppHandle, Emitter, Manager};

// After
use tauri::{AppHandle, Emitter};
```

**Location:** `src-tauri/src/lib.rs:1`

---

## Prevention Strategies

### For Path Resolution Issues

1. **Test in both modes**: Always verify behavior in `cargo tauri dev` AND production build
2. **Log early**: Add logging to path resolution functions during development
3. **Use constants for paths**: Consider a `DEBUG_MODE` constant that adjusts paths

```rust
#[cfg(debug_assertions)]
const DEV_MODE: bool = true;
#[cfg(not(debug_assertions))]
const DEV_MODE: bool = false;
```

### For File Watcher Feedback Loops

1. **Design for bi-directional sync**: When building apps that both read and write watched files, always plan for feedback prevention
2. **Use skip flags or timestamps**: Either track "last save time" or use explicit skip flags
3. **Document the pattern**: Add comments explaining why the flag exists

### For Build Tool Configuration

1. **Separate data from code**: Keep data files in a location that build tools ignore by default
2. **Configure ignores early**: Set up `Trunk.toml` ignore patterns before the first data file is added
3. **Test hot reload**: Verify that editing data files doesn't trigger rebuilds

---

## Verification Checklist

After implementing these fixes, verify:

- [ ] `cargo tauri dev` loads nodes from project root `board.json`
- [ ] Editing `board.json` externally updates the canvas within 100ms
- [ ] Clicking/dragging nodes does NOT reset the view
- [ ] Trunk does NOT rebuild when `board.json` changes
- [ ] No compiler warnings about unused imports
- [ ] Works correctly after `cargo tauri build` (production mode)

---

## Related Documentation

- [CLAUDE.md - Troubleshooting section](../../CLAUDE.md#troubleshooting)
- [README.md - Key Design Decisions](../../README.md#key-design-decisions)
- [CONTRIBUTING.md - Development Workflow](../../CONTRIBUTING.md#development-workflow)

## Architectural Context

These issues arise from the "agent-native" design philosophy where:
- A JSON file (`board.json`) serves as the API between the app and AI assistants
- File watching enables real-time sync for human-AI collaboration
- The same data format works in both Tauri (filesystem) and browser (localStorage) modes

The solutions maintain this design while preventing the side effects that come from having multiple writers to the same file.
