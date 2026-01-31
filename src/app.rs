use crate::canvas::{get_canvas_context, render_board};
use crate::state::{Board, Camera, Node};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
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

#[derive(Clone)]
struct DragState {
    is_dragging: bool,
    node_id: Option<String>,
    start_x: f64,
    start_y: f64,
    node_start_x: f64,
    node_start_y: f64,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            is_dragging: false,
            node_id: None,
            start_x: 0.0,
            start_y: 0.0,
            node_start_x: 0.0,
            node_start_y: 0.0,
        }
    }
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

#[component]
pub fn App() -> impl IntoView {
    let (board, set_board) = signal(Board::default());
    let (camera, set_camera) = signal(Camera::new());
    let (selected_node, set_selected_node) = signal::<Option<String>>(None);
    let (drag_state, set_drag_state) = signal(DragState::default());
    let (pan_state, set_pan_state) = signal(PanState::default());
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
        let current_selected = selected_node.get();

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
                render_board(&ctx, canvas_el, &current_board, &current_camera, current_selected.as_ref());
            }
        }
    });

    let on_mouse_down = move |ev: web_sys::MouseEvent| {
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
            set_selected_node.set(Some(node.id.clone()));
            set_drag_state.set(DragState {
                is_dragging: true,
                node_id: Some(node.id.clone()),
                start_x: canvas_x,
                start_y: canvas_y,
                node_start_x: node.x,
                node_start_y: node.y,
            });
        } else {
            set_selected_node.set(None);
            set_pan_state.set(PanState {
                is_panning: true,
                start_x: canvas_x,
                start_y: canvas_y,
                camera_start_x: cam.x,
                camera_start_y: cam.y,
            });
        }
    };

    let on_mouse_move = move |ev: web_sys::MouseEvent| {
        let canvas = canvas_ref.get().unwrap();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let current_drag = drag_state.get_untracked();
        let current_pan = pan_state.get_untracked();

        if current_drag.is_dragging {
            if let Some(node_id) = &current_drag.node_id {
                let cam = camera.get_untracked();
                let dx = (canvas_x - current_drag.start_x) / cam.zoom;
                let dy = (canvas_y - current_drag.start_y) / cam.zoom;

                set_board.update(|b| {
                    if let Some(node) = b.nodes.iter_mut().find(|n| &n.id == node_id) {
                        node.x = current_drag.node_start_x + dx;
                        node.y = current_drag.node_start_y + dy;
                    }
                });
            }
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

    let on_mouse_up = move |_ev: web_sys::MouseEvent| {
        let was_dragging = drag_state.get_untracked().is_dragging;

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
        set_selected_node.set(Some(new_id));

        let current_board = board.get_untracked();
        spawn_local(async move {
            let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: current_board }).unwrap();
            let _ = invoke("save_board", args).await;
        });
    };

    view! {
        <div style="width: 100vw; height: 100vh; overflow: hidden; background: #1a1a2e;">
            <canvas
                node_ref=canvas_ref
                style="width: 100%; height: 100%; display: block; cursor: grab;"
                on:mousedown=on_mouse_down
                on:mousemove=on_mouse_move
                on:mouseup=on_mouse_up
                on:mouseleave=on_mouse_up
                on:wheel=on_wheel
                on:dblclick=on_double_click
            />
            <div style="position: fixed; bottom: 20px; left: 20px; color: #6a6a9a; font-family: sans-serif; font-size: 12px;">
                "Double-click to add nodes | Drag nodes to move | Scroll to zoom | Drag canvas to pan"
            </div>
        </div>
    }
}
