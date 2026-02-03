use crate::canvas::{get_canvas_context, render_board, ImageCache, LinkPreviewCache};
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

// Flag to skip file watcher reload after our own saves
thread_local! {
    static SKIP_NEXT_RELOAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

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

async fn save_board_storage(board: &Board) {
    if is_tauri() {
        // Set flag to skip the file watcher reload triggered by our own save
        SKIP_NEXT_RELOAD.with(|flag| flag.set(true));
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

#[derive(Serialize, Deserialize)]
struct FetchLinkPreviewArgs {
    url: String,
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

fn parse_markdown(md: &str) -> String {
    let parser = Parser::new(md);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

fn convert_path_to_asset_url(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!("asset://localhost{}", path)
    }
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
    let (selection_box, set_selection_box) = signal::<Option<(f64, f64, f64, f64)>>(None);
    let (modal_image, set_modal_image) = signal::<Option<String>>(None);
    let (modal_md, set_modal_md) = signal::<Option<(String, bool)>>(None); // (node_id, is_editing)
    let (md_edit_text, set_md_edit_text) = signal::<String>(String::new()); // Separate signal to avoid re-render on typing
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let image_cache: ImageCache = Rc::new(RefCell::new(HashMap::new()));
    let image_cache_for_render = image_cache.clone();
    let image_cache_for_load = image_cache.clone();
    let image_cache_for_link_preview = image_cache.clone();
    let link_preview_cache: LinkPreviewCache = Rc::new(RefCell::new(HashMap::new()));
    let link_preview_cache_for_render = link_preview_cache.clone();
    let link_preview_cache_for_fetch = link_preview_cache.clone();
    let (image_load_trigger, set_image_load_trigger) = signal(0u32);
    let (link_preview_trigger, set_link_preview_trigger) = signal(0u32);

    // Load board on startup (with small delay to ensure Tauri is ready)
    Effect::new(move || {
        spawn_local(async move {
            // Small delay to ensure Tauri's __TAURI__ is injected
            gloo_timers::future::TimeoutFuture::new(50).await;
            let loaded_board = load_board_storage().await;
            set_board.set(loaded_board);
        });
    });

    // File watcher listener (Tauri only)
    Effect::new(move || {
        if !is_tauri() {
            return; // Skip file watching in browser mode
        }

        let handler = Closure::new(move |_event: JsValue| {
            // Skip reload if this was triggered by our own save
            let should_skip = SKIP_NEXT_RELOAD.with(|flag| {
                if flag.get() {
                    flag.set(false);
                    true
                } else {
                    false
                }
            });

            if should_skip {
                return;
            }

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

    // Image loading effect
    Effect::new({
        let image_cache = image_cache_for_load.clone();
        move || {
            let current_board = board.get();

            for node in &current_board.nodes {
                if node.node_type == "image" && !node.text.is_empty() {
                    let url = node.text.clone();
                    let asset_url = convert_path_to_asset_url(&url);

                    let needs_load = {
                        let cache = image_cache.borrow();
                        !cache.contains_key(&url)
                    };

                    if needs_load {
                        // Mark as loading
                        image_cache.borrow_mut().insert(url.clone(), None);

                        let img = HtmlImageElement::new().unwrap();
                        let url_for_closure = url.clone();
                        let cache_for_onload = image_cache.clone();
                        let trigger = set_image_load_trigger;

                        let onload_ref = Closure::wrap(Box::new({
                            let img = img.clone();
                            let cache = cache_for_onload.clone();
                            let url = url_for_closure.clone();
                            move || {
                                cache.borrow_mut().insert(url.clone(), Some(img.clone()));
                                trigger.update(|n| *n = n.wrapping_add(1));
                            }
                        }) as Box<dyn Fn()>);

                        img.set_onload(Some(onload_ref.as_ref().unchecked_ref()));
                        onload_ref.forget();

                        let onerror = Closure::wrap(Box::new({
                            let cache = image_cache.clone();
                            let url = url.clone();
                            move || {
                                // Keep it as None to show loading failed
                                cache.borrow_mut().insert(url.clone(), None);
                            }
                        }) as Box<dyn Fn()>);

                        img.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                        onerror.forget();

                        img.set_src(&asset_url);
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

    let on_mouse_down = move |ev: web_sys::MouseEvent| {
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
                let current_selected = selected_nodes.get_untracked();

                // Check for resize handle on selected nodes
                let handle_size = RESIZE_HANDLE_SIZE / cam.zoom;
                let resize_handle = if current_selected.contains(&node.id) {
                    node.resize_handle_at(world_x, world_y, handle_size)
                } else {
                    None
                };

                if let Some(handle) = resize_handle {
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

                    set_drag_state.set(DragState {
                        is_dragging: true,
                        is_box_selecting: false,
                        start_x: canvas_x,
                        start_y: canvas_y,
                        node_start_positions: start_positions,
                    });
                }
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
    };

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

    let on_mouse_up = move |ev: web_sys::MouseEvent| {
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
                        set_board.update(|b| {
                            b.edges.push(Edge {
                                id: uuid::Uuid::new_v4().to_string(),
                                from_node: from_id.clone(),
                                to_node: target.id.clone(),
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
    };

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

    let on_double_click = move |ev: web_sys::MouseEvent| {
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
                // Open image in modal
                let url = convert_path_to_asset_url(&node.text);
                set_modal_image.set(Some(url));
            } else if node.node_type == "md" {
                // Open MD in modal (view mode)
                set_modal_md.set(Some((node.id.clone(), false)));
            } else if node.node_type == "link" {
                // Open link in browser
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
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if editing_node.get_untracked().is_some() {
            return;
        }

        let key = ev.key();
        let selected = selected_nodes.get_untracked();
        let edge_sel = selected_edge.get_untracked();

        match key.as_str() {
            "Backspace" | "Delete" => {
                if let Some(edge_id) = edge_sel {
                    set_board.update(|b| {
                        b.edges.retain(|e| e.id != edge_id);
                    });
                    set_selected_edge.set(None);

                    let current_board = board.get_untracked();
                    spawn_local(async move {
                        save_board_storage(&current_board).await;
                    });
                } else if !selected.is_empty() {
                    set_board.update(|b| {
                        b.nodes.retain(|n| !selected.contains(&n.id));
                        b.edges.retain(|e| !selected.contains(&e.from_node) && !selected.contains(&e.to_node));
                    });
                    set_selected_nodes.set(HashSet::new());

                    let current_board = board.get_untracked();
                    spawn_local(async move {
                        save_board_storage(&current_board).await;
                    });
                }
            }
            "t" | "T" => {
                if !selected.is_empty() {
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
    };

    let editing_node_view = move || {
        if let Some(node_id) = editing_node.get() {
            let b = board.get();
            let cam = camera.get();
            if let Some(node) = b.nodes.iter().find(|n| n.id == node_id) {
                let (screen_x, screen_y) = cam.world_to_screen(node.x, node.y);
                let screen_w = node.width * cam.zoom;
                let screen_h = node.height * cam.zoom;
                let font_size = (14.0 * cam.zoom).max(8.0);
                let initial_text = node.text.clone();
                let is_md = node.node_type == "md";

                if is_md {
                    // MD nodes use textarea for multi-line editing
                    let node_id_for_blur = node_id.clone();
                    let on_blur_textarea = move |ev: web_sys::FocusEvent| {
                        if let Some(target) = ev.target() {
                            if let Ok(textarea) = target.dyn_into::<web_sys::HtmlTextAreaElement>() {
                                let new_text = textarea.value();
                                let node_id_clone = node_id_for_blur.clone();
                                set_board.update(|b| {
                                    if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                        node.text = new_text;
                                    }
                                });

                                let current_board = board.get_untracked();
                                spawn_local(async move {
                                    save_board_storage(&current_board).await;
                                });
                            }
                        }
                        set_editing_node.set(None);
                    };

                    let node_id_for_keydown = node_id.clone();
                    let on_keydown_textarea = move |ev: web_sys::KeyboardEvent| {
                        if ev.key().as_str() == "Escape" {
                            if let Some(target) = ev.target() {
                                if let Ok(textarea) = target.dyn_into::<web_sys::HtmlTextAreaElement>() {
                                    let new_text = textarea.value();
                                    let node_id_clone = node_id_for_keydown.clone();
                                    set_board.update(|b| {
                                        if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                            node.text = new_text;
                                        }
                                    });

                                    let current_board = board.get_untracked();
                                    spawn_local(async move {
                                        save_board_storage(&current_board).await;
                                    });
                                }
                            }
                            set_editing_node.set(None);
                        }
                    };

                    return Some(view! {
                        <textarea
                            autofocus=true
                            style=format!(
                                "position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; \
                                 font-size: {}px; background: #020202; resize: none; \
                                 color: #ccffdd; border: 1px solid #aaffbb; outline: none; \
                                 box-sizing: border-box; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                                 text-shadow: 0 0 6px #aaffbb; padding: 8px;",
                                screen_x, screen_y, screen_w, screen_h, font_size
                            )
                            on:blur=on_blur_textarea
                            on:keydown=on_keydown_textarea
                        >{initial_text}</textarea>
                    }.into_any());
                } else {
                    // Regular nodes use input
                    let node_id_for_blur = node_id.clone();
                    let on_blur = move |ev: web_sys::FocusEvent| {
                        if let Some(target) = ev.target() {
                            if let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() {
                                let new_text = input.value();
                                let node_id_clone = node_id_for_blur.clone();
                                set_board.update(|b| {
                                    if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                        node.text = new_text;
                                    }
                                });

                                let current_board = board.get_untracked();
                                spawn_local(async move {
                                    save_board_storage(&current_board).await;
                                });
                            }
                        }
                        set_editing_node.set(None);
                    };

                    let node_id_for_keydown = node_id.clone();
                    let on_keydown = move |ev: web_sys::KeyboardEvent| {
                        match ev.key().as_str() {
                            "Enter" => {
                                if let Some(target) = ev.target() {
                                    if let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() {
                                        let new_text = input.value();
                                        let node_id_clone = node_id_for_keydown.clone();
                                        set_board.update(|b| {
                                            if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                                node.text = new_text;
                                            }
                                        });

                                        let current_board = board.get_untracked();
                                        spawn_local(async move {
                                            save_board_storage(&current_board).await;
                                        });
                                        set_editing_node.set(None);
                                    }
                                }
                            }
                            "Escape" => {
                                set_editing_node.set(None);
                            }
                            _ => {}
                        }
                    };

                    return Some(view! {
                        <input
                            type="text"
                            value=initial_text
                            autofocus=true
                            style=format!(
                                "position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; \
                                 font-size: {}px; text-align: center; background: #020202; \
                                 color: #ccffdd; border: 1px solid #aaffbb; outline: none; \
                                 box-sizing: border-box; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                                 text-shadow: 0 0 6px #aaffbb;",
                                screen_x, screen_y, screen_w, screen_h, font_size
                            )
                            on:blur=on_blur
                            on:keydown=on_keydown
                        />
                    }.into_any());
                }
            }
        }
        None
    };

    let md_overlays_view = move || {
        let b = board.get();
        let cam = camera.get();
        let current_editing = editing_node.get();

        b.nodes
            .iter()
            .filter(|n| n.node_type == "md" && current_editing.as_ref() != Some(&n.id))
            .map(|node| {
                let (screen_x, screen_y) = cam.world_to_screen(node.x, node.y);
                let label_height = 16.0 * cam.zoom;
                let html_content = parse_markdown(&node.text);

                // Use transform to scale content uniformly
                // Container is sized at 1x zoom, transform scales it
                let base_w = node.width;
                let base_h = node.height - 16.0; // Account for label
                let base_padding = 8.0;

                view! {
                    <div
                        style=format!(
                            "position: absolute; left: {}px; top: {}px; \
                             width: {}px; height: {}px; overflow: hidden; \
                             transform: scale({}); transform-origin: top left; \
                             padding: {}px; box-sizing: border-box; \
                             color: #ccffdd; font-size: 12px; line-height: 1.4; \
                             font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                             pointer-events: none;",
                            screen_x, screen_y + label_height,
                            base_w, base_h,
                            cam.zoom,
                            base_padding
                        )
                        inner_html=html_content
                    />
                }
            })
            .collect::<Vec<_>>()
    };

    let modal_view = move || {
        modal_image.get().map(|image_url| view! {
                <div
                    style="position: fixed; inset: 0; background: rgba(0,0,0,0.9); \
                           display: flex; align-items: center; justify-content: center; \
                           z-index: 1000; cursor: pointer;"
                    on:click=move |_| set_modal_image.set(None)
                >
                    <img
                        src=image_url
                        style="max-width: 90vw; max-height: 90vh; object-fit: contain; \
                               border: 1px solid #44dd66; box-shadow: 0 0 30px rgba(68, 221, 102, 0.3);"
                    />
                </div>
            })
    };

    let md_modal_view = move || {
        if let Some((node_id, is_editing)) = modal_md.get() {
            let node_id_for_edit = node_id.clone();
            let node_id_for_save = node_id.clone();
            let node_id_for_content = node_id.clone();

            Some(view! {
                <div
                    style="position: fixed; inset: 0; background: rgba(0,0,0,0.9); \
                           display: flex; align-items: center; justify-content: center; \
                           z-index: 1000;"
                    on:click=move |_| set_modal_md.set(None)
                >
                    <div
                        style="width: 90vw; max-width: 800px; height: 80vh; \
                               background: #020202; border: 1px solid #44dd66; \
                               box-shadow: 0 0 30px rgba(68, 221, 102, 0.3); \
                               padding: 24px; display: flex; flex-direction: column; \
                               font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                               color: #ccffdd; font-size: 14px; line-height: 1.6;"
                        on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()
                    >
                        <div
                            style="margin-bottom: 16px; padding-bottom: 16px; \
                                   border-bottom: 1px solid #44dd66; \
                                   display: flex; justify-content: flex-end; gap: 8px;"
                        >
                            {move || {
                                let node_id = node_id_for_edit.clone();
                                let node_id_save = node_id_for_save.clone();
                                if is_editing {
                                    view! {
                                        <button
                                            style="background: transparent; color: #66cc88; border: 1px solid #66cc88; \
                                                   padding: 8px 16px; cursor: pointer; \
                                                   font-family: inherit; font-size: 12px;"
                                            on:click=move |_| {
                                                // Cancel - revert to view mode
                                                set_modal_md.set(Some((node_id.clone(), false)));
                                            }
                                        >
                                            "Cancel"
                                        </button>
                                        <button
                                            style="background: #44dd66; color: #020202; border: none; \
                                                   padding: 8px 16px; cursor: pointer; \
                                                   font-family: inherit; font-size: 12px; font-weight: bold;"
                                            on:click=move |_| {
                                                // Save - get content from edit signal and update board
                                                let new_content = md_edit_text.get_untracked();
                                                let nid = node_id_save.clone();
                                                set_board.update(|b| {
                                                    if let Some(node) = b.nodes.iter_mut().find(|n| n.id == nid) {
                                                        node.text = new_content;
                                                    }
                                                });

                                                let current_board = board.get_untracked();
                                                spawn_local(async move {
                                                    save_board_storage(&current_board).await;
                                                });

                                                // Switch back to view mode
                                                set_modal_md.set(Some((node_id_save.clone(), false)));
                                            }
                                        >
                                            "Save"
                                        </button>
                                    }.into_any()
                                } else {
                                    view! {
                                        <button
                                            style="background: #44dd66; color: #020202; border: none; \
                                                   padding: 8px 16px; cursor: pointer; \
                                                   font-family: inherit; font-size: 12px; font-weight: bold;"
                                            on:click=move |_| {
                                                // Enter edit mode - populate edit text from current board content
                                                let b = board.get_untracked();
                                                if let Some((id, _)) = modal_md.get_untracked() {
                                                    if let Some(n) = b.nodes.iter().find(|n| n.id == id) {
                                                        set_md_edit_text.set(n.text.clone());
                                                    }
                                                    set_modal_md.set(Some((id, true)));
                                                }
                                            }
                                        >
                                            "Edit"
                                        </button>
                                    }.into_any()
                                }
                            }}
                        </div>
                        <div style="flex: 1; overflow-y: auto; min-height: 0;">
                            {move || {
                                let nid = node_id_for_content.clone();
                                if is_editing {
                                    // Use md_edit_text signal for textarea - updates don't re-render modal
                                    view! {
                                        <textarea
                                            style="width: 100%; height: 100%; background: #020202; \
                                                   color: #ccffdd; border: 1px solid #33aa55; \
                                                   font-family: inherit; font-size: 14px; \
                                                   padding: 12px; box-sizing: border-box; resize: none; \
                                                   outline: none;"
                                            prop:value=move || md_edit_text.get()
                                            on:input=move |ev| {
                                                let value = event_target_value(&ev);
                                                set_md_edit_text.set(value);
                                            }
                                        />
                                    }.into_any()
                                } else {
                                    // View mode - get content from board
                                    let b = board.get();
                                    let content = b.nodes.iter()
                                        .find(|n| n.id == nid)
                                        .map(|n| n.text.clone())
                                        .unwrap_or_default();
                                    let html_content = parse_markdown(&content);
                                    view! {
                                        <div inner_html=html_content />
                                    }.into_any()
                                }
                            }}
                        </div>
                    </div>
                </div>
            })
        } else {
            None
        }
    };

    view! {
        <div style="width: 100vw; height: 100vh; overflow: hidden; background: #020202; position: relative;">
            <canvas
                node_ref=canvas_ref
                tabindex="0"
                style=move || format!("width: 100%; height: 100%; display: block; cursor: {}; outline: none;", cursor_style.get())
                on:mousedown=on_mouse_down
                on:mousemove=on_mouse_move
                on:mouseup=on_mouse_up
                on:mouseleave=on_mouse_up
                on:wheel=on_wheel
                on:dblclick=on_double_click
                on:keydown=on_keydown
            />
            {editing_node_view}
            {md_overlays_view}
            {modal_view}
            {md_modal_view}
            <div style="position: fixed; bottom: 12px; left: 12px; color: #66cc88; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; font-size: 11px; letter-spacing: 0.5px;">
                "[DBLCLK] add/edit  [DRAG corner] resize  [SHIFT+DRAG] connect  [CMD+DRAG] box  [T] type  [DEL] delete"
            </div>
        </div>
    }
}
