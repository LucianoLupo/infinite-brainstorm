use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_clipboard_manager::ClipboardExt;

// Flag to skip file watcher emission after our own saves
static SKIP_NEXT_EMIT: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Board {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Node {
    pub id: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    pub text: String,
    #[serde(default = "default_node_type")]
    pub node_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
}

fn default_node_type() -> String {
    "text".to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Edge {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
}

fn get_board_path(_app: &AppHandle) -> PathBuf {
    // Use parent of src-tauri (project root) during dev, or current dir in production
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // If we're in src-tauri, go up one level to project root
    if cwd.ends_with("src-tauri") {
        cwd.parent().unwrap_or(&cwd).join("board.json")
    } else {
        cwd.join("board.json")
    }
}

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

/// Atomically write a board to `path`.
///
/// Strategy: serialize JSON, write it to a sibling temp file (`<path>.tmp`) in
/// the SAME directory, `fsync` it, then `rename` it over `path`. Because rename
/// is atomic on the same filesystem, readers (and the file watcher) never
/// observe a partially-written file. Before the rename, the prior on-disk
/// contents are copied to `<path>.bak` (best-effort). The `SKIP_NEXT_EMIT` flag
/// is set immediately before the rename — the atomic commit point — so the file
/// watcher's debounce window only opens once the new contents are visible.
pub fn write_board_atomic(path: &std::path::Path, board: &Board) -> Result<(), String> {
    use std::io::Write;

    // Create parent directory if needed (only on actual save, not on load)
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    let json = serde_json::to_string_pretty(board).map_err(|e| e.to_string())?;

    // Write the serialized JSON to a sibling temp file in the same directory.
    let tmp_path = {
        let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(".tmp");
        path.with_file_name(name)
    };

    {
        let mut file = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
        file.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        // fsync: flush the temp file's contents to disk before the rename so a
        // crash mid-write can't leave a truncated file at the final path.
        file.sync_all().map_err(|e| e.to_string())?;
    }

    // Best-effort backup of the prior on-disk contents. Ignore failures (e.g.
    // no prior file yet, or a permissions hiccup) — the backup is advisory.
    if path.exists() {
        let bak_path = {
            let mut name = path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_default();
            name.push(".bak");
            path.with_file_name(name)
        };
        let _ = fs::copy(path, &bak_path);
    }

    // Set the skip flag at the atomic commit point — immediately before rename.
    SKIP_NEXT_EMIT.store(true, Ordering::SeqCst);

    fs::rename(&tmp_path, path).map_err(|e| {
        // The rename failed, so we never actually committed — undo the skip flag
        // and clean up the temp file so we don't leave litter behind.
        SKIP_NEXT_EMIT.store(false, Ordering::SeqCst);
        let _ = fs::remove_file(&tmp_path);
        e.to_string()
    })?;

    Ok(())
}

#[tauri::command]
fn save_board(app: AppHandle, board: Board) -> Result<(), String> {
    let path = get_board_path(&app);
    write_board_atomic(&path, &board)
}

#[tauri::command]
fn get_board_path_cmd(app: AppHandle) -> Result<String, String> {
    let path = get_board_path(&app);
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn fetch_link_preview(url: String) -> Result<LinkPreview, String> {
    // Skip non-HTTP URLs (file://, etc.)
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Ok(LinkPreview {
            url: url.clone(),
            title: Some(url),
            description: None,
            image: None,
            site_name: Some("Local File".to_string()),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let html = response.text().await.map_err(|e| e.to_string())?;
    let document = Html::parse_document(&html);

    // Selectors for Open Graph and fallback meta tags
    let og_title = Selector::parse(r#"meta[property="og:title"]"#).ok();
    let og_desc = Selector::parse(r#"meta[property="og:description"]"#).ok();
    let og_image = Selector::parse(r#"meta[property="og:image"]"#).ok();
    let og_site = Selector::parse(r#"meta[property="og:site_name"]"#).ok();
    let meta_desc = Selector::parse(r#"meta[name="description"]"#).ok();
    let title_tag = Selector::parse("title").ok();
    let twitter_image = Selector::parse(r#"meta[name="twitter:image"]"#).ok();

    let get_content = |sel: Option<Selector>| -> Option<String> {
        sel.and_then(|s| {
            document
                .select(&s)
                .next()
                .and_then(|el| el.value().attr("content").map(|s| s.to_string()))
        })
    };

    let title = get_content(og_title).or_else(|| {
        title_tag.and_then(|s| document.select(&s).next().map(|el| el.text().collect()))
    });

    let description = get_content(og_desc.clone()).or_else(|| get_content(meta_desc));

    let mut image = get_content(og_image).or_else(|| get_content(twitter_image));

    // Make relative image URLs absolute
    if let Some(ref img) = image {
        if img.starts_with('/') {
            if let Ok(base) = reqwest::Url::parse(&url) {
                if let Ok(absolute) = base.join(img) {
                    image = Some(absolute.to_string());
                }
            }
        }
    }

    let site_name = get_content(og_site);

    Ok(LinkPreview {
        url,
        title,
        description,
        image,
        site_name,
    })
}

fn get_assets_dir(app: &AppHandle) -> PathBuf {
    let board_path = get_board_path(app);
    let parent = board_path.parent().unwrap_or(&board_path);
    parent.join("assets")
}

/// Maximum byte size for an image we will base64-encode and hand to the
/// webview. Guards against memory exhaustion / DoS via a crafted board.json
/// pointing at a huge file.
const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024; // 25 MB

/// Expand `~`, strip a `file://` prefix, and URL-decode an input path string
/// into a concrete filesystem path. Shared by the file-reading commands so
/// their accepted path syntax stays consistent.
fn expand_path(path: &str) -> PathBuf {
    let expanded = if let Some(rest) = path.strip_prefix('~') {
        match dirs::home_dir() {
            Some(home) => home.join(rest.trim_start_matches('/')).to_string_lossy().into_owned(),
            None => path.to_string(),
        }
    } else if let Some(stripped) = path.strip_prefix("file://") {
        urlencoding::decode(stripped)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| stripped.to_string())
    } else {
        path.to_string()
    };
    PathBuf::from(expanded)
}

/// Resolve `input` to a canonical path and reject it unless it lives inside one
/// of `allowed_roots`. Canonicalization (which also resolves `..` and symlinks)
/// is what stops path-traversal exfiltration: a crafted board.json pointing at
/// `/etc/passwd`, `~/.ssh/id_rsa`, or `<board-dir>/../../secret` resolves to a
/// path that does not start with any allowed root, so we return `Err`.
fn scope_path(input: &str, allowed_roots: &[PathBuf]) -> Result<PathBuf, String> {
    let expanded = expand_path(input);

    let canonical = expanded
        .canonicalize()
        .map_err(|_| format!("File not found: {}", expanded.display()))?;

    let in_scope = allowed_roots.iter().any(|root| {
        root.canonicalize()
            .map(|r| canonical.starts_with(&r))
            .unwrap_or(false)
    });

    if !in_scope {
        return Err("Access denied: path is outside the allowed directories".to_string());
    }

    Ok(canonical)
}

/// The directory that holds the active `board.json`. Local images and assets
/// referenced by the board are scoped to live inside this directory.
fn board_dir(app: &AppHandle) -> PathBuf {
    let board_path = get_board_path(app);
    board_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Sniff the leading magic bytes of `data` and return the matching image MIME
/// type, or `None` if the content is not a supported image format. We trust the
/// file *content*, not its extension — a crafted board.json can rename
/// `/etc/passwd` to `evil.png`, but its bytes won't match any signature here.
fn sniff_image_mime(data: &[u8]) -> Option<&'static str> {
    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    // JPEG: FF D8 FF
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    // GIF: "GIF87a" or "GIF89a"
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // WebP: "RIFF" .... "WEBP"
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // BMP: "BM"
    if data.starts_with(b"BM") {
        return Some("image/bmp");
    }
    None
}

fn ensure_assets_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let assets_dir = get_assets_dir(app);
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir).map_err(|e| format!("Failed to create assets dir: {}", e))?;
    }
    Ok(assets_dir)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PasteImageResult {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// Validate, read, and base64-encode an image. Pure (no AppHandle) so it can be
/// unit-tested: the caller passes the directories the path is allowed to live in.
fn read_image_base64_scoped(path: &str, allowed_roots: &[PathBuf]) -> Result<String, String> {
    // Reject any path that resolves outside the allowed roots (path traversal,
    // absolute paths to system files, etc.).
    let canonical = scope_path(path, allowed_roots)?;

    // Cap file size BEFORE reading the bytes into memory.
    let meta = fs::metadata(&canonical)
        .map_err(|e| format!("Failed to stat file: {}", e))?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "Image too large: {} bytes (max {} bytes)",
            meta.len(),
            MAX_IMAGE_BYTES
        ));
    }

    let data = fs::read(&canonical)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Derive MIME from detected magic bytes, not the file extension. Reject any
    // file whose content is not a supported image format.
    let mime = sniff_image_mime(&data)
        .ok_or_else(|| "Unsupported or non-image file content".to_string())?;

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let b64 = STANDARD.encode(&data);

    Ok(format!("data:{};base64,{}", mime, b64))
}

#[tauri::command]
fn read_image_base64(app: AppHandle, path: String) -> Result<String, String> {
    read_image_base64_scoped(&path, &[board_dir(&app)])
}

/// Validate and read a local Markdown file. Pure (no AppHandle) so it can be
/// unit-tested. Only `.md` files inside an allowed root are readable — this both
/// preserves the Obsidian-vault integration (vault files live under `$HOME`) and
/// blocks exfiltration of non-Markdown system files like `/etc/passwd` or
/// `~/.ssh/id_rsa`.
fn read_markdown_file_scoped(path: &str, allowed_roots: &[PathBuf]) -> Result<String, String> {
    let canonical = scope_path(path, allowed_roots)?;

    let is_md = canonical
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false);
    if !is_md {
        return Err("Access denied: only .md files can be read".to_string());
    }

    std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read {}: {}", canonical.display(), e))
}

#[tauri::command]
fn read_markdown_file(app: AppHandle, path: String) -> Result<String, String> {
    let mut roots = vec![board_dir(&app)];
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    read_markdown_file_scoped(&path, &roots)
}

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

#[tauri::command]
fn paste_image(app: AppHandle) -> Result<PasteImageResult, String> {
    let clipboard = app.clipboard();

    // Try to read image from clipboard
    if let Ok(tauri_image) = clipboard.read_image() {
        let width = tauri_image.width();
        let height = tauri_image.height();
        let rgba_data = tauri_image.rgba();

        // Convert RGBA to PNG using image crate
        let img_buffer: image::RgbaImage = image::ImageBuffer::from_raw(width, height, rgba_data.to_vec())
            .ok_or_else(|| "Failed to create image buffer".to_string())?;

        // Generate unique filename
        let filename = format!("{}.png", uuid::Uuid::new_v4());
        let assets_dir = ensure_assets_dir(&app)?;
        let dest_path = assets_dir.join(&filename);

        // Save as PNG
        img_buffer.save_with_format(&dest_path, image::ImageFormat::Png)
            .map_err(|e| format!("Failed to save image: {}", e))?;

        return Ok(PasteImageResult {
            path: dest_path.to_string_lossy().to_string(),
            width,
            height,
        });
    }

    // Try to read text (might be a file path)
    if let Ok(text) = clipboard.read_text() {
        let text = text.trim();

        // Check if it's a file path to an image
        let path = PathBuf::from(text);
        if path.exists() && path.is_file() {
            let ext = path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&ext.as_str()) {
                // Read and decode to get dimensions
                let data = fs::read(&path)
                    .map_err(|e| format!("Failed to read file: {}", e))?;
                let img = image::load_from_memory(&data)
                    .map_err(|e| format!("Failed to decode image: {}", e))?;

                let width = img.width();
                let height = img.height();

                // Copy to assets folder
                let filename = format!("{}.png", uuid::Uuid::new_v4());
                let assets_dir = ensure_assets_dir(&app)?;
                let dest_path = assets_dir.join(&filename);

                // Save as PNG to normalize format
                img.save_with_format(&dest_path, image::ImageFormat::Png)
                    .map_err(|e| format!("Failed to save image: {}", e))?;

                return Ok(PasteImageResult {
                    path: dest_path.to_string_lossy().to_string(),
                    width,
                    height,
                });
            }
        }
    }

    Err("No image found in clipboard".to_string())
}

fn setup_file_watcher(app: AppHandle) {
    let board_path = get_board_path(&app);
    // Don't create board.json here - let user create it by adding nodes

    std::thread::spawn(move || {
        let (tx, rx) = channel();

        let mut watcher: RecommendedWatcher = Watcher::new(
            tx,
            Config::default().with_poll_interval(Duration::from_millis(500)),
        )
        .expect("Failed to create watcher");

        if let Some(parent) = board_path.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .expect("Failed to watch directory");
        }

        // Debounce: track last emit time to avoid multiple emissions for one save
        let mut last_emit: Option<std::time::Instant> = None;
        let debounce_duration = Duration::from_millis(500);

        loop {
            match rx.recv() {
                Ok(event) => {
                    if let Ok(event) = event {
                        let is_board_file = event.paths.iter().any(|p| {
                            p.file_name()
                                .map(|n| n == "board.json")
                                .unwrap_or(false)
                        });

                        if is_board_file {
                            match event.kind {
                                notify::EventKind::Modify(_) | notify::EventKind::Create(_) => {
                                    // Check if we should skip this emission (our own save)
                                    let was_skip_set = SKIP_NEXT_EMIT.swap(false, Ordering::SeqCst);
                                    if was_skip_set {
                                        continue; // Skip emitting for our own save
                                    }

                                    let now = std::time::Instant::now();
                                    let should_emit = last_emit
                                        .map(|t| now.duration_since(t) >= debounce_duration)
                                        .unwrap_or(true);

                                    if should_emit {
                                        last_emit = Some(now);
                                        std::thread::sleep(Duration::from_millis(100));
                                        let _ = app.emit("board-changed", ());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Watch error: {:?}", e);
                    break;
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            setup_file_watcher(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![load_board, save_board, get_board_path_cmd, fetch_link_preview, paste_image, read_image_base64, read_markdown_file, delete_asset])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    mod board_tests {
        use super::*;

        #[test]
        fn default_board_is_empty() {
            let board = Board::default();
            assert!(board.nodes.is_empty());
            assert!(board.edges.is_empty());
        }

        #[test]
        fn serde_round_trip() {
            let board = Board {
                nodes: vec![
                    Node {
                        id: "n1".to_string(),
                        x: 0.0,
                        y: 0.0,
                        width: 200.0,
                        height: 100.0,
                        text: "First".to_string(),
                        node_type: "text".to_string(),
                        color: None,
                        tags: vec![],
                        status: None,
                        group: None,
                        priority: None,
                    },
                    Node {
                        id: "n2".to_string(),
                        x: 250.0,
                        y: 0.0,
                        width: 200.0,
                        height: 100.0,
                        text: "Second".to_string(),
                        node_type: "idea".to_string(),
                        color: None,
                        tags: vec![],
                        status: None,
                        group: None,
                        priority: None,
                    },
                ],
                edges: vec![Edge {
                    id: "e1".to_string(),
                    from_node: "n1".to_string(),
                    to_node: "n2".to_string(),
                    label: None,
                }],
            };

            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();

            assert_eq!(board.nodes.len(), deserialized.nodes.len());
            assert_eq!(board.edges.len(), deserialized.edges.len());
            assert_eq!(board.nodes[0].id, deserialized.nodes[0].id);
            assert_eq!(board.nodes[0].text, deserialized.nodes[0].text);
            assert_eq!(board.nodes[1].node_type, deserialized.nodes[1].node_type);
        }

        #[test]
        fn deserialize_with_missing_node_type_uses_default() {
            let json = r#"{
                "nodes": [{
                    "id": "n1",
                    "x": 0,
                    "y": 0,
                    "width": 200,
                    "height": 100,
                    "text": "No type"
                }],
                "edges": []
            }"#;

            let board: Board = serde_json::from_str(json).unwrap();
            assert_eq!(board.nodes[0].node_type, "text");
        }

        #[test]
        fn deserialize_old_json_without_metadata_fields() {
            let json = r#"{
                "nodes": [{
                    "id": "n1",
                    "x": 0, "y": 0, "width": 200, "height": 100,
                    "text": "Old node", "node_type": "idea"
                }],
                "edges": []
            }"#;
            let board: Board = serde_json::from_str(json).unwrap();
            let node = &board.nodes[0];
            assert!(node.color.is_none());
            assert!(node.tags.is_empty());
            assert!(node.status.is_none());
            assert!(node.group.is_none());
            assert!(node.priority.is_none());
        }

        #[test]
        fn serde_round_trip_with_metadata() {
            let node = Node {
                id: "m1".to_string(),
                x: 0.0, y: 0.0, width: 200.0, height: 100.0,
                text: "Meta".to_string(),
                node_type: "note".to_string(),
                color: Some("#ff0000".to_string()),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                status: Some("done".to_string()),
                group: Some("g1".to_string()),
                priority: Some(1),
            };
            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.color, node.color);
            assert_eq!(deserialized.tags, node.tags);
            assert_eq!(deserialized.status, node.status);
            assert_eq!(deserialized.group, node.group);
            assert_eq!(deserialized.priority, node.priority);
        }

        #[test]
        fn skip_serializing_empty_metadata() {
            let node = Node {
                id: "n1".to_string(),
                x: 0.0, y: 0.0, width: 200.0, height: 100.0,
                text: "Plain".to_string(),
                node_type: "text".to_string(),
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };
            let json = serde_json::to_string(&node).unwrap();
            assert!(!json.contains("color"));
            assert!(!json.contains("tags"));
            assert!(!json.contains("status"));
            assert!(!json.contains("group"));
            assert!(!json.contains("priority"));
        }

        #[test]
        fn serialize_produces_valid_json() {
            let board = Board {
                nodes: vec![Node {
                    id: "test".to_string(),
                    x: 100.0,
                    y: 200.0,
                    width: 200.0,
                    height: 100.0,
                    text: "Hello \"world\"".to_string(),
                    node_type: "text".to_string(),
                    color: None,
                    tags: vec![],
                    status: None,
                    group: None,
                    priority: None,
                }],
                edges: vec![],
            };

            let json = serde_json::to_string_pretty(&board).unwrap();
            assert!(json.contains("\"id\": \"test\""));
            assert!(json.contains("\"x\": 100.0"));
            assert!(json.contains("Hello \\\"world\\\""));
        }
    }

    mod link_preview_tests {
        use super::*;

        #[test]
        fn default_has_empty_url() {
            let preview = LinkPreview::default();
            assert_eq!(preview.url, "");
            assert!(preview.title.is_none());
        }

        #[test]
        fn serde_with_optional_fields() {
            let preview = LinkPreview {
                url: "https://example.com".to_string(),
                title: Some("Title".to_string()),
                description: None,
                image: Some("https://example.com/img.png".to_string()),
                site_name: None,
            };

            let json = serde_json::to_string(&preview).unwrap();
            let deserialized: LinkPreview = serde_json::from_str(&json).unwrap();

            assert_eq!(preview.url, deserialized.url);
            assert_eq!(preview.title, deserialized.title);
            assert_eq!(preview.description, deserialized.description);
            assert_eq!(preview.image, deserialized.image);
        }
    }

    mod stress_tests {
        use super::*;

        #[test]
        fn board_with_1000_nodes_serde() {
            let nodes: Vec<Node> = (0..1000)
                .map(|i| Node {
                    id: format!("node-{}", i),
                    x: (i % 50) as f64 * 250.0,
                    y: (i / 50) as f64 * 150.0,
                    width: 200.0,
                    height: 100.0,
                    text: format!("Content for node {}", i),
                    node_type: "text".to_string(),
                    color: None,
                    tags: vec![],
                    status: None,
                    group: None,
                    priority: None,
                })
                .collect();

            let board = Board { nodes, edges: vec![] };

            let json = serde_json::to_string_pretty(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();

            assert_eq!(deserialized.nodes.len(), 1000);
            assert_eq!(deserialized.nodes[500].id, "node-500");
        }

        #[test]
        fn board_with_complex_edges() {
            let nodes: Vec<Node> = (0..50)
                .map(|i| Node {
                    id: format!("n{}", i),
                    x: i as f64 * 250.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                    text: format!("Node {}", i),
                    node_type: "text".to_string(),
                    color: None,
                    tags: vec![],
                    status: None,
                    group: None,
                    priority: None,
                })
                .collect();

            let mut edges = Vec::new();
            let mut id = 0;
            for i in 0..50 {
                for j in (i + 1)..50 {
                    edges.push(Edge {
                        id: format!("e{}", id),
                        from_node: format!("n{}", i),
                        to_node: format!("n{}", j),
                        label: None,
                    });
                    id += 1;
                }
            }

            let board = Board { nodes, edges };
            let expected_edges = 50 * 49 / 2;
            assert_eq!(board.edges.len(), expected_edges);

            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.edges.len(), expected_edges);
        }

        #[test]
        fn node_with_large_text() {
            let large_text: String = (0..10000).map(|_| 'x').collect();
            let node = Node {
                id: "large".to_string(),
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
                text: large_text.clone(),
                node_type: "text".to_string(),
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let board = Board { nodes: vec![node], edges: vec![] };
            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();

            assert_eq!(deserialized.nodes[0].text.len(), 10000);
        }
    }

    mod edge_cases {
        use super::*;

        #[test]
        fn node_with_special_characters_in_text() {
            let text = r#"Quotes: "test" 'single' and \backslash\ and tabs	here"#;
            let node = Node {
                id: "special".to_string(),
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
                text: text.to_string(),
                node_type: "text".to_string(),
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.text, text);
        }

        #[test]
        fn node_with_unicode() {
            let text = "日本語 中文 한국어 العربية 🎉🚀";
            let node = Node {
                id: "unicode".to_string(),
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
                text: text.to_string(),
                node_type: "text".to_string(),
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.text, text);
        }

        #[test]
        fn all_node_types() {
            let types = vec!["text", "idea", "note", "image", "md", "link"];
            for node_type in types {
                let node = Node {
                    id: format!("node-{}", node_type),
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                    text: "content".to_string(),
                    node_type: node_type.to_string(),
                    color: None,
                    tags: vec![],
                    status: None,
                    group: None,
                    priority: None,
                };

                let json = serde_json::to_string(&node).unwrap();
                let deserialized: Node = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized.node_type, node_type);
            }
        }

        #[test]
        fn node_with_negative_coordinates() {
            let node = Node {
                id: "neg".to_string(),
                x: -1000.0,
                y: -500.0,
                width: 200.0,
                height: 100.0,
                text: "negative".to_string(),
                node_type: "text".to_string(),
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.x, -1000.0);
            assert_eq!(deserialized.y, -500.0);
        }

        #[test]
        fn node_with_float_precision() {
            let node = Node {
                id: "precise".to_string(),
                x: 123.456789,
                y: 987.654321,
                width: 200.123,
                height: 100.789,
                text: "precise".to_string(),
                node_type: "text".to_string(),
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert!((deserialized.x - 123.456789).abs() < 1e-6);
            assert!((deserialized.y - 987.654321).abs() < 1e-6);
        }
    }

    mod read_markdown_file_tests {
        use super::*;

        // Allowed root for tests = the canonical system temp dir.
        fn temp_roots() -> Vec<PathBuf> {
            vec![std::env::temp_dir()]
        }

        #[test]
        fn reads_absolute_path() {
            let dir = std::env::temp_dir();
            let path = dir.join("test_read_absolute.md");
            let content = "# Test\nHello world";
            std::fs::write(&path, content).unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &temp_roots());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&path).ok();
        }

        #[test]
        fn reads_file_url() {
            let dir = std::env::temp_dir();
            let path = dir.join("test_read_file_url.md");
            let content = "# File URL Test";
            std::fs::write(&path, content).unwrap();

            let file_url = format!("file://{}", path.to_string_lossy());
            let result = read_markdown_file_scoped(&file_url, &temp_roots());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&path).ok();
        }

        #[test]
        fn decodes_url_encoded_spaces() {
            let dir = std::env::temp_dir();
            let subdir = dir.join("test folder");
            std::fs::create_dir_all(&subdir).ok();
            let path = subdir.join("test file.md");
            let content = "# Spaces in path";
            std::fs::write(&path, content).unwrap();

            // URL encode the path with %20 for spaces
            let encoded_path = format!(
                "file://{}",
                path.to_string_lossy().replace(' ', "%20")
            );
            let result = read_markdown_file_scoped(&encoded_path, &temp_roots());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&path).ok();
            std::fs::remove_dir(&subdir).ok();
        }

        #[test]
        fn expands_home_tilde() {
            // This test verifies tilde expansion works
            // We can only test the path transformation, not the actual read
            // unless we create a file in the actual home directory
            let home = dirs::home_dir().unwrap();
            let test_file = home.join(".brainstorm_test_temp.md");
            let content = "# Home test";
            std::fs::write(&test_file, content).unwrap();

            let result = read_markdown_file_scoped("~/.brainstorm_test_temp.md", &[home]);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&test_file).ok();
        }

        #[test]
        fn returns_error_for_nonexistent_file() {
            let result = read_markdown_file_scoped("/nonexistent/path/to/file.md", &temp_roots());
            assert!(result.is_err());
        }

        #[test]
        fn handles_unicode_content() {
            let dir = std::env::temp_dir();
            let path = dir.join("test_unicode.md");
            let content = "# Unicode Test\n日本語 中文 한국어 🎉";
            std::fs::write(&path, content).unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &temp_roots());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&path).ok();
        }

        #[test]
        fn handles_empty_file() {
            let dir = std::env::temp_dir();
            let path = dir.join("test_empty.md");
            std::fs::write(&path, "").unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &temp_roots());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "");

            std::fs::remove_file(&path).ok();
        }

        #[test]
        fn handles_multiple_encoded_characters() {
            let dir = std::env::temp_dir();
            // Create a path with multiple special chars that need encoding
            let subdir = dir.join("test & folder");
            std::fs::create_dir_all(&subdir).ok();
            let path = subdir.join("notes (copy).md");
            let content = "# Special chars in path";
            std::fs::write(&path, content).unwrap();

            // URL encode special characters
            let encoded_path = format!(
                "file://{}",
                path.to_string_lossy()
                    .replace(' ', "%20")
                    .replace('&', "%26")
                    .replace('(', "%28")
                    .replace(')', "%29")
            );
            let result = read_markdown_file_scoped(&encoded_path, &temp_roots());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&path).ok();
            std::fs::remove_dir(&subdir).ok();
        }

        #[test]
        fn rejects_md_file_outside_allowed_roots() {
            // A .md file that exists but lives outside the allowed roots is denied.
            let dir = std::env::temp_dir();
            let path = dir.join("test_outside_scope.md");
            std::fs::write(&path, "# secret").unwrap();

            // Allowed root is an unrelated subdirectory, NOT the temp dir itself.
            let other_root = dir.join("brainstorm_unrelated_root");
            std::fs::create_dir_all(&other_root).ok();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &[other_root.clone()]);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("Access denied"));

            std::fs::remove_file(&path).ok();
            std::fs::remove_dir(&other_root).ok();
        }

        #[test]
        fn rejects_non_md_file_inside_allowed_roots() {
            // Even inside an allowed root, a non-.md file (e.g. an imitation of a
            // system secret) is rejected by the extension guard.
            let dir = std::env::temp_dir();
            let path = dir.join("test_secret_creds.txt");
            std::fs::write(&path, "topsecret").unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &temp_roots());
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("only .md"));

            std::fs::remove_file(&path).ok();
        }
    }

    mod path_scope_tests {
        use super::*;

        #[test]
        fn rejects_etc_passwd_for_image_read() {
            // /etc/passwd is outside any board dir and not image bytes anyway.
            let dir = std::env::temp_dir();
            let board = dir.join("brainstorm_board_scope_a");
            std::fs::create_dir_all(&board).ok();

            let result = read_image_base64_scoped("/etc/passwd", &[board.clone()]);
            assert!(result.is_err(), "/etc/passwd must be rejected");

            std::fs::remove_dir(&board).ok();
        }

        #[test]
        fn rejects_ssh_key_for_markdown_read() {
            let home = dirs::home_dir().unwrap();
            // ~/.ssh/id_rsa: even if it existed under the home root, it is not a
            // .md file, so the extension guard rejects it. And on machines where
            // it doesn't exist, canonicalize fails first. Either way -> Err.
            let result = read_markdown_file_scoped("~/.ssh/id_rsa", &[home]);
            assert!(result.is_err(), "~/.ssh/id_rsa must be rejected");
        }

        #[test]
        fn rejects_path_traversal_escape() {
            // A path that climbs out of the board dir via `..` must be rejected
            // because canonicalization resolves it outside the allowed root.
            let dir = std::env::temp_dir();
            let board = dir.join("brainstorm_board_scope_b");
            std::fs::create_dir_all(&board).ok();

            // Create a real .md file OUTSIDE the board dir, then reference it via ..
            let secret = dir.join("brainstorm_outside_secret.md");
            std::fs::write(&secret, "# leak").unwrap();

            let traversal = format!("{}/../brainstorm_outside_secret.md", board.to_string_lossy());
            let result = read_markdown_file_scoped(&traversal, &[board.clone()]);
            assert!(result.is_err(), "traversal escape must be rejected");

            std::fs::remove_file(&secret).ok();
            std::fs::remove_dir(&board).ok();
        }

        #[test]
        fn allows_image_inside_board_dir() {
            // A real PNG inside the board dir is accepted and base64-encoded.
            let dir = std::env::temp_dir();
            let board = dir.join("brainstorm_board_scope_c");
            std::fs::create_dir_all(&board).ok();

            // Minimal valid PNG magic-byte header (signature is enough for sniffing).
            let png_sig = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01];
            let img = board.join("pic.png");
            std::fs::write(&img, png_sig).unwrap();

            let result = read_image_base64_scoped(&img.to_string_lossy(), &[board.clone()]);
            assert!(result.is_ok(), "board-dir image should load: {:?}", result);
            assert!(result.unwrap().starts_with("data:image/png;base64,"));

            std::fs::remove_file(&img).ok();
            std::fs::remove_dir(&board).ok();
        }

        #[test]
        fn rejects_non_image_content_with_image_extension() {
            // A text file renamed to .png is rejected by magic-byte sniffing.
            let dir = std::env::temp_dir();
            let board = dir.join("brainstorm_board_scope_d");
            std::fs::create_dir_all(&board).ok();

            let fake = board.join("evil.png");
            std::fs::write(&fake, b"root:x:0:0:root:/root:/bin/bash\n").unwrap();

            let result = read_image_base64_scoped(&fake.to_string_lossy(), &[board.clone()]);
            assert!(result.is_err(), "non-image content must be rejected");
            assert!(result.unwrap_err().contains("non-image"));

            std::fs::remove_file(&fake).ok();
            std::fs::remove_dir(&board).ok();
        }

        #[test]
        fn rejects_oversized_image() {
            let dir = std::env::temp_dir();
            let board = dir.join("brainstorm_board_scope_e");
            std::fs::create_dir_all(&board).ok();

            // Write a file larger than the cap. Start with a PNG signature so the
            // size check (which runs first) is unambiguously what rejects it.
            let big = board.join("huge.png");
            let mut data = vec![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
            data.resize((MAX_IMAGE_BYTES + 1) as usize, 0u8);
            std::fs::write(&big, &data).unwrap();

            let result = read_image_base64_scoped(&big.to_string_lossy(), &[board.clone()]);
            assert!(result.is_err(), "oversized image must be rejected");
            assert!(result.unwrap_err().contains("too large"));

            std::fs::remove_file(&big).ok();
            std::fs::remove_dir(&board).ok();
        }

        #[test]
        fn sniff_detects_supported_formats() {
            assert_eq!(sniff_image_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]), Some("image/png"));
            assert_eq!(sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
            assert_eq!(sniff_image_mime(b"GIF89a..."), Some("image/gif"));
            assert_eq!(sniff_image_mime(b"GIF87a..."), Some("image/gif"));
            assert_eq!(sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
            assert_eq!(sniff_image_mime(b"BM\0\0"), Some("image/bmp"));
            assert_eq!(sniff_image_mime(b"not an image"), None);
            assert_eq!(sniff_image_mime(&[]), None);
        }
    }
}
