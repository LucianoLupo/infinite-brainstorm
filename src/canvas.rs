use crate::app::is_local_md_file;
use crate::state::{Board, Camera, LinkPreview, Node, RESIZE_HANDLE_SIZE};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement};

const BG_COLOR: &str = "#020202";
const GRID_COLOR: &str = "#0a1a0a";
const BORDER_COLOR: &str = "#44dd66";
const BORDER_SELECTED: &str = "#aaffbb";
const TEXT_COLOR: &str = "#ccffdd";
const TEXT_DIM: &str = "#66cc88";
const NODE_BG_TEXT: &str = "#040804";
const NODE_BG_IDEA: &str = "#041004";
const NODE_BG_NOTE: &str = "#0a0a04";
const NODE_BG_IMAGE: &str = "#040408";
const NODE_BG_MD: &str = "#080408";
const NODE_BG_LINK: &str = "#040410";
const EDGE_COLOR: &str = "#33aa55";
const EDGE_PREVIEW: &str = "#aaffbb";
const SELECT_BOX_FILL: &str = "rgba(100, 200, 130, 0.15)";
const SELECT_BOX_STROKE: &str = "#aaffbb";
const RESIZE_HANDLE_COLOR: &str = "#aaffbb";
const RESIZE_HANDLE_BG: &str = "#020202";
const FONT: &str = "JetBrains Mono, Fira Code, Consolas, monospace";

