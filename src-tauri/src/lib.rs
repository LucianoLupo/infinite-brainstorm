use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

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

fn ensure_board_file(path: &PathBuf) {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let default_board = Board::default();
        let json = serde_json::to_string_pretty(&default_board).unwrap();
        let _ = fs::write(path, json);
    }
}

#[tauri::command]
fn load_board(app: AppHandle) -> Result<Board, String> {
    let path = get_board_path(&app);
    ensure_board_file(&path);

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let board: Board = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(board)
}

#[tauri::command]
fn save_board(app: AppHandle, board: Board) -> Result<(), String> {
    let path = get_board_path(&app);
    ensure_board_file(&path);

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

fn setup_file_watcher(app: AppHandle) {
    let board_path = get_board_path(&app);
    ensure_board_file(&board_path);

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
                                    std::thread::sleep(Duration::from_millis(100));
                                    let _ = app.emit("board-changed", ());
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
        .setup(|app| {
            setup_file_watcher(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![load_board, save_board, get_board_path_cmd, fetch_link_preview])
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
                    },
                    Node {
                        id: "n2".to_string(),
                        x: 250.0,
                        y: 0.0,
                        width: 200.0,
                        height: 100.0,
                        text: "Second".to_string(),
                        node_type: "idea".to_string(),
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
            };

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert!((deserialized.x - 123.456789).abs() < 1e-6);
            assert!((deserialized.y - 987.654321).abs() < 1e-6);
        }
    }
}
