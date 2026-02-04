# `brainstorm .` Command Overwrites Existing board.json

```yaml
category: bugs
tags:
  - cli
  - initialization
  - file-creation
  - startup
severity: medium
date_solved: 2026-02-03
affects:
  - src-tauri/src/lib.rs
```

## Problem

Running `brainstorm .` in a folder with an existing `board.json` would create an empty board, overwriting the existing content.

### Symptoms
- User has existing `board.json` with nodes and edges
- User runs `brainstorm .` or `brainstorm /path/to/folder`
- App opens with empty canvas
- Previous `board.json` content is lost

## Root Cause

The app had an `ensure_board_file()` function that was called on startup:

```rust
// REMOVED - This was the problematic function
fn ensure_board_file(app: &AppHandle) {
    let path = get_board_path(app);
    if !path.exists() {
        let empty = Board::default();
        let json = serde_json::to_string_pretty(&empty).unwrap();
        let _ = fs::write(&path, json);
    }
}
```

This function was called in `setup_file_watcher()` before starting the watcher. The problem: it was supposed to only create the file if it didn't exist, but due to path resolution issues or race conditions, it would sometimes overwrite existing files.

More importantly, this violated the principle of "don't create files until the user takes an action."

## Solution

Removed automatic `board.json` creation on startup. The file is now only created when the user explicitly saves (creates nodes, drags, edits, etc.).

### 1. Removed ensure_board_file Function

Deleted the entire function from `src-tauri/src/lib.rs`.

### 2. Updated load_board to Return Empty Board

**src-tauri/src/lib.rs:66-76**
```rust
#[tauri::command]
fn load_board(app: AppHandle) -> Result<Board, String> {
    let path = get_board_path(&app);

    // If file doesn't exist, return empty board (don't create file until user saves)
    if !path.exists() {
        return Ok(Board::default());
    }

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let board: Board = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(board)
}
```

### 3. Updated setup_file_watcher

**src-tauri/src/lib.rs:325-328**
```rust
fn setup_file_watcher(app: AppHandle) {
    let board_path = get_board_path(&app);
    // Don't create board.json here - let user create it by adding nodes

    std::thread::spawn(move || {
        // ... watcher setup ...
    });
}
```

### 4. save_board Creates Directory If Needed

**src-tauri/src/lib.rs:83-88**
```rust
#[tauri::command]
fn save_board(app: AppHandle, board: Board) -> Result<(), String> {
    let path = get_board_path(&app);

    // Create parent directory if needed (only on actual save, not on load)
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }

    // ... rest of save logic ...
}
```

## Behavior Change

**Before:**
1. Run `brainstorm /path/to/folder`
2. App creates/overwrites `board.json` immediately
3. Existing content potentially lost

**After:**
1. Run `brainstorm /path/to/folder`
2. App loads existing `board.json` if present, or shows empty canvas
3. `board.json` only created when user first adds a node
4. Existing content preserved

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Lazy file creation | Follows Unix philosophy of not creating files until needed |
| Return empty Board on missing file | Clean separation between "no file" and "empty file" |
| Create directory on save | Handles case where user starts fresh in new folder |

## Agent-Native Implications

This change improves the agent-native design:
- Claude Code can check if `board.json` exists before deciding to create content
- No race condition between app startup and agent writing
- Clean slate: if no `board.json`, agent knows it's starting fresh

## Verification

1. Create folder without `board.json`
2. Run `brainstorm .`
3. Confirm no `board.json` created yet
4. Double-click to create node
5. Confirm `board.json` now exists with the node

6. Close app
7. Run `brainstorm .` again
8. Confirm node still present (file was loaded, not overwritten)

## Related

- [Tauri Trunk File Watcher Issues](../integration-issues/tauri-trunk-file-watcher-issues.md) - Path resolution context
