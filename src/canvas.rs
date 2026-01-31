use crate::state::{Board, Camera, Node};
use std::collections::HashSet;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

const BG_COLOR: &str = "#020202";
const GRID_COLOR: &str = "#0a1a0a";
const BORDER_COLOR: &str = "#44dd66";
const BORDER_SELECTED: &str = "#aaffbb";
const TEXT_COLOR: &str = "#ccffdd";
const TEXT_DIM: &str = "#66cc88";
const NODE_BG_TEXT: &str = "#040804";
const NODE_BG_IDEA: &str = "#041004";
const NODE_BG_NOTE: &str = "#0a0a04";
const EDGE_COLOR: &str = "#33aa55";
const EDGE_PREVIEW: &str = "#aaffbb";
const SELECT_BOX_FILL: &str = "rgba(100, 200, 130, 0.15)";
const SELECT_BOX_STROKE: &str = "#aaffbb";
const FONT: &str = "JetBrains Mono, Fira Code, Consolas, monospace";

pub fn render_board(
    ctx: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    board: &Board,
    camera: &Camera,
    selected_nodes: &HashSet<String>,
    selected_edge: Option<&String>,
    editing_node: Option<&String>,
    edge_preview: Option<(Option<&String>, f64, f64)>,
    selection_box: Option<(f64, f64, f64, f64)>,
) {
    let width = canvas.width() as f64;
    let height = canvas.height() as f64;

    ctx.set_fill_style_str(BG_COLOR);
    ctx.fill_rect(0.0, 0.0, width, height);

    draw_grid(ctx, camera, width, height);

    for edge in &board.edges {
        let is_selected = selected_edge.map_or(false, |id| id == &edge.id);
        draw_edge(ctx, board, edge, camera, is_selected);
    }

    if let Some((Some(from_node_id), to_screen_x, to_screen_y)) = edge_preview {
        draw_edge_preview(ctx, board, from_node_id, to_screen_x, to_screen_y, camera);
    }

    for node in &board.nodes {
        let is_selected = selected_nodes.contains(&node.id);
        let is_editing = editing_node.map_or(false, |id| id == &node.id);
        draw_node(ctx, node, camera, is_selected, is_editing);
    }

    if let Some((min_x, min_y, max_x, max_y)) = selection_box {
        draw_selection_box(ctx, camera, min_x, min_y, max_x, max_y);
    }
}

