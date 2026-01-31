use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

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

fn get_board_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("board.json")
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
        .invoke_handler(tauri::generate_handler![load_board, save_board, get_board_path_cmd])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
