use crate::app::{node_matches_query, BoardDataCtx, SelectionCtx};
use crate::state::Camera;
use leptos::prelude::*;
use std::collections::HashSet;
use wasm_bindgen::JsCast;

/// Center the live canvas viewport on a world-space point, preserving the
/// current zoom. Returns the repositioned camera, or `None` if the canvas
/// element can't be measured (so the caller leaves the camera untouched).
fn center_camera_on(cam: &Camera, wx: f64, wy: f64) -> Option<Camera> {
    let canvas = web_sys::window()?
        .document()?
        .query_selector("canvas")
        .ok()
        .flatten()?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;
    let rect = canvas.get_bounding_client_rect();
    let (cw, ch) = (rect.width(), rect.height());
    let zoom = if cam.zoom.is_finite() && cam.zoom > 0.0 { cam.zoom } else { 1.0 };
    Some(Camera {
        x: wx - (cw / zoom) / 2.0,
        y: wy - (ch / zoom) / 2.0,
        zoom,
    })
}

/// Cmd/Ctrl+F search overlay (P2.4 / F99).
///
/// While `search_query` is `Some`, renders a floating input. On every keystroke
/// it filters the board by text/tags/status via [`node_matches_query`] and writes
/// the matching node ids into `selected_nodes` so they render with the existing
/// selection highlight. Enter recenters the camera on the first match (board
/// order); Escape closes the overlay and clears the highlight.
#[component]
pub fn SearchOverlay() -> impl IntoView {
    let board_ctx = use_context::<BoardDataCtx>().unwrap();
    let sel_ctx = use_context::<SelectionCtx>().unwrap();

    // Recompute matches for `query`, push them into the selection highlight, and
    // return the ids in board order (so "first match" is deterministic).
    let apply_matches = move |query: &str| -> Vec<String> {
        let board = board_ctx.board.get_untracked();
        let ids: Vec<String> = board
            .nodes
            .iter()
            .filter(|n| node_matches_query(n, query))
            .map(|n| n.id.clone())
            .collect();
        let set: HashSet<String> = ids.iter().cloned().collect();
        sel_ctx.set_selected_nodes.set(set);
        ids
    };

    let on_input = move |ev: web_sys::Event| {
        if let Some(target) = ev.target() {
            if let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() {
                let q = input.value();
                apply_matches(&q);
                sel_ctx.set_search_query.set(Some(q));
            }
        }
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        match ev.key().as_str() {
            "Enter" => {
                ev.prevent_default();
                let query = sel_ctx.search_query.get_untracked().unwrap_or_default();
                let ids = apply_matches(&query);
                if let Some(first_id) = ids.first() {
                    let board = board_ctx.board.get_untracked();
                    if let Some(node) = board.nodes.iter().find(|n| &n.id == first_id) {
                        let (wx, wy) = node.center();
                        let cam = board_ctx.camera.get_untracked();
                        if let Some(next) = center_camera_on(&cam, wx, wy) {
                            board_ctx.set_camera.set(next);
                        }
                    }
                }
            }
            "Escape" => {
                ev.prevent_default();
                sel_ctx.set_selected_nodes.set(HashSet::new());
                sel_ctx.set_search_query.set(None);
            }
            _ => {}
        }
    };

    move || {
        sel_ctx.search_query.get().map(|query| {
            view! {
                <div style="position: fixed; top: 16px; left: 50%; transform: translateX(-50%); \
                            z-index: 250; background: #051005; border: 1px solid #2a4a3a; \
                            border-radius: 6px; padding: 8px 10px; display: flex; align-items: center; \
                            gap: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.5);">
                    <span style="color: #66cc88; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                                 font-size: 12px;">"search"</span>
                    <input
                        type="text"
                        value=query
                        autofocus=true
                        placeholder="text, tag, or status…"
                        style="background: #020202; color: #ccffdd; border: 1px solid #2a4a3a; \
                               border-radius: 4px; outline: none; padding: 6px 10px; width: 280px; \
                               font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; font-size: 13px;"
                        on:input=on_input
                        on:keydown=on_keydown
                    />
                </div>
            }
        })
    }
}
