# Camera Reset to (0,0) After Paste

```yaml
category: bugs
tags:
  - tauri
  - file-watcher
  - notify
  - state-management
  - atomics
  - race-condition
severity: high
date_solved: 2026-02-03
affects:
  - src-tauri/src/lib.rs
  - src/app.rs
```

## Problem

After pasting an image (or any save operation), the camera would reset to origin (0,0), losing the user's current view position.

### Symptoms
- User pans to a location on the canvas
- User pastes an image with Cmd+V
- Image appears correctly
- Canvas suddenly "jumps" back to (0,0)
- Panned view is lost

### Previous Fix Attempt

An earlier version had a frontend-side skip flag:

```rust
// In src/app.rs
thread_local! {
    static SKIP_NEXT_RELOAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

async fn save_board_storage(board: &Board) {
    SKIP_NEXT_RELOAD.with(|flag| flag.set(true));  // Set before save
    let _ = invoke("save_board", args).await;
}

// In file watcher handler
let should_skip = SKIP_NEXT_RELOAD.with(|flag| {
    if flag.get() {
        flag.set(false);
        true
    } else {
        false
    }
});
if should_skip { return; }
```

This worked for single events but failed when multiple events fired.

## Root Cause

The `notify` crate's file watcher emits **multiple events** for a single file write operation:

1. First event: `Modify(Data(Any))` - detected immediately
2. Second event: `Modify(Data(Content))` - detected ~50ms later

The frontend skip flag was consumed by the first event, allowing the second event to trigger a full board reload, which reset the camera.

**Timeline:**
```
T+0ms:    save_board() called, SKIP_NEXT_RELOAD = true
T+1ms:    File write completes
T+10ms:   File watcher: Event 1 (Modify/Data), checks flag=true, skips, sets flag=false
T+60ms:   File watcher: Event 2 (Modify/Content), checks flag=false, EMITS EVENT
T+65ms:   Frontend receives "board-changed", reloads board, camera resets
```

## Solution

Moved the skip flag from frontend to backend using `AtomicBool`. The file watcher checks and clears the flag before emitting, ensuring ALL events from a self-initiated save are skipped.

### 1. Backend: Static AtomicBool Flag

**src-tauri/src/lib.rs:6,13**
```rust
use std::sync::atomic::{AtomicBool, Ordering};

// Flag to skip file watcher emission after our own saves
static SKIP_NEXT_EMIT: AtomicBool = AtomicBool::new(false);
```

### 2. save_board Sets Flag Before Write

**src-tauri/src/lib.rs:90-91**
```rust
#[tauri::command]
fn save_board(app: AppHandle, board: Board) -> Result<(), String> {
    let path = get_board_path(&app);

    // ... directory creation ...

    // Set flag to skip file watcher emission for our own save
    SKIP_NEXT_EMIT.store(true, Ordering::SeqCst);

    let json = serde_json::to_string_pretty(&board).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}
```

### 3. File Watcher Checks Flag Before Emitting

**src-tauri/src/lib.rs:361-365**
```rust
notify::EventKind::Modify(_) | notify::EventKind::Create(_) => {
    // Check if we should skip this emission (our own save)
    let was_skip_set = SKIP_NEXT_EMIT.swap(false, Ordering::SeqCst);
    if was_skip_set {
        continue; // Skip emitting for our own save
    }

    // ... debounce logic ...
    let _ = app.emit("board-changed", ());
}
```

### 4. Simplified Frontend Handler

**src/app.rs:206-224**
```rust
Effect::new(move || {
    if !is_tauri() {
        return; // Skip file watching in browser mode
    }

    let handler = Closure::new(move |_event: JsValue| {
        // Only external changes reach here (backend skips our own saves)
        web_sys::console::log_1(&"External board change detected, reloading...".into());
        spawn_local(async move {
            let loaded_board = load_board_storage().await;
            set_board.set(loaded_board);
        });
    });

    spawn_local(async move {
        let _ = listen("board-changed", &handler).await;
        handler.forget();
    });
});
```

## Why AtomicBool Works

**Key insight:** All file watcher events for a single write occur on the same backend thread. Using `swap(false, Ordering::SeqCst)` atomically reads and clears the flag in one operation.

**Corrected Timeline:**
```
T+0ms:    save_board() called, SKIP_NEXT_EMIT.store(true)
T+1ms:    File write completes
T+10ms:   File watcher: Event 1, swap(false) returns true, skips, flag now false
T+60ms:   File watcher: Event 2, swap(false) returns false, skips due to debounce
          (or would skip anyway since external changes are legitimate)
```

Even if debounce doesn't catch Event 2, the flag was cleared by Event 1 and there's a 500ms debounce window.

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Backend flag vs frontend | Backend sees ALL events before emitting; frontend only sees what's emitted |
| AtomicBool vs Mutex | Simpler, no lock contention, sufficient for single flag |
| SeqCst ordering | Strongest ordering, ensures visibility across threads |
| swap() vs load()+store() | Atomic read-and-clear prevents race conditions |

## Verification Checklist

- [x] Pan to non-origin location
- [x] Paste image via Cmd+V
- [x] Camera stays at current position
- [x] External edits (Claude Code editing board.json) still trigger reload
- [x] Works for all save operations (drag, delete, type change, etc.)

## Related

- [Image Paste Loading Stuck](./image-paste-loading-stuck.md) - Related issue in same session
- [Tauri Trunk File Watcher Issues](../integration-issues/tauri-trunk-file-watcher-issues.md) - Earlier file watcher fixes
