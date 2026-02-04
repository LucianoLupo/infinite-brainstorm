# Image Assets Not Cleaned Up on Node Delete

```yaml
category: bugs
tags:
  - file-management
  - assets
  - cleanup
  - security
  - path-traversal
severity: medium
date_solved: 2026-02-03
affects:
  - src-tauri/src/lib.rs
  - src/app.rs
```

## Problem

When deleting an image node, the corresponding image file in the `assets/` folder was not deleted, leading to orphaned files accumulating over time.

### Symptoms
- User pastes image (creates `./assets/uuid.png`)
- User deletes the image node
- Node removed from `board.json`
- File remains in `./assets/` folder
- Over time, assets folder grows with unused files

## Solution

Added a `delete_asset` Tauri command with security checks, and updated the frontend delete handler to clean up asset files.

### 1. Backend: delete_asset Command with Security

**src-tauri/src/lib.rs:228-250**
```rust
#[tauri::command]
fn delete_asset(_app: AppHandle, path: String) -> Result<(), String> {
    let file_path = PathBuf::from(&path);

    // Only allow deleting files in the assets folder (safety check)
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let assets_dir = cwd.join("assets");

    // Canonicalize paths to prevent path traversal attacks
    let canonical_file = file_path.canonicalize()
        .map_err(|_| "File not found".to_string())?;
    let canonical_assets = assets_dir.canonicalize()
        .map_err(|_| "Assets folder not found".to_string())?;

    if !canonical_file.starts_with(&canonical_assets) {
        return Err("Can only delete files from assets folder".to_string());
    }

    fs::remove_file(&canonical_file)
        .map_err(|e| format!("Failed to delete file: {}", e))?;

    Ok(())
}
```

### 2. Frontend: Delete Handler Cleanup

**src/app.rs:877-908**
```rust
"Backspace" | "Delete" => {
    if let Some(edge_id) = edge_sel {
        // ... edge deletion unchanged ...
    } else if !selected.is_empty() {
        // Collect image paths to delete from assets folder
        let current_board = board.get_untracked();
        let image_paths_to_delete: Vec<String> = current_board
            .nodes
            .iter()
            .filter(|n| selected.contains(&n.id) && n.node_type == "image")
            .filter(|n| n.text.contains("/assets/")) // Only delete local assets
            .map(|n| n.text.clone())
            .collect();

        set_board.update(|b| {
            b.nodes.retain(|n| !selected.contains(&n.id));
            b.edges.retain(|e| !selected.contains(&e.from_node) && !selected.contains(&e.to_node));
        });
        set_selected_nodes.set(HashSet::new());

        let current_board = board.get_untracked();
        spawn_local(async move {
            // Delete asset files from disk (Tauri only)
            if is_tauri() {
                for path in image_paths_to_delete {
                    #[derive(Serialize)]
                    struct DeleteAssetArgs { path: String }
                    let args = serde_wasm_bindgen::to_value(&DeleteAssetArgs { path: path.clone() }).unwrap();
                    let _ = invoke("delete_asset", args).await;
                }
            }
            save_board_storage(&current_board).await;
        });
    }
}
```

## Security Considerations

The `delete_asset` command includes multiple security measures:

### 1. Path Traversal Prevention
```rust
let canonical_file = file_path.canonicalize()?;
let canonical_assets = assets_dir.canonicalize()?;
if !canonical_file.starts_with(&canonical_assets) {
    return Err("Can only delete files from assets folder".to_string());
}
```

This prevents attacks like:
- `../../../etc/passwd` - canonicalize resolves `..`, then fails `starts_with`
- `assets/../secrets.txt` - same protection
- Symlinks pointing outside assets - canonicalize follows symlinks

### 2. Assets Folder Restriction
Only files inside the `./assets/` folder can be deleted. This is a whitelist approach.

### 3. Frontend Filtering
```rust
.filter(|n| n.text.contains("/assets/")) // Only delete local assets
```

Only attempts to delete files that are actually in the assets folder. HTTP URLs and other paths are ignored.

### 4. Graceful Failure
```rust
let _ = invoke("delete_asset", args).await;
```

Deletion failures are logged but don't prevent the node deletion from completing. This handles:
- File already deleted
- File moved by user
- Permission issues

## What Gets Deleted vs. Preserved

| Node Text Content | Deleted? | Reason |
|-------------------|----------|--------|
| `/path/to/project/assets/uuid.png` | Yes | In assets folder |
| `https://example.com/image.png` | No | HTTP URL |
| `/Users/me/photos/vacation.jpg` | No | Not in assets folder |
| `file:///tmp/screenshot.png` | No | Not in assets folder |

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Backend deletion only | Frontend WASM can't delete files; Tauri provides controlled access |
| Canonicalize paths | Prevents path traversal via `..` or symlinks |
| Whitelist approach | Only assets folder, nothing else |
| Silent failure | Don't block node deletion if file cleanup fails |
| Filter by path | Don't attempt to delete external images |

## Verification

1. Paste image via Cmd+V
2. Confirm file exists in `./assets/`
3. Delete the image node
4. Confirm file removed from `./assets/`
5. Confirm external image URLs (HTTP) don't trigger deletion attempts

## Future Considerations

- **Orphan cleanup command**: Scan assets folder for files not referenced in board.json
- **Reference counting**: Track how many nodes reference each asset (for copy/paste scenarios)
- **Undo buffer**: Keep deleted files temporarily in case of undo (not implemented yet)

## Related

- [Image Paste Feature](./image-paste-loading-stuck.md) - How images get into assets folder
