use crate::canvas::{get_canvas_context, render_board};
use crate::state::{Board, Camera, Edge, Node};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &Closure<dyn Fn(JsValue)>) -> JsValue;
}

#[derive(Serialize, Deserialize)]
struct SaveBoardArgs {
    board: Board,
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

fn cycle_node_type(current: &str) -> String {
    match current {
        "text" => "idea".to_string(),
        "idea" => "note".to_string(),
        _ => "text".to_string(),
    }
}

fn intersects_box(node: &Node, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
    let node_right = node.x + node.width;
    let node_bottom = node.y + node.height;
    !(node.x > max_x || node_right < min_x || node.y > max_y || node_bottom < min_y)
}

#[component]
pub fn App() -> impl IntoView {
    let (board, set_board) = signal(Board::default());
    let (camera, set_camera) = signal(Camera::new());
    let (selected_nodes, set_selected_nodes) = signal::<HashSet<String>>(HashSet::new());
    let (drag_state, set_drag_state) = signal(DragState::default());
    let (pan_state, set_pan_state) = signal(PanState::default());
    let (editing_node, set_editing_node) = signal::<Option<String>>(None);
    let (edge_creation, set_edge_creation) = signal(EdgeCreationState::default());
    let (selection_box, set_selection_box) = signal::<Option<(f64, f64, f64, f64)>>(None);
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();

    Effect::new(move || {
        spawn_local(async move {
            let result = invoke("load_board", JsValue::NULL).await;
            if let Ok(loaded_board) = serde_wasm_bindgen::from_value::<Board>(result) {
                set_board.set(loaded_board);
            }
        });
    });

    Effect::new(move || {
        let handler = Closure::new(move |_event: JsValue| {
            spawn_local(async move {
                let result = invoke("load_board", JsValue::NULL).await;
                if let Ok(loaded_board) = serde_wasm_bindgen::from_value::<Board>(result) {
                    set_board.set(loaded_board);
                }
            });
        });

        spawn_local(async move {
            let _ = listen("board-changed", &handler).await;
            handler.forget();
        });
    });

    Effect::new(move || {
        let current_board = board.get();
        let current_camera = camera.get();
        let current_selected = selected_nodes.get();
        let current_editing = editing_node.get();
        let current_edge_creation = edge_creation.get();
        let current_selection_box = selection_box.get();

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
                    current_editing.as_ref(),
                    current_edge_creation.is_creating.then(|| {
                        (
                            current_edge_creation.from_node_id.as_ref(),
                            current_edge_creation.current_x,
                            current_edge_creation.current_y,
                        )
                    }),
                    current_selection_box,
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
            if ev.shift_key() {
                set_edge_creation.set(EdgeCreationState {
                    is_creating: true,
                    from_node_id: Some(node.id.clone()),
                    current_x: canvas_x,
                    current_y: canvas_y,
                });
            } else {
                let current_selected = selected_nodes.get_untracked();
                if ev.meta_key() || ev.ctrl_key() {
                    set_selected_nodes.update(|s| {
                        if !s.remove(&node.id) {
                            s.insert(node.id.clone());
                        }
                    });
                } else if !current_selected.contains(&node.id) {
                    set_selected_nodes.set([node.id.clone()].into_iter().collect());
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
        } else {
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
    };

    let on_mouse_move = move |ev: web_sys::MouseEvent| {
        let canvas = canvas_ref.get().unwrap();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let current_drag = drag_state.get_untracked();
        let current_pan = pan_state.get_untracked();
        let edge_state = edge_creation.get_untracked();

        if edge_state.is_creating {
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
        }
    };

    let on_mouse_up = move |ev: web_sys::MouseEvent| {
        let was_dragging = drag_state.get_untracked().is_dragging;
        let current_drag = drag_state.get_untracked();
        let edge_state = edge_creation.get_untracked();

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
                            let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
                            let _ = invoke("save_board", args).await;
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
                let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
                let _ = invoke("save_board", args).await;
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
            set_editing_node.set(Some(node.id.clone()));
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
                let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
                let _ = invoke("save_board", args).await;
            });
        }
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if editing_node.get_untracked().is_some() {
            return;
        }

        let key = ev.key();
        let selected = selected_nodes.get_untracked();

        match key.as_str() {
            "Backspace" | "Delete" => {
                if !selected.is_empty() {
                    set_board.update(|b| {
                        b.nodes.retain(|n| !selected.contains(&n.id));
                        b.edges.retain(|e| !selected.contains(&e.from_node) && !selected.contains(&e.to_node));
                    });
                    set_selected_nodes.set(HashSet::new());

                    let current_board = board.get_untracked();
                    spawn_local(async move {
                        let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
                        let _ = invoke("save_board", args).await;
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
                        let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
                        let _ = invoke("save_board", args).await;
                    });
                }
            }
            "Escape" => {
                set_selected_nodes.set(HashSet::new());
                set_editing_node.set(None);
                set_edge_creation.set(EdgeCreationState::default());
                set_selection_box.set(None);
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
                                let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
                                let _ = invoke("save_board", args).await;
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
                                        let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
                                        let _ = invoke("save_board", args).await;
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
                             font-size: {}px; text-align: center; background: rgba(0,0,0,0.5); \
                             color: white; border: 2px solid #00ff88; outline: none; border-radius: 8px; \
                             box-sizing: border-box;",
                            screen_x, screen_y, screen_w, screen_h, font_size
                        )
                        on:blur=on_blur
                        on:keydown=on_keydown
                    />
                });
            }
        }
        None
    };

    view! {
        <div style="width: 100vw; height: 100vh; overflow: hidden; background: #1a1a2e; position: relative;">
            <canvas
                node_ref=canvas_ref
                tabindex="0"
                style="width: 100%; height: 100%; display: block; cursor: grab; outline: none;"
                on:mousedown=on_mouse_down
                on:mousemove=on_mouse_move
                on:mouseup=on_mouse_up
                on:mouseleave=on_mouse_up
                on:wheel=on_wheel
                on:dblclick=on_double_click
                on:keydown=on_keydown
            />
            {editing_node_view}
            <div style="position: fixed; bottom: 20px; left: 20px; color: #6a6a9a; font-family: sans-serif; font-size: 12px;">
                "Dbl-click: add/edit | Shift+drag: connect | Ctrl/Cmd+drag: box select | T: cycle type | Del: delete | Esc: deselect"
            </div>
        </div>
    }
}