pub type ImageCache = Rc<RefCell<HashMap<String, Option<HtmlImageElement>>>>;
pub type LinkPreviewCache = Rc<RefCell<HashMap<String, Option<LinkPreview>>>>;

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
    image_cache: &ImageCache,
    link_preview_cache: &LinkPreviewCache,
) {
    let width = canvas.width() as f64;
    let height = canvas.height() as f64;

    ctx.set_fill_style_str(BG_COLOR);
    ctx.fill_rect(0.0, 0.0, width, height);

    draw_grid(ctx, camera, width, height);

    for edge in &board.edges {
        let is_selected = selected_edge == Some(&edge.id);
        draw_edge(ctx, board, edge, camera, is_selected);
    }

    if let Some((Some(from_node_id), to_screen_x, to_screen_y)) = edge_preview {
        draw_edge_preview(ctx, board, from_node_id, to_screen_x, to_screen_y, camera);
    }

    for node in &board.nodes {
        let is_selected = selected_nodes.contains(&node.id);
        let is_editing = editing_node == Some(&node.id);
        draw_node(ctx, node, camera, is_selected, is_editing, image_cache, link_preview_cache);
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

fn draw_node(
    ctx: &CanvasRenderingContext2d,
    node: &Node,
    camera: &Camera,
    is_selected: bool,
    is_editing: bool,
    image_cache: &ImageCache,
    link_preview_cache: &LinkPreviewCache,
) {
    let (screen_x, screen_y) = camera.world_to_screen(node.x, node.y);
    let screen_width = node.width * camera.zoom;
    let screen_height = node.height * camera.zoom;

    let bg_color = match node.node_type.as_str() {
        "idea" => NODE_BG_IDEA,
        "note" => NODE_BG_NOTE,
        "image" => NODE_BG_IMAGE,
        "md" => NODE_BG_MD,
        "link" => NODE_BG_LINK,
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

    match node.node_type.as_str() {
        "image" => {
            draw_image_content(ctx, node, camera, screen_x, screen_y, screen_width, screen_height, image_cache);
        }
        "link" => {
            // Local .md files are rendered via HTML overlay like md nodes
            if !is_local_md_file(&node.text) {
                draw_link_content(ctx, node, camera, screen_x, screen_y, screen_width, screen_height, image_cache, link_preview_cache);
            }
            // Otherwise just show background + label (content handled by HTML overlay)
        }
        "md" => {
            // MD nodes render their content via HTML overlay, just show background + label
        }
        _ => {
            if !is_editing {
                ctx.set_fill_style_str(if is_selected { TEXT_COLOR } else { TEXT_DIM });
                let font_size = (12.0 * camera.zoom).max(8.0);
                ctx.set_font(&format!("{}px {}", font_size, FONT));

                let padding = 8.0 * camera.zoom;
                let label_height = 16.0 * camera.zoom;
                let text_x = screen_x + screen_width / 2.0;
                let text_y = screen_y + label_height + (screen_height - label_height) / 2.0;
                let max_width = screen_width - 2.0 * padding;
                let max_height = screen_height - label_height - padding;
                let line_height = font_size * 1.4;

                draw_wrapped_text(ctx, &node.text, text_x, text_y, max_width, max_height, line_height);
            }
        }
    }

    let type_indicator = match node.node_type.as_str() {
        "idea" => "[IDEA]",
        "note" => "[NOTE]",
        "image" => "[IMAGE]",
        "md" => "[MD]",
        "link" => "[LINK]",
        _ => "[TEXT]",
    };
    ctx.set_fill_style_str(TEXT_DIM);
    let small_font = (9.0 * camera.zoom).max(6.0);
    ctx.set_font(&format!("{}px {}", small_font, FONT));
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    let _ = ctx.fill_text(type_indicator, screen_x + 4.0 * camera.zoom, screen_y + 4.0 * camera.zoom);

    if is_selected {
        draw_resize_handles(ctx, screen_x, screen_y, screen_width, screen_height, camera.zoom);
    }
}

fn draw_image_content(
    ctx: &CanvasRenderingContext2d,
    node: &Node,
    camera: &Camera,
    screen_x: f64,
    screen_y: f64,
    screen_width: f64,
    screen_height: f64,
    image_cache: &ImageCache,
) {
    let url = &node.text;
    let cache = image_cache.borrow();

    match cache.get(url) {
        Some(Some(img)) => {
            // Image is loaded, draw it
            let padding = 4.0 * camera.zoom;
            let label_height = 16.0 * camera.zoom;
            let img_x = screen_x + padding;
            let img_y = screen_y + label_height + padding;
            let img_max_w = screen_width - 2.0 * padding;
            let img_max_h = screen_height - label_height - 2.0 * padding;

            let natural_w = img.natural_width() as f64;
            let natural_h = img.natural_height() as f64;

            if natural_w > 0.0 && natural_h > 0.0 {
                // Scale to fit the available space, allowing upscaling when zoomed in
                let scale = (img_max_w / natural_w).min(img_max_h / natural_h);
                let draw_w = natural_w * scale;
                let draw_h = natural_h * scale;
                let offset_x = (img_max_w - draw_w) / 2.0;
                let offset_y = (img_max_h - draw_h) / 2.0;

                let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                    img,
                    img_x + offset_x,
                    img_y + offset_y,
                    draw_w,
                    draw_h,
                );
            }

            // Show filename
            let filename = url.rsplit('/').next().unwrap_or(url);
            let truncated = if filename.len() > 20 {
                format!("{}...", &filename[..17])
            } else {
                filename.to_string()
            };
            ctx.set_fill_style_str(TEXT_DIM);
            let small_font = (9.0 * camera.zoom).max(6.0);
            ctx.set_font(&format!("{}px {}", small_font, FONT));
            ctx.set_text_align("right");
            ctx.set_text_baseline("top");
            let _ = ctx.fill_text(&truncated, screen_x + screen_width - 4.0 * camera.zoom, screen_y + 4.0 * camera.zoom);
        }
        Some(None) => {
            // Image is loading
            ctx.set_fill_style_str(TEXT_DIM);
            let font_size = (12.0 * camera.zoom).max(8.0);
            ctx.set_font(&format!("{}px {}", font_size, FONT));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("Loading...", screen_x + screen_width / 2.0, screen_y + screen_height / 2.0);
        }
        None => {
            // Image not in cache yet, show placeholder
            ctx.set_fill_style_str(TEXT_DIM);
            let font_size = (12.0 * camera.zoom).max(8.0);
            ctx.set_font(&format!("{}px {}", font_size, FONT));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("[No Image]", screen_x + screen_width / 2.0, screen_y + screen_height / 2.0);
        }
    }
}

fn draw_link_content(
    ctx: &CanvasRenderingContext2d,
    node: &Node,
    camera: &Camera,
    screen_x: f64,
    screen_y: f64,
    screen_width: f64,
    screen_height: f64,
    image_cache: &ImageCache,
    link_preview_cache: &LinkPreviewCache,
) {
    let url = &node.text;
    let cache = link_preview_cache.borrow();
    let padding = 4.0 * camera.zoom;
    let label_height = 16.0 * camera.zoom;
    let domain_font_size = (9.0 * camera.zoom).max(6.0);

    // Content bounds (inside node, below label)
    let content_top = screen_y + label_height;
    let content_bottom = screen_y + screen_height - padding;
    let content_left = screen_x + padding;
    let content_width = screen_width - 2.0 * padding;
    let content_height = content_bottom - content_top - domain_font_size - padding;

    // Use clipping to prevent drawing outside node
    ctx.save();
    ctx.begin_path();
    ctx.rect(screen_x, screen_y, screen_width, screen_height);
    ctx.clip();

    match cache.get(url) {
        Some(Some(preview)) => {
            // Draw preview image - OG images usually contain title/desc already
            if let Some(ref image_url) = preview.image {
                let img_cache = image_cache.borrow();
                if let Some(Some(img)) = img_cache.get(image_url) {
                    let natural_w = img.natural_width() as f64;
                    let natural_h = img.natural_height() as f64;

                    if natural_w > 0.0 && natural_h > 0.0 && content_height > 10.0 {
                        let scale = (content_width / natural_w).min(content_height / natural_h);
                        let draw_w = natural_w * scale;
                        let draw_h = natural_h * scale;
                        let offset_x = (content_width - draw_w) / 2.0;

                        let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                            img,
                            content_left + offset_x,
                            content_top,
                            draw_w,
                            draw_h,
                        );
                    }
                }
            }

            // Draw domain at bottom
            let domain = preview.site_name.clone().unwrap_or_else(|| {
                url.split('/').nth(2).unwrap_or(url).to_string()
            });
            ctx.set_fill_style_str(TEXT_DIM);
            ctx.set_font(&format!("{}px {}", domain_font_size, FONT));
            ctx.set_text_align("right");
            ctx.set_text_baseline("bottom");
            let _ = ctx.fill_text(&domain, screen_x + screen_width - padding, content_bottom);
        }
        Some(None) => {
            ctx.set_fill_style_str(TEXT_DIM);
            let font_size = (12.0 * camera.zoom).max(8.0);
            ctx.set_font(&format!("{}px {}", font_size, FONT));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("Loading...", screen_x + screen_width / 2.0, screen_y + screen_height / 2.0);
        }
        None => {
            ctx.set_fill_style_str(TEXT_DIM);
            let font_size = (10.0 * camera.zoom).max(7.0);
            ctx.set_font(&format!("{}px {}", font_size, FONT));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text_with_max_width(url, screen_x + screen_width / 2.0, screen_y + screen_height / 2.0, content_width);
        }
    }

    ctx.restore();
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

fn draw_resize_handles(
    ctx: &CanvasRenderingContext2d,
    screen_x: f64,
    screen_y: f64,
    screen_width: f64,
    screen_height: f64,
    zoom: f64,
) {
    let handle_size = RESIZE_HANDLE_SIZE * zoom;
    let half = handle_size / 2.0;

    ctx.set_fill_style_str(RESIZE_HANDLE_BG);
    ctx.set_stroke_style_str(RESIZE_HANDLE_COLOR);
    ctx.set_line_width(1.0);

    // Top-left
    ctx.fill_rect(screen_x - half, screen_y - half, handle_size, handle_size);
    ctx.stroke_rect(screen_x - half, screen_y - half, handle_size, handle_size);

    // Top-right
    ctx.fill_rect(screen_x + screen_width - half, screen_y - half, handle_size, handle_size);
    ctx.stroke_rect(screen_x + screen_width - half, screen_y - half, handle_size, handle_size);

    // Bottom-left
    ctx.fill_rect(screen_x - half, screen_y + screen_height - half, handle_size, handle_size);
    ctx.stroke_rect(screen_x - half, screen_y + screen_height - half, handle_size, handle_size);

    // Bottom-right
    ctx.fill_rect(screen_x + screen_width - half, screen_y + screen_height - half, handle_size, handle_size);
    ctx.stroke_rect(screen_x + screen_width - half, screen_y + screen_height - half, handle_size, handle_size);
}

/// Wrap text into multiple lines that fit within max_width
fn wrap_text(ctx: &CanvasRenderingContext2d, text: &str, max_width: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    // Split by explicit newlines first
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }

        let words: Vec<&str> = paragraph.split_whitespace().collect();
        if words.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();

        for word in words {
            let test_line = if current_line.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current_line, word)
            };

            let metrics = ctx.measure_text(&test_line).unwrap_or_else(|_| {
                ctx.measure_text("").unwrap()
            });

            if metrics.width() <= max_width || current_line.is_empty() {
                current_line = test_line;
            } else {
                lines.push(current_line);
                current_line = word.to_string();
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Draw wrapped text centered in a box
fn draw_wrapped_text(
    ctx: &CanvasRenderingContext2d,
    text: &str,
    center_x: f64,
    center_y: f64,
    max_width: f64,
    max_height: f64,
    line_height: f64,
) {
    let lines = wrap_text(ctx, text, max_width);

    // Clamp to available height
    let visible_lines = ((max_height / line_height).floor() as usize).max(1);
    let lines_to_draw: Vec<_> = lines.into_iter().take(visible_lines).collect();
    let actual_height = lines_to_draw.len() as f64 * line_height;

    // Start Y position to center the text block
    let start_y = center_y - actual_height / 2.0 + line_height / 2.0;

    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");

    for (i, line) in lines_to_draw.iter().enumerate() {
        let y = start_y + i as f64 * line_height;
        let _ = ctx.fill_text(line, center_x, y);
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
