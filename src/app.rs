use crate::canvas::{get_canvas_context, render_board, ImageCache, LinkPreviewCache};
use crate::components::{ImageModal, MarkdownModal, MarkdownOverlays, NodeEditor};
use crate::history::History;
use crate::state::{Board, Camera, Edge, LinkPreview, Node, ResizeHandle, RESIZE_HANDLE_SIZE, MIN_NODE_WIDTH, MIN_NODE_HEIGHT};
use leptos::prelude::*;
use leptos::task::spawn_local;
use pulldown_cmark::{html, Parser};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, HtmlImageElement};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &Closure<dyn Fn(JsValue)>) -> JsValue;
}

const LOCALSTORAGE_KEY: &str = "infinite-brainstorm-board";

fn is_tauri() -> bool {
    web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w, &JsValue::from_str("__TAURI__")).ok())
        .map(|v| !v.is_undefined())
        .unwrap_or(false)
}

async fn load_board_storage() -> Board {
    if is_tauri() {
        let result = invoke("load_board", JsValue::NULL).await;
        serde_wasm_bindgen::from_value::<Board>(result).unwrap_or_default()
    } else {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item(LOCALSTORAGE_KEY).ok().flatten())
            .and_then(|json| serde_json::from_str::<Board>(&json).ok())
            .unwrap_or_default()
    }
}

