use crate::state::{Board, Camera, Node};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

pub fn render_board(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    board: &Board,
    camera: &Camera,
    selected_node: Option<&String>,
) {
    let width = canvas.width() as f64;
    let height = canvas.height() as f64;

    ctx.set_fill_style_str("#1a1a2e");
    ctx.fill_rect(0.0, 0.0, width, height);

    draw_grid(ctx, camera, width, height);

    for edge in &board.edges {
        draw_edge(ctx, board, edge, camera);
    }

    for node in &board.nodes {
        let is_selected = selected_node.map_or(false, |id| id == &node.id);
        draw_node(ctx, node, camera, is_selected);
    }
}

fn draw_grid(ctx: &CanvasRenderingContext2d, camera: &Camera, width: f64, height: f64) {
    let grid_size = 50.0 * camera.zoom;
    if grid_size < 10.0 {
        return;
    }

    ctx.set_stroke_style_str("#2a2a4e");
    ctx.set_line_width(1.0);

    let offset_x = (camera.x * camera.zoom) % grid_size;
    let offset_y = (camera.y * camera.zoom) % grid_size;

    let mut x = -offset_x;
    while x < width {
        ctx.begin_path();
        ctx.move_to(x, 0.0);
        ctx.line_to(x, height);
        ctx.stroke();
        x += grid_size;
    }

    let mut y = -offset_y;
    while y < height {
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(width, y);
        ctx.stroke();
        y += grid_size;
    }
}

fn draw_node(ctx: &CanvasRenderingContext2d, node: &Node, camera: &Camera, is_selected: bool) {
    let (screen_x, screen_y) = camera.world_to_screen(node.x, node.y);
    let screen_width = node.width * camera.zoom;
    let screen_height = node.height * camera.zoom;

    let bg_color = match node.node_type.as_str() {
        "idea" => "#4a4a8a",
        "note" => "#8a4a4a",
        _ => "#3a3a5a",
    };
    ctx.set_fill_style_str(bg_color);

    let radius = 8.0 * camera.zoom;
    draw_rounded_rect(ctx, screen_x, screen_y, screen_width, screen_height, radius);
    ctx.fill();

    if is_selected {
        ctx.set_stroke_style_str("#00ff88");
        ctx.set_line_width(3.0);
    } else {
        ctx.set_stroke_style_str("#6a6a9a");
        ctx.set_line_width(1.0);
    }
    draw_rounded_rect(ctx, screen_x, screen_y, screen_width, screen_height, radius);
    ctx.stroke();

    ctx.set_fill_style_str("#ffffff");
    let font_size = (14.0 * camera.zoom).max(8.0);
    ctx.set_font(&format!("{}px sans-serif", font_size));
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");

    let text_x = screen_x + screen_width / 2.0;
    let text_y = screen_y + screen_height / 2.0;

    let max_width = screen_width - 20.0 * camera.zoom;
    let _ = ctx.fill_text_with_max_width(&node.text, text_x, text_y, max_width);
}

fn draw_rounded_rect(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    ctx.begin_path();
    ctx.move_to(x + radius, y);
    ctx.line_to(x + width - radius, y);
    ctx.arc_to(x + width, y, x + width, y + radius, radius)
        .unwrap();
    ctx.line_to(x + width, y + height - radius);
    ctx.arc_to(x + width, y + height, x + width - radius, y + height, radius)
        .unwrap();
    ctx.line_to(x + radius, y + height);
    ctx.arc_to(x, y + height, x, y + height - radius, radius)
        .unwrap();
    ctx.line_to(x, y + radius);
    ctx.arc_to(x, y, x + radius, y, radius).unwrap();
    ctx.close_path();
}

fn draw_edge(ctx: &CanvasRenderingContext2d, board: &Board, edge: &crate::state::Edge, camera: &Camera) {
    let from_node = board.nodes.iter().find(|n| n.id == edge.from_node);
    let to_node = board.nodes.iter().find(|n| n.id == edge.to_node);

    if let (Some(from), Some(to)) = (from_node, to_node) {
        let from_center_x = from.x + from.width / 2.0;
        let from_center_y = from.y + from.height / 2.0;
        let to_center_x = to.x + to.width / 2.0;
        let to_center_y = to.y + to.height / 2.0;

        let (from_screen_x, from_screen_y) = camera.world_to_screen(from_center_x, from_center_y);
        let (to_screen_x, to_screen_y) = camera.world_to_screen(to_center_x, to_center_y);

        ctx.set_stroke_style_str("#6a6a9a");
        ctx.set_line_width(2.0);
        ctx.begin_path();
        ctx.move_to(from_screen_x, from_screen_y);
        ctx.line_to(to_screen_x, to_screen_y);
        ctx.stroke();
    }
}

pub fn get_canvas_context(
    canvas: &HtmlCanvasElement,
) -> Result<CanvasRenderingContext2d, JsValue> {
    Ok(canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("Failed to get 2d context"))?
        .dyn_into::<CanvasRenderingContext2d>()?)
}
