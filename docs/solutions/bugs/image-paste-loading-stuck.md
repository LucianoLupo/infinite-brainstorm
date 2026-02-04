# Image Paste Feature - Images Stuck at "Loading"

```yaml
category: bugs
tags:
  - tauri
  - image
  - asset-protocol
  - base64
  - file-system
severity: high
date_solved: 2026-02-03
affects:
  - src-tauri/src/lib.rs
  - src-tauri/Cargo.toml
  - src/app.rs
```

## Problem

When pasting images via Cmd+V, image nodes were created but displayed "Loading" indefinitely. The images never rendered on the canvas.

### Symptoms
- Cmd+V successfully created image nodes with correct dimensions
- Console showed `paste_image` returning valid file paths
- Image nodes displayed "Loading" text permanently
- No error messages in console initially

### Investigation

The `paste_image` command worked correctly:
1. Image data extracted from clipboard
2. PNG file created in `./assets/` folder
3. File path returned to frontend

The issue was in image loading. Original implementation used Tauri's asset protocol:

```rust
// Original approach in app.rs
let final_url = if url.starts_with("/") || url.starts_with("~") {
    format!("asset://localhost{}", url.replace("~", "/Users/lucianolupo"))
} else {
    url.clone()
};
img.set_src(&final_url);
```

Despite `tauri.conf.json` having asset protocol enabled:
```json
{
  "security": {
    "assetProtocol": {
      "enable": true,
      "scope": ["**"]
    }
  }
}
```

The asset protocol returned HTTP 500 errors for local file paths.

## Root Cause

Tauri's asset protocol (`asset://localhost/path`) has strict security requirements and doesn't reliably work with arbitrary file paths, even with permissive scope settings. The protocol is designed for bundled assets, not dynamically-created user files.

## Solution

Replaced the asset protocol approach with base64 data URLs.

### 1. Added `read_image_base64` Tauri Command

**src-tauri/src/lib.rs:197-226**
```rust
#[tauri::command]
fn read_image_base64(path: String) -> Result<String, String> {
    let path = PathBuf::from(&path);

    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    let data = fs::read(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Detect MIME type from extension
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();

    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/png",
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let b64 = STANDARD.encode(&data);

    Ok(format!("data:{};base64,{}", mime, b64))
}
```

### 2. Added base64 Crate Dependency

**src-tauri/Cargo.toml:33**
```toml
base64 = "0.22"
```

### 3. Updated Frontend Image Loading

**src/app.rs:250-271**
```rust
// Determine image source URL
let image_src = if url_for_async.starts_with("http://") || url_for_async.starts_with("https://") {
    // HTTP URL - use directly
    url_for_async.clone()
} else if is_tauri() {
    // Local file - use Tauri command to convert to base64
    #[derive(Serialize)]
    struct ReadImageArgs { path: String }
    let args = serde_wasm_bindgen::to_value(&ReadImageArgs { path: url_for_async.clone() }).unwrap();
    match invoke("read_image_base64", args).await.as_string() {
        Some(data_url) => data_url,
        None => {
            web_sys::console::error_1(&format!("Failed to read image: {}", url_for_async).into());
            return;
        }
    }
} else {
    // Browser mode - can't load local files
    web_sys::console::error_1(&"Local files not supported in browser mode".into());
    return;
};
```

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Base64 data URLs | Bypass asset protocol entirely; works reliably with any file path |
| Backend file reading | Frontend WASM can't access filesystem directly |
| MIME type detection | Browser needs correct MIME for proper rendering |
| Keep path in board.json | Allows re-reading file if needed; portable between machines |

## Trade-offs

**Pros:**
- Works reliably for all local file paths
- No security configuration needed
- Works immediately after file creation

**Cons:**
- Memory overhead: entire image loaded into JS heap as string
- No browser caching of base64 URLs
- Large images increase IPC payload

**Mitigation:** For typical brainstorming images (screenshots, diagrams), base64 overhead is negligible. For very large images, consider thumbnail generation.

## Verification

After implementing:
1. Cmd+V pastes image from clipboard
2. Image node appears immediately with thumbnail
3. Double-click opens full-size modal
4. Works for PNG, JPEG, GIF, WebP, BMP

## Related

- [File Watcher Camera Reset](./file-watcher-camera-reset.md) - Solved in same session
- [Image Markdown Nodes](../features/image-markdown-nodes.md) - Original image implementation