fn draw_grid(ctx: &CanvasRenderingContext2d, camera: &Camera, width: f64, height: f64) {
    let grid_size = 50.0 * camera.zoom;
    if grid_size < 10.0 {
        return;
    }

    ctx.set_stroke_style_str(GRID_COLOR);
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

fn draw_node(ctx: &CanvasRenderingContext2d, node: &Node, camera: &Camera, is_selected: bool, is_editing: bool) {
    let (screen_x, screen_y) = camera.world_to_screen(node.x, node.y);
    let screen_width = node.width * camera.zoom;
    let screen_height = node.height * camera.zoom;

    let bg_color = match node.node_type.as_str() {
        "idea" => NODE_BG_IDEA,
        "note" => NODE_BG_NOTE,
        _ => NODE_BG_TEXT,
    };
    ctx.set_fill_style_str(bg_color);
    ctx.fill_rect(screen_x, screen_y, screen_width, screen_height);

    if is_selected {
        ctx.set_stroke_style_str(BORDER_SELECTED);
        ctx.set_line_width(1.0);
        ctx.set_shadow_color(BORDER_SELECTED);
        ctx.set_shadow_blur(8.0);
    } else {
        ctx.set_stroke_style_str(BORDER_COLOR);
        ctx.set_line_width(1.0);
        ctx.set_shadow_blur(0.0);
    }
    ctx.stroke_rect(screen_x, screen_y, screen_width, screen_height);
    ctx.set_shadow_blur(0.0);

    if !is_editing {
        ctx.set_fill_style_str(if is_selected { TEXT_COLOR } else { TEXT_DIM });
        let font_size = (12.0 * camera.zoom).max(8.0);
        ctx.set_font(&format!("{}px {}", font_size, FONT));
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");

        let text_x = screen_x + screen_width / 2.0;
        let text_y = screen_y + screen_height / 2.0;

        let max_width = screen_width - 16.0 * camera.zoom;
        let _ = ctx.fill_text_with_max_width(&node.text, text_x, text_y, max_width);
    }

    let type_indicator = match node.node_type.as_str() {
        "idea" => "[IDEA]",
        "note" => "[NOTE]",
        _ => "[TEXT]",
    };
    ctx.set_fill_style_str(TEXT_DIM);
    let small_font = (9.0 * camera.zoom).max(6.0);
    ctx.set_font(&format!("{}px {}", small_font, FONT));
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    let _ = ctx.fill_text(type_indicator, screen_x + 4.0 * camera.zoom, screen_y + 4.0 * camera.zoom);
}

fn draw_edge(ctx: &CanvasRenderingContext2d, board: &Board, edge: &crate::state::Edge, camera: &Camera, is_selected: bool) {
    let from_node = board.nodes.iter().find(|n| n.id == edge.from_node);
    let to_node = board.nodes.iter().find(|n| n.id == edge.to_node);

    if let (Some(from), Some(to)) = (from_node, to_node) {
        let from_center_x = from.x + from.width / 2.0;
        let from_center_y = from.y + from.height / 2.0;
        let to_center_x = to.x + to.width / 2.0;
        let to_center_y = to.y + to.height / 2.0;

        let (from_screen_x, from_screen_y) = camera.world_to_screen(from_center_x, from_center_y);
        let (to_screen_x, to_screen_y) = camera.world_to_screen(to_center_x, to_center_y);

        if is_selected {
            ctx.set_stroke_style_str(BORDER_SELECTED);
            ctx.set_line_width(2.0);
            ctx.set_shadow_color(BORDER_SELECTED);
            ctx.set_shadow_blur(8.0);
        } else {
            ctx.set_stroke_style_str(EDGE_COLOR);
            ctx.set_line_width(1.0);
        }
        ctx.begin_path();
        ctx.move_to(from_screen_x, from_screen_y);
        ctx.line_to(to_screen_x, to_screen_y);
        ctx.stroke();
        ctx.set_shadow_blur(0.0);
    }
}

fn draw_edge_preview(
    ctx: &CanvasRenderingContext2d,
    board: &Board,
    from_node_id: &str,
    to_screen_x: f64,
    to_screen_y: f64,
    camera: &Camera,
) {
    if let Some(from) = board.nodes.iter().find(|n| n.id == from_node_id) {
        let from_center_x = from.x + from.width / 2.0;
        let from_center_y = from.y + from.height / 2.0;
        let (from_screen_x, from_screen_y) = camera.world_to_screen(from_center_x, from_center_y);

        ctx.set_stroke_style_str(EDGE_PREVIEW);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(from_screen_x, from_screen_y);
        ctx.line_to(to_screen_x, to_screen_y);
        ctx.stroke();
    }
}

fn draw_selection_box(
    ctx: &CanvasRenderingContext2d,
    camera: &Camera,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) {
    let (screen_min_x, screen_min_y) = camera.world_to_screen(min_x, min_y);
    let (screen_max_x, screen_max_y) = camera.world_to_screen(max_x, max_y);
    let width = screen_max_x - screen_min_x;
    let height = screen_max_y - screen_min_y;

    ctx.set_fill_style_str(SELECT_BOX_FILL);
    ctx.fill_rect(screen_min_x, screen_min_y, width, height);

    ctx.set_stroke_style_str(SELECT_BOX_STROKE);
    ctx.set_line_width(1.0);
    ctx.stroke_rect(screen_min_x, screen_min_y, width, height);
}

pub fn get_canvas_context(
    canvas: &HtmlCanvasElement,
) -> Result<CanvasRenderingContext2d, JsValue> {
    Ok(canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("Failed to get 2d context"))?
        .dyn_into::<CanvasRenderingContext2d>()?)
}
