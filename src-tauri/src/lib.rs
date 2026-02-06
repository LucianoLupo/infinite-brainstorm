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
    pub width: f64,
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

#[tauri::command]
fn save_board(app: AppHandle, board: Board) -> Result<(), String> {
    let path = get_board_path(&app);

    // Create parent directory if needed (only on actual save, not on load)
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }

    // Set flag to skip file watcher emission for our own save
    SKIP_NEXT_EMIT.store(true, Ordering::SeqCst);

    let json = serde_json::to_string_pretty(&board).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
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

#[tauri::command]
fn read_markdown_file(path: String) -> Result<String, String> {
    // Expand ~ to home directory, strip file:// prefix, and URL-decode
    let expanded = if path.starts_with('~') {
        dirs::home_dir()
            .map(|h| path.replacen('~', &h.to_string_lossy(), 1))
            .unwrap_or(path)
    } else if path.starts_with("file://") {
        let stripped = path.strip_prefix("file://").unwrap_or(&path);
        // URL-decode the path (handles %20 for spaces, etc.)
        urlencoding::decode(stripped)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| stripped.to_string())
    } else {
        path
    };

    std::fs::read_to_string(&expanded)
        .map_err(|e| format!("Failed to read {}: {}", expanded, e))
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

        #[test]
        fn reads_absolute_path() {
            let dir = std::env::temp_dir();
            let path = dir.join("test_read_absolute.md");
            let content = "# Test\nHello world";
            std::fs::write(&path, content).unwrap();

            let result = read_markdown_file(path.to_string_lossy().to_string());
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
            let result = read_markdown_file(file_url);
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
            let result = read_markdown_file(encoded_path);
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

            let result = read_markdown_file("~/.brainstorm_test_temp.md".to_string());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&test_file).ok();
        }

        #[test]
        fn returns_error_for_nonexistent_file() {
            let result = read_markdown_file("/nonexistent/path/to/file.md".to_string());
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("Failed to read"));
        }

        #[test]
        fn handles_unicode_content() {
            let dir = std::env::temp_dir();
            let path = dir.join("test_unicode.md");
            let content = "# Unicode Test\n日本語 中文 한국어 🎉";
            std::fs::write(&path, content).unwrap();

            let result = read_markdown_file(path.to_string_lossy().to_string());
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&path).ok();
        }

        #[test]
        fn handles_empty_file() {
            let dir = std::env::temp_dir();
            let path = dir.join("test_empty.md");
            std::fs::write(&path, "").unwrap();

            let result = read_markdown_file(path.to_string_lossy().to_string());
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
            let result = read_markdown_file(encoded_path);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&path).ok();
            std::fs::remove_dir(&subdir).ok();
        }
    }
}