pub(crate) async fn save_board_storage(board: &Board) {
    if is_tauri() {
        let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: board.clone() }).unwrap();
        let _ = invoke("save_board", args).await;
    } else if let Ok(json) = serde_json::to_string(board) {
        if let Some(storage) = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
        {
            let _ = storage.set_item(LOCALSTORAGE_KEY, &json);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SaveBoardArgs {
    board: Board,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PasteImageResult {
    path: String,
    width: u32,
    height: u32,
}

#[derive(Serialize, Deserialize)]
struct FetchLinkPreviewArgs {
    url: String,
}

#[derive(Serialize, Deserialize)]
struct ReadMarkdownFileArgs {
    path: String,
}

#[derive(Clone, Default)]
struct DragState {
    is_dragging: bool,
    is_box_selecting: bool,
    start_x: f64,
    start_y: f64,
    node_start_positions: HashMap<String, (f64, f64)>,
}

#[derive(Clone)]
struct PanState {
    is_panning: bool,
    start_x: f64,
    start_y: f64,
    camera_start_x: f64,
    camera_start_y: f64,
}

impl Default for PanState {
    fn default() -> Self {
        Self {
            is_panning: false,
            start_x: 0.0,
            start_y: 0.0,
            camera_start_x: 0.0,
            camera_start_y: 0.0,
        }
    }
}

#[derive(Clone, Default)]
struct EdgeCreationState {
    is_creating: bool,
    from_node_id: Option<String>,
    current_x: f64,
    current_y: f64,
}

#[derive(Clone, Default)]
struct ResizeState {
    is_resizing: bool,
    node_id: Option<String>,
    handle: Option<ResizeHandle>,
    start_mouse_x: f64,
    start_mouse_y: f64,
    original_x: f64,
    original_y: f64,
    original_width: f64,
    original_height: f64,
}

fn cycle_node_type(current: &str) -> String {
    match current {
        "text" => "idea".to_string(),
        "idea" => "note".to_string(),
        "note" => "image".to_string(),
        "image" => "md".to_string(),
        "md" => "link".to_string(),
        _ => "text".to_string(),
    }
}

pub(crate) fn parse_markdown(md: &str) -> String {
    let parser = Parser::new(md);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Check if a path points to a local .md file (not HTTP URL)
pub fn is_local_md_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    if !path_lower.ends_with(".md") {
        return false;
    }
    path.starts_with('/') || path.starts_with("file://") || path.starts_with('~')
}

fn intersects_box(node: &Node, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
    let node_right = node.x + node.width;
    let node_bottom = node.y + node.height;
    !(node.x > max_x || node_right < min_x || node.y > max_y || node_bottom < min_y)
}

fn point_near_line(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64, threshold: f64) -> bool {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt() < threshold;
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let closest_x = x1 + t * dx;
    let closest_y = y1 + t * dy;
    let dist = ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt();
    dist < threshold
}

#[derive(Clone, Copy)]
pub struct BoardCtx {
    pub board: ReadSignal<Board>,
    pub set_board: WriteSignal<Board>,
    pub camera: ReadSignal<Camera>,
    pub set_camera: WriteSignal<Camera>,
    pub selected_nodes: ReadSignal<HashSet<String>>,
    pub set_selected_nodes: WriteSignal<HashSet<String>>,
    pub selected_edge: ReadSignal<Option<String>>,
    pub set_selected_edge: WriteSignal<Option<String>>,
    pub editing_node: ReadSignal<Option<String>>,
    pub set_editing_node: WriteSignal<Option<String>>,
    pub modal_image: ReadSignal<Option<String>>,
    pub set_modal_image: WriteSignal<Option<String>>,
    pub modal_md: ReadSignal<Option<(String, bool)>>,
    pub set_modal_md: WriteSignal<Option<(String, bool)>>,
    pub md_edit_text: ReadSignal<String>,
    pub set_md_edit_text: WriteSignal<String>,
    pub md_file_cache: ReadSignal<HashMap<String, Option<String>>>,
}

#[component]
pub fn App() -> impl IntoView {
    let (board, set_board) = signal(Board::default());
    let (camera, set_camera) = signal(Camera::new());
    let (selected_nodes, set_selected_nodes) = signal::<HashSet<String>>(HashSet::new());
    let (selected_edge, set_selected_edge) = signal::<Option<String>>(None);
    let (drag_state, set_drag_state) = signal(DragState::default());
    let (pan_state, set_pan_state) = signal(PanState::default());
    let (editing_node, set_editing_node) = signal::<Option<String>>(None);
    let (edge_creation, set_edge_creation) = signal(EdgeCreationState::default());
    let (resize_state, set_resize_state) = signal(ResizeState::default());
    let (cursor_style, set_cursor_style) = signal("crosshair".to_string());
    let (last_mouse_world_pos, set_last_mouse_world_pos) = signal((0.0f64, 0.0f64));
    let (selection_box, set_selection_box) = signal::<Option<(f64, f64, f64, f64)>>(None);
    let (modal_image, set_modal_image) = signal::<Option<String>>(None);
    let (modal_md, set_modal_md) = signal::<Option<(String, bool)>>(None); // (node_id, is_editing)
    let (md_edit_text, set_md_edit_text) = signal::<String>(String::new()); // Separate signal to avoid re-render on typing
    let (node_clipboard, set_node_clipboard) = signal::<Option<(Vec<Node>, Vec<Edge>)>>(None);

    // Undo/redo history - using Rc<RefCell> since mutations don't need reactivity
    type BoardHistory = Rc<RefCell<History<Board>>>;
    let history: BoardHistory = Rc::new(RefCell::new(History::new(100)));
    let history_for_mouse_down = history.clone();
    let history_for_mouse_up = history.clone();
    let history_for_double_click = history.clone();
    let history_for_keydown = history.clone();
    let history_for_paste = history;  // Last clone can take ownership

    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let file_input_ref = NodeRef::<leptos::html::Input>::new();
    let image_cache: ImageCache = Rc::new(RefCell::new(HashMap::new()));
    let image_cache_for_render = image_cache.clone();
    let image_cache_for_load = image_cache.clone();
    let image_cache_for_link_preview = image_cache.clone();
    let image_cache_for_modal = image_cache.clone();
    let link_preview_cache: LinkPreviewCache = Rc::new(RefCell::new(HashMap::new()));
    let link_preview_cache_for_render = link_preview_cache.clone();
    let link_preview_cache_for_fetch = link_preview_cache.clone();
    // Markdown file cache stored as a signal (for local .md files in link nodes)
    let (md_file_cache, set_md_file_cache) = signal::<HashMap<String, Option<String>>>(HashMap::new());
    let (image_load_trigger, set_image_load_trigger) = signal(0u32);
    let (link_preview_trigger, set_link_preview_trigger) = signal(0u32);

    provide_context(BoardCtx {
        board,
        set_board,
        camera,
        set_camera,
        selected_nodes,
        set_selected_nodes,
        selected_edge,
        set_selected_edge,
        editing_node,
        set_editing_node,
        modal_image,
        set_modal_image,
        modal_md,
        set_modal_md,
        md_edit_text,
        set_md_edit_text,
        md_file_cache,
    });

    // Load board on startup (with small delay to ensure Tauri is ready)
    Effect::new(move || {
        spawn_local(async move {
            // Small delay to ensure Tauri's __TAURI__ is injected
            gloo_timers::future::TimeoutFuture::new(50).await;
            let mut loaded_board = load_board_storage().await;
            for node in &mut loaded_board.nodes {
                if node.width == 0.0 || node.height == 0.0 {
                    let (w, h) = Node::auto_size(&node.text);
                    if node.width == 0.0 { node.width = w; }
                    if node.height == 0.0 { node.height = h; }
                }
            }
            set_board.set(loaded_board);
        });
    });

    // File watcher listener (Tauri only)
    // Note: Backend handles skipping emissions for our own saves
    Effect::new(move || {
        if !is_tauri() {
            return; // Skip file watching in browser mode
        }

        let handler = Closure::new(move |_event: JsValue| {
            // Only external changes reach here (backend skips our own saves)
            web_sys::console::log_1(&"External board change detected, reloading...".into());
            spawn_local(async move {
                let mut loaded_board = load_board_storage().await;
                for node in &mut loaded_board.nodes {
                    if node.width == 0.0 || node.height == 0.0 {
                        let (w, h) = Node::auto_size(&node.text);
                        if node.width == 0.0 { node.width = w; }
                        if node.height == 0.0 { node.height = h; }
                    }
                }
                set_board.set(loaded_board);
            });
        });

        spawn_local(async move {
            let _ = listen("board-changed", &handler).await;
            handler.forget();
        });
    });

    // Image loading effect
    Effect::new({
        let image_cache = image_cache_for_load.clone();
        move || {
            let current_board = board.get();

            for node in &current_board.nodes {
                if node.node_type == "image" && !node.text.is_empty() {
                    let url = node.text.clone();

                    let needs_load = {
                        let cache = image_cache.borrow();
                        !cache.contains_key(&url)
                    };

                    if needs_load {
                        // Mark as loading
                        web_sys::console::log_1(&format!("Loading image: {}", url).into());
                        image_cache.borrow_mut().insert(url.clone(), None);

                        let cache_for_async = image_cache.clone();
                        let url_for_async = url.clone();
                        let trigger = set_image_load_trigger;

                        spawn_local(async move {
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

                            // Create image element and load
                            let img = HtmlImageElement::new().unwrap();
                            let url_for_closure = url_for_async.clone();
                            let cache_for_onload = cache_for_async.clone();

                            let onload_ref = Closure::wrap(Box::new({
                                let img = img.clone();
                                let cache = cache_for_onload.clone();
                                let url = url_for_closure.clone();
                                move || {
                                    web_sys::console::log_1(&format!("Image loaded successfully: {}", url).into());
                                    cache.borrow_mut().insert(url.clone(), Some(img.clone()));
                                    trigger.update(|n| *n = n.wrapping_add(1));
                                }
                            }) as Box<dyn Fn()>);

                            img.set_onload(Some(onload_ref.as_ref().unchecked_ref()));
                            onload_ref.forget();

                            let onerror = Closure::wrap(Box::new({
                                let url = url_for_async.clone();
                                move || {
                                    web_sys::console::error_1(&format!("Image load FAILED: {}", url).into());
                                }
                            }) as Box<dyn Fn()>);

                            img.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                            onerror.forget();

                            img.set_src(&image_src);
                        });
                    }
                }
            }
        }
    });

    // Link preview fetching effect
    Effect::new({
        let link_cache = link_preview_cache_for_fetch.clone();
        let image_cache = image_cache_for_link_preview.clone();
        move || {
            let current_board = board.get();

            for node in &current_board.nodes {
                if node.node_type == "link" && !node.text.is_empty() {
                    let url = node.text.clone();

                    let needs_fetch = {
                        let cache = link_cache.borrow();
                        !cache.contains_key(&url)
                    };

                    if needs_fetch {
                        // Mark as loading
                        link_cache.borrow_mut().insert(url.clone(), None);

                        let cache_for_result = link_cache.clone();
                        let image_cache_for_result = image_cache.clone();
                        let trigger = set_link_preview_trigger;
                        let img_trigger = set_image_load_trigger;

                        spawn_local(async move {
                            let args = serde_wasm_bindgen::to_value(&FetchLinkPreviewArgs { url: url.clone() }).unwrap();
                            let result = invoke("fetch_link_preview", args).await;

                            if let Ok(preview) = serde_wasm_bindgen::from_value::<LinkPreview>(result) {
                                // If preview has an image, start loading it
                                if let Some(ref image_url) = preview.image {
                                    let img_url = image_url.clone();
                                    let needs_img_load = {
                                        let cache = image_cache_for_result.borrow();
                                        !cache.contains_key(&img_url)
                                    };

                                    if needs_img_load {
                                        image_cache_for_result.borrow_mut().insert(img_url.clone(), None);

                                        let img = HtmlImageElement::new().unwrap();
                                        let cache_for_onload = image_cache_for_result.clone();
                                        let url_for_closure = img_url.clone();

                                        let onload = Closure::wrap(Box::new({
                                            let img = img.clone();
                                            let cache = cache_for_onload.clone();
                                            let url = url_for_closure.clone();
                                            move || {
                                                cache.borrow_mut().insert(url.clone(), Some(img.clone()));
                                                img_trigger.update(|n| *n = n.wrapping_add(1));
                                            }
                                        }) as Box<dyn Fn()>);

                                        img.set_onload(Some(onload.as_ref().unchecked_ref()));
                                        onload.forget();
                                        img.set_src(&img_url);
                                    }
                                }

                                cache_for_result.borrow_mut().insert(url, Some(preview));
                                trigger.update(|n| *n = n.wrapping_add(1));
                            }
                        });
                    }
                }
            }
        }
    });

    // Markdown file fetching effect (for local .md files in link nodes)
    Effect::new(move || {
        let current_board = board.get();
        let current_cache = md_file_cache.get();

        for node in &current_board.nodes {
            if node.node_type == "link" && is_local_md_file(&node.text) {
                let path = node.text.clone();

                if !current_cache.contains_key(&path) {
                    // Mark as loading
                    set_md_file_cache.update(|c| {
                        c.insert(path.clone(), None);
                    });

                    spawn_local(async move {
                        let args = serde_wasm_bindgen::to_value(&ReadMarkdownFileArgs { path: path.clone() }).unwrap();
                        let result = invoke("read_markdown_file", args).await;

                        let content = result.as_string();
                        set_md_file_cache.update(|c| {
                            c.insert(path, content);
                        });
                    });
                }
            }
        }
    });

    Effect::new(move || {
        let current_board = board.get();
        let current_camera = camera.get();
        let current_selected = selected_nodes.get();
        let current_selected_edge = selected_edge.get();
        let current_editing = editing_node.get();
        let current_edge_creation = edge_creation.get();
        let current_selection_box = selection_box.get();
        let _ = image_load_trigger.get(); // Subscribe to image loads
        let _ = link_preview_trigger.get(); // Subscribe to link preview loads

        if let Some(canvas) = canvas_ref.get() {
            let canvas_el: &HtmlCanvasElement = &canvas;

            let rect = canvas_el.get_bounding_client_rect();
            let display_width = rect.width() as u32;
            let display_height = rect.height() as u32;

            if canvas_el.width() != display_width {
                canvas_el.set_width(display_width);
            }
            if canvas_el.height() != display_height {
                canvas_el.set_height(display_height);
            }

            if let Ok(ctx) = get_canvas_context(canvas_el) {
                render_board(
                    &ctx,
                    canvas_el,
                    &current_board,
                    &current_camera,
                    &current_selected,
                    current_selected_edge.as_ref(),
                    current_editing.as_ref(),
                    current_edge_creation.is_creating.then_some({
                        (
                            current_edge_creation.from_node_id.as_ref(),
                            current_edge_creation.current_x,
                            current_edge_creation.current_y,
                        )
                    }),
                    current_selection_box,
                    &image_cache_for_render,
                    &link_preview_cache_for_render,
                );
            }
        }
    });

    let on_mouse_down = {
        let history = history_for_mouse_down.clone();
        move |ev: web_sys::MouseEvent| {
        if editing_node.get_untracked().is_some() {
            return;
        }

        let canvas = canvas_ref.get().unwrap();
        let _ = canvas.focus();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let cam = camera.get_untracked();
        let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);

        let current_board = board.get_untracked();
        let current_selected = selected_nodes.get_untracked();
        let handle_size = RESIZE_HANDLE_SIZE / cam.zoom;

        // First check if clicking on a resize handle of any selected node
        // (handles extend outside node bounds, so check before contains_point)
        let resize_hit = current_board.nodes.iter()
            .filter(|n| current_selected.contains(&n.id))
            .find_map(|n| n.resize_handle_at(world_x, world_y, handle_size).map(|h| (n, h)));

        if let Some((node, handle)) = resize_hit {
            // Record history before resize starts
            history.borrow_mut().push(board.get_untracked());
            // Start resize operation
            set_resize_state.set(ResizeState {
                is_resizing: true,
                node_id: Some(node.id.clone()),
                handle: Some(handle),
                start_mouse_x: world_x,
                start_mouse_y: world_y,
                original_x: node.x,
                original_y: node.y,
                original_width: node.width,
                original_height: node.height,
            });
            return;
        }

        let clicked_node = current_board
            .nodes
            .iter()
            .rev()
            .find(|n| n.contains_point(world_x, world_y));

        if let Some(node) = clicked_node {
            set_selected_edge.set(None);
            if ev.shift_key() {
                set_edge_creation.set(EdgeCreationState {
                    is_creating: true,
                    from_node_id: Some(node.id.clone()),
                    current_x: canvas_x,
                    current_y: canvas_y,
                });
            } else {
                if ev.meta_key() || ev.ctrl_key() {
                    set_selected_nodes.update(|s| {
                        if !s.remove(&node.id) {
                            s.insert(node.id.clone());
                        }
                    });
                } else if !current_selected.contains(&node.id) {
                    set_selected_nodes.set([node.id.clone()].into_iter().collect());
                }

                // Copy link URL to clipboard when clicking a link node
                if node.node_type == "link" && !node.text.is_empty() {
                    let url = node.text.clone();
                    spawn_local(async move {
                        if let Some(window) = web_sys::window() {
                            let clipboard = window.navigator().clipboard();
                            let _ = wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&url)).await;
                        }
                    });
                }

                let selected = selected_nodes.get_untracked();
                let mut start_positions = HashMap::new();
                for n in &current_board.nodes {
                    if selected.contains(&n.id) {
                        start_positions.insert(n.id.clone(), (n.x, n.y));
                    }
                }
                if start_positions.is_empty() {
                    start_positions.insert(node.id.clone(), (node.x, node.y));
                    set_selected_nodes.set([node.id.clone()].into_iter().collect());
                }

                // Record history before drag starts
                history.borrow_mut().push(board.get_untracked());
                set_drag_state.set(DragState {
                    is_dragging: true,
                    is_box_selecting: false,
                    start_x: canvas_x,
                    start_y: canvas_y,
                    node_start_positions: start_positions,
                });
            }
        } else {
            let clicked_edge = current_board.edges.iter().find(|edge| {
                let from = current_board.nodes.iter().find(|n| n.id == edge.from_node);
                let to = current_board.nodes.iter().find(|n| n.id == edge.to_node);
                if let (Some(from), Some(to)) = (from, to) {
                    let from_cx = from.x + from.width / 2.0;
                    let from_cy = from.y + from.height / 2.0;
                    let to_cx = to.x + to.width / 2.0;
                    let to_cy = to.y + to.height / 2.0;
                    point_near_line(world_x, world_y, from_cx, from_cy, to_cx, to_cy, 10.0 / cam.zoom)
                } else {
                    false
                }
            });

            if let Some(edge) = clicked_edge {
                set_selected_nodes.set(HashSet::new());
                set_selected_edge.set(Some(edge.id.clone()));
            } else {
                set_selected_edge.set(None);
                if !ev.shift_key() && !ev.meta_key() && !ev.ctrl_key() {
                    set_selected_nodes.set(HashSet::new());
                }
                if ev.ctrl_key() || ev.meta_key() {
                    set_drag_state.set(DragState {
                        is_dragging: false,
                        is_box_selecting: true,
                        start_x: canvas_x,
                        start_y: canvas_y,
                        node_start_positions: HashMap::new(),
                    });
                } else {
                    set_pan_state.set(PanState {
                        is_panning: true,
                        start_x: canvas_x,
                        start_y: canvas_y,
                        camera_start_x: cam.x,
                        camera_start_y: cam.y,
                    });
                }
            }
        }
    }};

    let on_mouse_move = move |ev: web_sys::MouseEvent| {
        let canvas = canvas_ref.get().unwrap();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let current_drag = drag_state.get_untracked();
        let current_pan = pan_state.get_untracked();
        let edge_state = edge_creation.get_untracked();
        let current_resize = resize_state.get_untracked();

        if current_resize.is_resizing {
            let cam = camera.get_untracked();
            let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);
            let dx = world_x - current_resize.start_mouse_x;
            let dy = world_y - current_resize.start_mouse_y;

            set_board.update(|b| {
                if let Some(node_id) = &current_resize.node_id {
                    if let Some(node) = b.nodes.iter_mut().find(|n| &n.id == node_id) {
                        match current_resize.handle {
                            Some(ResizeHandle::TopLeft) => {
                                let new_width = (current_resize.original_width - dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height - dy).max(MIN_NODE_HEIGHT);
                                let actual_dx = current_resize.original_width - new_width;
                                let actual_dy = current_resize.original_height - new_height;
                                node.x = current_resize.original_x + actual_dx;
                                node.y = current_resize.original_y + actual_dy;
                                node.width = new_width;
                                node.height = new_height;
                            }
                            Some(ResizeHandle::TopRight) => {
                                let new_width = (current_resize.original_width + dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height - dy).max(MIN_NODE_HEIGHT);
                                let actual_dy = current_resize.original_height - new_height;
                                node.y = current_resize.original_y + actual_dy;
                                node.width = new_width;
                                node.height = new_height;
                            }
                            Some(ResizeHandle::BottomLeft) => {
                                let new_width = (current_resize.original_width - dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height + dy).max(MIN_NODE_HEIGHT);
                                let actual_dx = current_resize.original_width - new_width;
                                node.x = current_resize.original_x + actual_dx;
                                node.width = new_width;
                                node.height = new_height;
                            }
                            Some(ResizeHandle::BottomRight) => {
                                let new_width = (current_resize.original_width + dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height + dy).max(MIN_NODE_HEIGHT);
                                node.width = new_width;
                                node.height = new_height;
                            }
                            None => {}
                        }
                    }
                }
            });
        } else if edge_state.is_creating {
            set_edge_creation.update(|s| {
                s.current_x = canvas_x;
                s.current_y = canvas_y;
            });
        } else if current_drag.is_dragging {
            let cam = camera.get_untracked();
            let dx = (canvas_x - current_drag.start_x) / cam.zoom;
            let dy = (canvas_y - current_drag.start_y) / cam.zoom;

            set_board.update(|b| {
                for (id, (start_x, start_y)) in &current_drag.node_start_positions {
                    if let Some(node) = b.nodes.iter_mut().find(|n| &n.id == id) {
                        node.x = start_x + dx;
                        node.y = start_y + dy;
                    }
                }
            });
        } else if current_drag.is_box_selecting {
            let cam = camera.get_untracked();
            let (start_wx, start_wy) = cam.screen_to_world(current_drag.start_x, current_drag.start_y);
            let (end_wx, end_wy) = cam.screen_to_world(canvas_x, canvas_y);
            set_selection_box.set(Some((
                start_wx.min(end_wx),
                start_wy.min(end_wy),
                start_wx.max(end_wx),
                start_wy.max(end_wy),
            )));
        } else if current_pan.is_panning {
            let cam = camera.get_untracked();
            let dx = (canvas_x - current_pan.start_x) / cam.zoom;
            let dy = (canvas_y - current_pan.start_y) / cam.zoom;

            set_camera.update(|c| {
                c.x = current_pan.camera_start_x - dx;
                c.y = current_pan.camera_start_y - dy;
            });
        } else {
            // Update cursor based on what we're hovering over
            let cam = camera.get_untracked();
            let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);
            let current_selected = selected_nodes.get_untracked();
            let current_board = board.get_untracked();
            let handle_size = RESIZE_HANDLE_SIZE / cam.zoom;

            // Track mouse position for paste operations
            set_last_mouse_world_pos.set((world_x, world_y));

            let mut new_cursor = "crosshair";

            // Check if over a resize handle on a selected node
            for node in current_board.nodes.iter().rev() {
                if current_selected.contains(&node.id) {
                    if let Some(handle) = node.resize_handle_at(world_x, world_y, handle_size) {
                        new_cursor = match handle {
                            ResizeHandle::TopLeft | ResizeHandle::BottomRight => "nwse-resize",
                            ResizeHandle::TopRight | ResizeHandle::BottomLeft => "nesw-resize",
                        };
                        break;
                    }
                }
                if node.contains_point(world_x, world_y) {
                    new_cursor = "move";
                    break;
                }
            }

            set_cursor_style.set(new_cursor.to_string());
        }
    };

    let on_mouse_up = {
        let history = history_for_mouse_up.clone();
        move |ev: web_sys::MouseEvent| {
        let was_dragging = drag_state.get_untracked().is_dragging;
        let was_resizing = resize_state.get_untracked().is_resizing;
        let current_drag = drag_state.get_untracked();
        let edge_state = edge_creation.get_untracked();

        if was_resizing {
            set_resize_state.set(ResizeState::default());

            let current_board = board.get_untracked();
            spawn_local(async move {
                save_board_storage(&current_board).await;
            });
            return;
        }

        if edge_state.is_creating {
            if let Some(from_id) = &edge_state.from_node_id {
                let canvas = canvas_ref.get().unwrap();
                let rect = canvas.get_bounding_client_rect();
                let canvas_x = ev.client_x() as f64 - rect.left();
                let canvas_y = ev.client_y() as f64 - rect.top();
                let cam = camera.get_untracked();
                let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);

                let current_board = board.get_untracked();
                if let Some(target) = current_board.nodes.iter().rev().find(|n| n.contains_point(world_x, world_y)) {
                    if &target.id != from_id {
                        // Record history before edge creation
                        history.borrow_mut().push(board.get_untracked());
                        set_board.update(|b| {
                            b.edges.push(Edge {
                                id: uuid::Uuid::new_v4().to_string(),
                                from_node: from_id.clone(),
                                to_node: target.id.clone(),
                                label: None,
                            });
                        });

                        let current_board = board.get_untracked();
                        spawn_local(async move {
                            save_board_storage(&current_board).await;
                        });
                    }
                }
            }
            set_edge_creation.set(EdgeCreationState::default());
            return;
        }

        if current_drag.is_box_selecting {
            if let Some((min_x, min_y, max_x, max_y)) = selection_box.get_untracked() {
                let current_board = board.get_untracked();
                let nodes_in_box: HashSet<String> = current_board
                    .nodes
                    .iter()
                    .filter(|n| intersects_box(n, min_x, min_y, max_x, max_y))
                    .map(|n| n.id.clone())
                    .collect();

                if ev.shift_key() {
                    set_selected_nodes.update(|s| s.extend(nodes_in_box));
                } else {
                    set_selected_nodes.set(nodes_in_box);
                }
            }
            set_selection_box.set(None);
        }

        set_drag_state.set(DragState::default());
        set_pan_state.set(PanState::default());

        if was_dragging {
            let current_board = board.get_untracked();
            spawn_local(async move {
                save_board_storage(&current_board).await;
            });
        }
    }};

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();

        let canvas = canvas_ref.get().unwrap();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let zoom_factor = if ev.delta_y() < 0.0 { 1.1 } else { 0.9 };

        set_camera.update(|c| {
            let (world_x, world_y) = c.screen_to_world(canvas_x, canvas_y);

            c.zoom = (c.zoom * zoom_factor).clamp(0.1, 5.0);

            c.x = world_x - canvas_x / c.zoom;
            c.y = world_y - canvas_y / c.zoom;
        });
    };

    let on_double_click = {
        let history = history_for_double_click.clone();
        let image_cache_for_modal = image_cache_for_modal.clone();
        move |ev: web_sys::MouseEvent| {
            let canvas = canvas_ref.get().unwrap();
            let rect = canvas.get_bounding_client_rect();
            let canvas_x = ev.client_x() as f64 - rect.left();
            let canvas_y = ev.client_y() as f64 - rect.top();

            let cam = camera.get_untracked();
            let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);

            let current_board = board.get_untracked();
            let clicked_node = current_board
                .nodes
                .iter()
                .rev()
                .find(|n| n.contains_point(world_x, world_y));

            if let Some(node) = clicked_node {
                if node.node_type == "image" {
                    // Open image in modal - get src from cached HtmlImageElement
                    let cache = image_cache_for_modal.borrow();
                    if let Some(Some(img)) = cache.get(&node.text) {
                        set_modal_image.set(Some(img.src()));
                    }
                } else if node.node_type == "md" {
                    // Open MD in modal (view mode)
                    set_modal_md.set(Some((node.id.clone(), false)));
                } else if node.node_type == "link" && is_local_md_file(&node.text) {
                    // Open local .md file in modal (view mode)
                    set_modal_md.set(Some((node.id.clone(), false)));
                } else if node.node_type == "link" {
                    // Open regular link in browser
                    if let Some(window) = web_sys::window() {
                        let _ = window.open_with_url_and_target(&node.text, "_blank");
                    }
                } else {
                    // Edit mode for text, idea, note nodes
                    set_editing_node.set(Some(node.id.clone()));
                }
            } else {
                let new_node = Node::new(
                    uuid::Uuid::new_v4().to_string(),
                    world_x - 100.0,
                    world_y - 50.0,
                    "New Node".to_string(),
                );
                let new_id = new_node.id.clone();

                // Record history before node creation
                history.borrow_mut().push(board.get_untracked());
                set_board.update(|b| {
                    b.nodes.push(new_node);
                });
                set_selected_nodes.set([new_id.clone()].into_iter().collect());
                set_editing_node.set(Some(new_id));

                let current_board = board.get_untracked();
                spawn_local(async move {
                    save_board_storage(&current_board).await;
                });
            }
        }
    };

    let on_keydown = {
        let history = history_for_keydown.clone();
        move |ev: web_sys::KeyboardEvent| {
        if editing_node.get_untracked().is_some() {
            return;
        }

        let key = ev.key();
        let selected = selected_nodes.get_untracked();
        let edge_sel = selected_edge.get_untracked();

        match key.as_str() {
            "z" if ev.meta_key() || ev.ctrl_key() => {
                ev.prevent_default();
                if ev.shift_key() {
                    // Redo: Ctrl+Shift+Z / Cmd+Shift+Z
                    if let Some(new_board) = history.borrow_mut().redo(board.get_untracked()) {
                        set_board.set(new_board.clone());
                        set_selected_nodes.set(HashSet::new());
                        set_selected_edge.set(None);
                        spawn_local(async move {
                            save_board_storage(&new_board).await;
                        });
                    }
                } else {
                    // Undo: Ctrl+Z / Cmd+Z
                    if let Some(new_board) = history.borrow_mut().undo(board.get_untracked()) {
                        set_board.set(new_board.clone());
                        set_selected_nodes.set(HashSet::new());
                        set_selected_edge.set(None);
                        spawn_local(async move {
                            save_board_storage(&new_board).await;
                        });
                    }
                }
            }
            "Backspace" | "Delete" => {
                if let Some(edge_id) = edge_sel {
                    // Record history before edge deletion
                    history.borrow_mut().push(board.get_untracked());
                    set_board.update(|b| {
                        b.edges.retain(|e| e.id != edge_id);
                    });
                    set_selected_edge.set(None);

                    let current_board = board.get_untracked();
                    spawn_local(async move {
                        save_board_storage(&current_board).await;
                    });
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

                    // Record history before node deletion
                    history.borrow_mut().push(board.get_untracked());
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
            "c" if ev.meta_key() || ev.ctrl_key() => {
                if !selected.is_empty() {
                    let current_board = board.get_untracked();
                    let copied_nodes: Vec<Node> = current_board.nodes.iter()
                        .filter(|n| selected.contains(&n.id))
                        .cloned()
                        .collect();
                    let copied_edges: Vec<Edge> = current_board.edges.iter()
                        .filter(|e| selected.contains(&e.from_node) && selected.contains(&e.to_node))
                        .cloned()
                        .collect();
                    set_node_clipboard.set(Some((copied_nodes, copied_edges)));
                }
            }
            "v" if ev.meta_key() || ev.ctrl_key() => {
                if let Some((ref nodes, ref edges)) = node_clipboard.get_untracked() {
                    if !nodes.is_empty() {
                        ev.prevent_default();

                        // Calculate center of copied nodes
                        let cx = nodes.iter().map(|n| n.x + n.width / 2.0).sum::<f64>() / nodes.len() as f64;
                        let cy = nodes.iter().map(|n| n.y + n.height / 2.0).sum::<f64>() / nodes.len() as f64;
                        let (mouse_x, mouse_y) = last_mouse_world_pos.get_untracked();

                        // Build old_id -> new_id mapping
                        let id_map: HashMap<String, String> = nodes.iter()
                            .map(|n| (n.id.clone(), uuid::Uuid::new_v4().to_string()))
                            .collect();

                        let new_nodes: Vec<Node> = nodes.iter().map(|n| {
                            Node {
                                id: id_map[&n.id].clone(),
                                x: n.x - cx + mouse_x,
                                y: n.y - cy + mouse_y,
                                ..n.clone()
                            }
                        }).collect();

                        let new_edges: Vec<Edge> = edges.iter().map(|e| {
                            Edge {
                                id: uuid::Uuid::new_v4().to_string(),
                                from_node: id_map[&e.from_node].clone(),
                                to_node: id_map[&e.to_node].clone(),
                                label: e.label.clone(),
                            }
                        }).collect();

                        let new_ids: HashSet<String> = new_nodes.iter().map(|n| n.id.clone()).collect();

                        history.borrow_mut().push(board.get_untracked());
                        set_board.update(|b| {
                            b.nodes.extend(new_nodes);
                            b.edges.extend(new_edges);
                        });
                        set_selected_nodes.set(new_ids);

                        let current_board = board.get_untracked();
                        spawn_local(async move {
                            save_board_storage(&current_board).await;
                        });
                    }
                }
                // If no internal clipboard, let ClipboardEvent fire for image paste
            }
            "t" | "T" => {
                if !selected.is_empty() {
                    // Record history before type change
                    history.borrow_mut().push(board.get_untracked());
                    set_board.update(|b| {
                        for node in &mut b.nodes {
                            if selected.contains(&node.id) {
                                node.node_type = cycle_node_type(&node.node_type);
                            }
                        }
                    });

                    let current_board = board.get_untracked();
                    spawn_local(async move {
                        save_board_storage(&current_board).await;
                    });
                }
            }
            "Escape" => {
                set_selected_nodes.set(HashSet::new());
                set_selected_edge.set(None);
                set_editing_node.set(None);
                set_edge_creation.set(EdgeCreationState::default());
                set_selection_box.set(None);
                set_modal_image.set(None);
                set_modal_md.set(None);
            }
            _ => {}
        }
    }};

    let on_paste = {
        let history = history_for_paste.clone();
        move |ev: web_sys::ClipboardEvent| {
        // If internal node clipboard was used, keydown already handled it
        if node_clipboard.get_untracked().as_ref().is_some_and(|(n, _)| !n.is_empty()) {
            return;
        }

        ev.prevent_default();

        if !is_tauri() {
            return; // Image paste only works in Tauri mode
        }

        let (world_x, world_y) = last_mouse_world_pos.get_untracked();
        let history = history.clone();

        spawn_local(async move {
            let result = invoke("paste_image", JsValue::NULL).await;

            // Debug: log the raw result
            web_sys::console::log_2(&"paste_image result:".into(), &result);

            match serde_wasm_bindgen::from_value::<PasteImageResult>(result.clone()) {
                Ok(paste_result) => {
                    web_sys::console::log_1(&format!("Paste success: path={}, {}x{}", paste_result.path, paste_result.width, paste_result.height).into());

                    let node_width = (paste_result.width as f64).min(400.0).max(100.0);
                    let node_height = (paste_result.height as f64).min(400.0).max(100.0);

                    let new_node = Node {
                        id: uuid::Uuid::new_v4().to_string(),
                        x: world_x - node_width / 2.0,
                        y: world_y - node_height / 2.0,
                        width: node_width,
                        height: node_height,
                        text: paste_result.path,
                        node_type: "image".to_string(),
                        color: None,
                        tags: Vec::new(),
                        status: None,
                        group: None,
                        priority: None,
                    };
                    let new_id = new_node.id.clone();

                    // Record history before image paste
                    history.borrow_mut().push(board.get_untracked());
                    set_board.update(|b| {
                        b.nodes.push(new_node);
                    });
                    set_selected_nodes.set([new_id].into_iter().collect());

                    let current_board = board.get_untracked();
                    save_board_storage(&current_board).await;
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Paste failed: {:?}", e).into());
                }
            }
        });
    }};

    let on_upload = move |_ev: web_sys::MouseEvent| {
        if let Some(input) = file_input_ref.get() {
            let el: &web_sys::HtmlElement = &input;
            el.click();
        }
    };

    let on_file_selected = move |_ev: web_sys::Event| {
        let input = file_input_ref.get().unwrap();
        let input_el: &web_sys::HtmlInputElement = (*input).unchecked_ref();
        let files = input_el.files().unwrap();
        if files.length() == 0 {
            return;
        }
        let file = files.get(0).unwrap();
        let reader = web_sys::FileReader::new().unwrap();
        let reader_clone = reader.clone();

        let onload = Closure::wrap(Box::new(move || {
            if let Ok(result) = reader_clone.result() {
                if let Some(text) = result.as_string() {
                    if let Ok(parsed) = serde_json::from_str::<Board>(&text) {
                        set_board.set(parsed.clone());
                        spawn_local(async move {
                            save_board_storage(&parsed).await;
                        });
                    }
                }
            }
        }) as Box<dyn Fn()>);

        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
        onload.forget();
        let _ = reader.read_as_text(&file);

        // Reset input so re-uploading same file triggers change
        input_el.set_value("");
    };

    let on_download = move |_ev: web_sys::MouseEvent| {
        let current_board = board.get_untracked();
        let json = serde_json::to_string_pretty(&current_board).unwrap_or_default();

        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();

        let array = js_sys::Array::new();
        array.push(&JsValue::from_str(&json));
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type("application/json");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&array, &opts).unwrap();

        let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
        let a: web_sys::HtmlAnchorElement = document
            .create_element("a")
            .unwrap()
            .unchecked_into();
        a.set_href(&url);
        a.set_download("board.json");
        a.click();
        let _ = web_sys::Url::revoke_object_url(&url);
    };

    let button_style = "background: #0a0a0a; color: #66cc88; border: 1px solid #2a4a3a; \
        padding: 6px 14px; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
        font-size: 12px; cursor: pointer; border-radius: 4px;";

    view! {
        <div style="width: 100vw; height: 100vh; overflow: hidden; background: #020202; position: relative;">
            <canvas
                node_ref=canvas_ref
                tabindex="0"
                style=move || format!("width: 100%; height: 100%; display: block; cursor: {}; outline: none;", cursor_style.get())
                on:mousedown=on_mouse_down
                on:mousemove=on_mouse_move
                on:mouseup=on_mouse_up.clone()
                on:mouseleave=on_mouse_up
                on:wheel=on_wheel
                on:dblclick=on_double_click
                on:keydown=on_keydown
                on:paste=on_paste
            />
            <NodeEditor/>
            <MarkdownOverlays/>
            <ImageModal/>
            <MarkdownModal/>
            <Show when=move || !is_tauri()>
                <div style="position: fixed; top: 12px; right: 12px; display: flex; gap: 8px; z-index: 100;">
                    <button style=button_style on:click=on_upload>"Upload board.json"</button>
                    <button style=button_style on:click=on_download>"Download board.json"</button>
                </div>
                <input type="file" accept=".json" node_ref=file_input_ref style="display:none"
                       on:change=on_file_selected />
            </Show>
            <div style="position: fixed; bottom: 12px; left: 12px; color: #66cc88; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; font-size: 11px; letter-spacing: 0.5px;">
                "[DBLCLK] add/edit  [DRAG corner] resize  [SHIFT+DRAG] connect  [CMD+DRAG] box  [CMD+C] copy  [CMD+V] paste  [T] type  [DEL] delete  [CMD+Z] undo  [CMD+SHIFT+Z] redo"
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod is_local_md_file_tests {
        use super::*;

        #[test]
        fn absolute_path() {
            assert!(is_local_md_file("/Users/me/vault/note.md"));
            assert!(is_local_md_file("/path/to/file.md"));
        }

        #[test]
        fn file_url() {
            assert!(is_local_md_file("file:///Users/me/vault/note.md"));
            assert!(is_local_md_file("file:///path/to/file.md"));
        }

        #[test]
        fn file_url_with_encoded_spaces() {
            assert!(is_local_md_file("file:///Users/me/Obsidian%20Vault/note.md"));
        }

        #[test]
        fn home_relative_path() {
            assert!(is_local_md_file("~/Documents/note.md"));
            assert!(is_local_md_file("~/vault/subfolder/note.md"));
        }

        #[test]
        fn case_insensitive_extension() {
            assert!(is_local_md_file("/path/to/file.MD"));
            assert!(is_local_md_file("/path/to/file.Md"));
            assert!(is_local_md_file("~/note.MD"));
        }

        #[test]
        fn rejects_http_urls() {
            assert!(!is_local_md_file("http://example.com/file.md"));
            assert!(!is_local_md_file("https://example.com/file.md"));
        }

        #[test]
        fn rejects_non_md_files() {
            assert!(!is_local_md_file("/path/to/file.txt"));
            assert!(!is_local_md_file("/path/to/file.pdf"));
            assert!(!is_local_md_file("~/document.docx"));
            assert!(!is_local_md_file("file:///path/to/image.png"));
        }

        #[test]
        fn rejects_relative_paths() {
            assert!(!is_local_md_file("./note.md"));
            assert!(!is_local_md_file("../note.md"));
            assert!(!is_local_md_file("note.md"));
        }

        #[test]
        fn rejects_empty_string() {
            assert!(!is_local_md_file(""));
        }

        #[test]
        fn handles_md_in_path_but_wrong_extension() {
            assert!(!is_local_md_file("/path/to/markdown/file.txt"));
            assert!(!is_local_md_file("~/Documents/md-files/note.pdf"));
        }
    }

    mod cycle_node_type_tests {
        use super::*;

        #[test]
        fn cycles_through_all_types() {
            assert_eq!(cycle_node_type("text"), "idea");
            assert_eq!(cycle_node_type("idea"), "note");
            assert_eq!(cycle_node_type("note"), "image");
            assert_eq!(cycle_node_type("image"), "md");
            assert_eq!(cycle_node_type("md"), "link");
            assert_eq!(cycle_node_type("link"), "text");
        }

        #[test]
        fn unknown_type_wraps_to_text() {
            assert_eq!(cycle_node_type("unknown"), "text");
            assert_eq!(cycle_node_type(""), "text");
        }
    }

    mod intersects_box_tests {
        use super::*;
        use crate::state::Node;

        fn node_at(x: f64, y: f64, w: f64, h: f64) -> Node {
            Node { x, y, width: w, height: h, ..Node::new("t".into(), x, y, String::new()) }
        }

        #[test]
        fn fully_inside() {
            assert!(intersects_box(&node_at(10.0, 10.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_right() {
            assert!(!intersects_box(&node_at(200.0, 10.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_left() {
            assert!(!intersects_box(&node_at(-50.0, 10.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_above() {
            assert!(!intersects_box(&node_at(10.0, -50.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_below() {
            assert!(!intersects_box(&node_at(10.0, 200.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn partially_overlapping() {
            assert!(intersects_box(&node_at(90.0, 90.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn touching_edge() {
            assert!(intersects_box(&node_at(100.0, 0.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }
    }

    mod point_near_line_tests {
        use super::*;

        #[test]
        fn point_on_line() {
            assert!(point_near_line(5.0, 5.0, 0.0, 0.0, 10.0, 10.0, 1.0));
        }

        #[test]
        fn point_far_from_line() {
            assert!(!point_near_line(50.0, 50.0, 0.0, 0.0, 10.0, 0.0, 5.0));
        }

        #[test]
        fn point_near_midpoint() {
            assert!(point_near_line(5.0, 1.0, 0.0, 0.0, 10.0, 0.0, 2.0));
        }

        #[test]
        fn point_near_endpoint() {
            assert!(point_near_line(0.5, 0.0, 0.0, 0.0, 10.0, 0.0, 1.0));
        }

        #[test]
        fn point_beyond_segment_end() {
            assert!(!point_near_line(15.0, 0.0, 0.0, 0.0, 10.0, 0.0, 1.0));
        }

        #[test]
        fn degenerate_zero_length_line() {
            assert!(point_near_line(0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0));
            assert!(!point_near_line(5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0));
        }
    }

    mod parse_markdown_tests {
        use super::*;

        #[test]
        fn renders_heading() {
            let html = parse_markdown("# Hello");
            assert!(html.contains("<h1>Hello</h1>"));
        }

        #[test]
        fn renders_bold() {
            let html = parse_markdown("**bold**");
            assert!(html.contains("<strong>bold</strong>"));
        }

        #[test]
        fn renders_list() {
            let html = parse_markdown("- item 1\n- item 2");
            assert!(html.contains("<li>item 1</li>"));
            assert!(html.contains("<li>item 2</li>"));
        }

        #[test]
        fn empty_input() {
            assert_eq!(parse_markdown(""), "");
        }
    }
}
