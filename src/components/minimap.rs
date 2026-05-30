use crate::app::{minimap_transform, nodes_bounding_box, BoardDataCtx};
use crate::state::Camera;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

/// Minimap CSS dimensions and inset, in CSS pixels.
const MINIMAP_W: f64 = 200.0;
const MINIMAP_H: f64 = 140.0;
const MINIMAP_PAD: f64 = 8.0;

/// A small overview canvas pinned to the bottom-right. It draws every node as a
/// scaled rectangle plus a rectangle marking the current viewport, and recenters
/// the main camera when clicked (F101). Hidden when the board is empty.
///
/// It renders on its own `requestAnimationFrame`-free effect (the board is small
/// enough that a direct draw per change is cheap) and reuses the pure
/// [`minimap_transform`] / [`nodes_bounding_box`] helpers so its mapping math is
/// unit-tested without a DOM.
#[component]
pub fn Minimap() -> impl IntoView {
    let ctx = use_context::<BoardDataCtx>().unwrap();
    let board = ctx.board;
    let camera = ctx.camera;
    let set_camera = ctx.set_camera;
    let viewport_size = ctx.viewport_size;

    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();

    // Redraw whenever the board, camera, or main viewport size changes.
    Effect::new(move || {
        let current_board = board.get();
        let cam = camera.get();
        let (vw, vh) = viewport_size.get();

        let Some(canvas) = canvas_ref.get() else {
            return;
        };
        let canvas_el: &HtmlCanvasElement = &canvas;

        // Size the backing store to the fixed CSS dimensions (1:1; the minimap is
        // small and detail is not the point, so we skip DPR scaling here).
        let bw = MINIMAP_W as u32;
        let bh = MINIMAP_H as u32;
        if canvas_el.width() != bw {
            canvas_el.set_width(bw);
        }
        if canvas_el.height() != bh {
            canvas_el.set_height(bh);
        }

        let Ok(Some(ctx_obj)) = canvas_el.get_context("2d") else {
            return;
        };
        let Ok(c) = ctx_obj.dyn_into::<CanvasRenderingContext2d>() else {
            return;
        };

        // Background.
        c.set_fill_style_str("rgba(4, 8, 4, 0.92)");
        c.fill_rect(0.0, 0.0, MINIMAP_W, MINIMAP_H);

        let Some(bbox) = nodes_bounding_box(&current_board.nodes) else {
            return;
        };
        let (scale, off_x, off_y) = minimap_transform(bbox, MINIMAP_W, MINIMAP_H, MINIMAP_PAD);

        // Node rectangles.
        c.set_fill_style_str("rgba(102, 204, 136, 0.55)");
        for n in &current_board.nodes {
            let x = n.x * scale + off_x;
            let y = n.y * scale + off_y;
            let w = (n.width * scale).max(1.0);
            let h = (n.height * scale).max(1.0);
            c.fill_rect(x, y, w, h);
        }

        // Viewport rectangle: the visible world region is
        // (camera.x, camera.y) .. + (vw/zoom, vh/zoom).
        if vw > 0.0 && vh > 0.0 && cam.zoom > 0.0 {
            let vx = cam.x * scale + off_x;
            let vy = cam.y * scale + off_y;
            let vrw = (vw / cam.zoom) * scale;
            let vrh = (vh / cam.zoom) * scale;
            c.set_stroke_style_str("rgba(150, 255, 190, 0.95)");
            c.set_line_width(1.5);
            c.stroke_rect(vx, vy, vrw, vrh);
        }
    });

    // Click-to-recenter: translate the click position back into world coords and
    // move the camera so that world point sits at the viewport center.
    let on_click = move |ev: web_sys::MouseEvent| {
        let Some(canvas) = canvas_ref.get_untracked() else {
            return;
        };
        let rect = canvas.get_bounding_client_rect();
        let mx = ev.client_x() as f64 - rect.left();
        let my = ev.client_y() as f64 - rect.top();

        let current_board = board.get_untracked();
        let Some(bbox) = nodes_bounding_box(&current_board.nodes) else {
            return;
        };
        let (scale, off_x, off_y) = minimap_transform(bbox, MINIMAP_W, MINIMAP_H, MINIMAP_PAD);
        if scale <= 0.0 {
            return;
        }
        // Inverse of the forward mapping `world * scale + off`.
        let world_x = (mx - off_x) / scale;
        let world_y = (my - off_y) / scale;

        let (vw, vh) = viewport_size.get_untracked();
        set_camera.update(|c: &mut Camera| {
            let half_w = if c.zoom > 0.0 { (vw / c.zoom) / 2.0 } else { 0.0 };
            let half_h = if c.zoom > 0.0 { (vh / c.zoom) / 2.0 } else { 0.0 };
            c.x = world_x - half_w;
            c.y = world_y - half_h;
        });
    };

    let container_style = format!(
        "position: fixed; bottom: 40px; right: 12px; width: {}px; height: {}px; \
         z-index: 90; border: 1px solid #2a4a3a; border-radius: 4px; \
         box-shadow: 0 2px 12px rgba(0,0,0,0.5); overflow: hidden;",
        MINIMAP_W, MINIMAP_H
    );

    // Hide the minimap entirely when there is nothing to overview.
    move || {
        if board.with(|b| b.nodes.is_empty()) {
            None
        } else {
            Some(view! {
                <div style=container_style.clone()>
                    <canvas
                        node_ref=canvas_ref
                        style="width: 100%; height: 100%; display: block; cursor: pointer;"
                        on:click=on_click.clone()
                    />
                </div>
            })
        }
    }
}
