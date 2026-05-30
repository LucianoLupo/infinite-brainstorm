use crate::app::is_local_md_file;
use crate::state::{
    truncate_filename, Board, Camera, LinkPreview, Node, NodeType, RESIZE_HANDLE_SIZE,
};
use std::cell::{Cell, RefCell};
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
const EDGE_LABEL_BG: &str = "rgba(2, 2, 2, 0.85)";
const GROUP_BG: &str = "rgba(50, 170, 85, 0.06)";
const GROUP_BORDER: &str = "rgba(50, 170, 85, 0.25)";
const GROUP_LABEL_COLOR: &str = "#448855";
const FONT: &str = "JetBrains Mono, Fira Code, Consolas, monospace";

/// Tri-state for an async-loaded cache entry. Replaces the previous
/// `Option<T>` (where `None` ambiguously meant "loading") so a load *failure*
/// is distinct from a load *in progress*: a failed entry stops re-fetching on
/// every render (no spin loop) yet can be evicted and retried, and the renderer
/// can show a real error instead of a perpetual "Loading…".
#[derive(Clone)]
pub enum LoadState<T> {
    /// Fetch issued, result not yet available.
    Loading,
    /// Fetch succeeded with this value.
    Loaded(T),
    /// Fetch failed; entry is terminal until evicted (e.g. node deleted or URL
    /// changed), at which point it will be retried.
    Failed,
}

impl<T> LoadState<T> {
    /// The loaded value, if any.
    pub fn loaded(&self) -> Option<&T> {
        match self {
            LoadState::Loaded(v) => Some(v),
            _ => None,
        }
    }
}

pub type ImageCache = Rc<RefCell<HashMap<String, LoadState<HtmlImageElement>>>>;
pub type LinkPreviewCache = Rc<RefCell<HashMap<String, LoadState<LinkPreview>>>>;

/// Soft cap on the number of decoded images kept in [`ImageCache`]. When a fresh
/// image finishes loading and the cache is over this size, the least-recently
/// inserted entry that is *not* currently on the board is evicted. Bounds memory
/// for boards that cycle through many image URLs over a session.
pub const IMAGE_CACHE_CAP: usize = 64;

/// Identity of a wrapped-text layout. Wrapping is a pure function of the text,
/// the wrap width, and the font size — all three are captured here so identical
/// frames (e.g. during a pan, where nothing but the camera offset changes) reuse
/// the previously computed line breaks instead of re-measuring every word.
///
/// `width` and `font_px` are bucketed to whole pixels (`u32`) so sub-pixel
/// camera jitter doesn't thrash the cache, and `node_id` lets us evict a node's
/// entry when it is deleted.
#[derive(Clone, PartialEq, Eq, Hash)]
struct WrapKey {
    node_id: String,
    text: String,
    width: u32,
    font_px: u32,
}

thread_local! {
    /// Memoized wrapped-text layouts. Pruned to the live node set once per render
    /// pass (see [`prune_wrap_cache`]) so it cannot grow without bound.
    static WRAP_CACHE: RefCell<HashMap<WrapKey, Rc<Vec<String>>>> =
        RefCell::new(HashMap::new());
}

/// Drop wrapped-text cache entries whose owning node is no longer on the board.
/// Called once per [`render_board`] pass before any node is drawn.
fn prune_wrap_cache(live_ids: &HashSet<&str>) {
    WRAP_CACHE.with(|c| {
        c.borrow_mut()
            .retain(|k, _| live_ids.contains(k.node_id.as_str()));
    });
}

/// [`wrap_text`] with memoization keyed on `(node_id, text, width, font_px)`.
///
/// The caller must have already set `ctx`'s font to `font_px` (as `draw_node`
/// does) so a cache *miss* measures with the same font the key records. On a hit
/// no `measure_text` calls happen at all — the dominant cost of a pan frame for
/// text-heavy boards.
fn wrap_text_cached(
    ctx: &CanvasRenderingContext2d,
    node_id: &str,
    text: &str,
    max_width: f64,
    font_px: u32,
) -> Rc<Vec<String>> {
    let key = WrapKey {
        node_id: node_id.to_string(),
        text: text.to_string(),
        width: max_width.max(0.0).round() as u32,
        font_px,
    };

    if let Some(hit) = WRAP_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }

    let lines = Rc::new(wrap_text(ctx, text, max_width));
    WRAP_CACHE.with(|c| {
        c.borrow_mut().insert(key, lines.clone());
    });
    lines
}

/// Extra screen-space margin (px) kept around the canvas when culling, so that
/// nodes/edges straddling the viewport edge (and their shadows, arrowheads, and
/// labels) still draw instead of popping in/out as they cross the boundary.
const CULL_MARGIN: f64 = 64.0;

/// Returns `true` when the screen-space axis-aligned box `[x0,x1] x [y0,y1]`
/// lies fully outside the canvas viewport (expanded by [`CULL_MARGIN`]) and can
/// therefore be skipped. Boxes that overlap the viewport at all are kept.
fn box_outside_viewport(x0: f64, y0: f64, x1: f64, y1: f64, view_w: f64, view_h: f64) -> bool {
    // Normalise so x0<=x1, y0<=y1 regardless of caller ordering.
    let (lo_x, hi_x) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let (lo_y, hi_y) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    hi_x < -CULL_MARGIN
        || lo_x > view_w + CULL_MARGIN
        || hi_y < -CULL_MARGIN
        || lo_y > view_h + CULL_MARGIN
}

/// Returns `true` when an edge can be skipped because the screen-space bounding
/// box spanning its two endpoint node centers is fully outside the viewport.
/// Edges whose endpoints are missing draw nothing, so they're also "outside".
/// Using node centers (a superset of the clipped+arrowhead line) means a kept
/// edge is never wrongly culled.
fn edge_outside_viewport(
    node_map: &HashMap<&str, &Node>,
    edge: &crate::state::Edge,
    camera: &Camera,
    view_w: f64,
    view_h: f64,
) -> bool {
    match (
        node_map.get(edge.from_node.as_str()),
        node_map.get(edge.to_node.as_str()),
    ) {
        (Some(from), Some(to)) => {
            let (fx, fy) =
                camera.world_to_screen(from.x + from.width / 2.0, from.y + from.height / 2.0);
            let (tx, ty) =
                camera.world_to_screen(to.x + to.width / 2.0, to.y + to.height / 2.0);
            box_outside_viewport(fx, fy, tx, ty, view_w, view_h)
        }
        _ => true,
    }
}

/// All inputs to a single [`render_board`] pass, bundled into one named-field
/// struct. Using named fields (rather than a long positional argument list)
/// makes a tuple-transposition mistake at the call site impossible: every input
/// is labeled, so e.g. `selected_nodes` and `editing_node` can't be swapped.
pub struct RenderState<'a> {
    pub ctx: &'a CanvasRenderingContext2d,
    pub canvas: &'a HtmlCanvasElement,
    pub board: &'a Board,
    pub camera: &'a Camera,
    pub selected_nodes: &'a HashSet<String>,
    pub selected_edge: Option<&'a String>,
    pub editing_node: Option<&'a String>,
    /// In-progress edge being dragged: `(from_node_id, cursor_screen_x, cursor_screen_y)`.
    pub edge_preview: Option<(Option<&'a String>, f64, f64)>,
    /// Active box-selection rectangle in world coords: `(min_x, min_y, max_x, max_y)`.
    pub selection_box: Option<(f64, f64, f64, f64)>,
    pub image_cache: &'a ImageCache,
    pub link_preview_cache: &'a LinkPreviewCache,
    /// Device-pixel ratio applied by the caller as a context transform
    /// (`ctx.set_transform(dpr,0,0,dpr,0,0)`). All drawing here happens in CSS
    /// pixels, so the on-screen dimensions are `backing-store / dpr`.
    pub dpr: f64,
}

pub fn render_board(state: RenderState) {
    let RenderState {
        ctx,
        canvas,
        board,
        camera,
        selected_nodes,
        selected_edge,
        editing_node,
        edge_preview,
        selection_box,
        image_cache,
        link_preview_cache,
        dpr,
    } = state;

    // The backing store is sized `display * dpr`; the caller has scaled the
    // context by `dpr`, so every draw call below works in CSS-pixel space.
    // Use CSS dimensions for the background/grid so we cover exactly the visible
    // area regardless of the device-pixel ratio.
    let dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    let width = canvas.width() as f64 / dpr;
    let height = canvas.height() as f64 / dpr;

    ctx.set_fill_style_str(BG_COLOR);
    ctx.fill_rect(0.0, 0.0, width, height);

    draw_grid(ctx, camera, width, height);

    draw_groups(ctx, board, camera);

    let node_map: HashMap<&str, &Node> = board.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // Evict wrapped-text cache entries for nodes that no longer exist before any
    // drawing happens, keeping the memo bounded to the live board.
    {
        let live_ids: HashSet<&str> = node_map.keys().copied().collect();
        prune_wrap_cache(&live_ids);
    }

    for edge in &board.edges {
        if edge_outside_viewport(&node_map, edge, camera, width, height) {
            continue;
        }
        let is_selected = selected_edge == Some(&edge.id);
        draw_edge(ctx, &node_map, edge, camera, is_selected);
    }

    if let Some((Some(from_node_id), to_screen_x, to_screen_y)) = edge_preview {
        draw_edge_preview(ctx, &node_map, from_node_id, to_screen_x, to_screen_y, camera);
    }

    for node in &board.nodes {
        let (sx, sy) = camera.world_to_screen(node.x, node.y);
        let sw = node.width * camera.zoom;
        let sh = node.height * camera.zoom;
        if box_outside_viewport(sx, sy, sx + sw, sy + sh, width, height) {
            continue;
        }
        let is_selected = selected_nodes.contains(&node.id);
        let is_editing = editing_node == Some(&node.id);
        draw_node(ctx, node, camera, is_selected, is_editing, image_cache, link_preview_cache);
    }

    if let Some((min_x, min_y, max_x, max_y)) = selection_box {
        draw_selection_box(ctx, camera, min_x, min_y, max_x, max_y);
    }
}

fn draw_groups(ctx: &CanvasRenderingContext2d, board: &Board, camera: &Camera) {
    // Early-out the common case: no grouped nodes means nothing to draw and we
    // skip allocating the bounds map entirely.
    if !board.nodes.iter().any(|n| n.group.is_some()) {
        return;
    }

    let mut groups: HashMap<&str, (f64, f64, f64, f64)> = HashMap::new();

    for node in &board.nodes {
        if let Some(ref group) = node.group {
            let entry = groups.entry(group.as_str()).or_insert((
                node.x,
                node.y,
                node.x + node.width,
                node.y + node.height,
            ));
            entry.0 = entry.0.min(node.x);
            entry.1 = entry.1.min(node.y);
            entry.2 = entry.2.max(node.x + node.width);
            entry.3 = entry.3.max(node.y + node.height);
        }
    }

    let padding = 30.0;
    let label_font_size = (10.0 * camera.zoom).max(7.0);

    for (name, (min_x, min_y, max_x, max_y)) in &groups {
        let (sx, sy) = camera.world_to_screen(min_x - padding, min_y - padding);
        let (ex, ey) = camera.world_to_screen(max_x + padding, max_y + padding);
        let w = ex - sx;
        let h = ey - sy;

        ctx.set_fill_style_str(GROUP_BG);
        ctx.fill_rect(sx, sy, w, h);

        ctx.set_stroke_style_str(GROUP_BORDER);
        ctx.set_line_width(1.0);
        ctx.stroke_rect(sx, sy, w, h);

        ctx.set_fill_style_str(GROUP_LABEL_COLOR);
        ctx.set_font(&format!("{}px {}", label_font_size, FONT));
        ctx.set_text_align("left");
        ctx.set_text_baseline("top");
        let label_pad = 4.0 * camera.zoom;
        let _ = ctx.fill_text(name, sx + label_pad, sy + label_pad);
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

    let bg_color = match node.node_type {
        NodeType::Idea => NODE_BG_IDEA,
        NodeType::Note => NODE_BG_NOTE,
        NodeType::Image => NODE_BG_IMAGE,
        NodeType::Md => NODE_BG_MD,
        NodeType::Link => NODE_BG_LINK,
        NodeType::Text | NodeType::Unknown => NODE_BG_TEXT,
    };
    ctx.set_fill_style_str(bg_color);
    ctx.fill_rect(screen_x, screen_y, screen_width, screen_height);

    if is_selected {
        let border = node.color.as_deref().unwrap_or(BORDER_SELECTED);
        ctx.set_stroke_style_str(border);
        ctx.set_line_width(1.0);
        ctx.set_shadow_color(border);
        ctx.set_shadow_blur(8.0);
    } else {
        let border = node.color.as_deref().unwrap_or(BORDER_COLOR);
        ctx.set_stroke_style_str(border);
        ctx.set_line_width(1.0);
        ctx.set_shadow_blur(0.0);
    }
    ctx.stroke_rect(screen_x, screen_y, screen_width, screen_height);
    ctx.set_shadow_blur(0.0);

    match node.node_type {
        NodeType::Image => {
            draw_image_content(ctx, node, camera, screen_x, screen_y, screen_width, screen_height, image_cache);
        }
        NodeType::Link => {
            // Local .md files are rendered via HTML overlay like md nodes
            if !is_local_md_file(&node.text) {
                draw_link_content(ctx, node, camera, screen_x, screen_y, screen_width, screen_height, image_cache, link_preview_cache);
            }
            // Otherwise just show background + label (content handled by HTML overlay)
        }
        NodeType::Md => {
            // MD nodes render their content via HTML overlay, just show background + label
        }
        NodeType::Text | NodeType::Idea | NodeType::Note | NodeType::Unknown => {
            if !is_editing {
                ctx.set_fill_style_str(if is_selected { TEXT_COLOR } else { TEXT_DIM });
                // Bucket the font size to a whole pixel; this is both the rendered
                // font and the wrap-cache key dimension, so identical buckets reuse
                // the cached line breaks.
                let font_px = (12.0 * camera.zoom).max(8.0).round() as u32;
                set_font_px(ctx, font_px);

                let padding = 8.0 * camera.zoom;
                let label_height = 16.0 * camera.zoom;
                let text_x = screen_x + screen_width / 2.0;
                let text_y = screen_y + label_height + (screen_height - label_height) / 2.0;
                let max_width = screen_width - 2.0 * padding;
                let max_height = screen_height - label_height - padding;
                let line_height = font_px as f64 * 1.4;

                draw_wrapped_text(
                    ctx, &node.id, &node.text, text_x, text_y, max_width, max_height,
                    line_height, font_px,
                );
            }
        }
    }

    let type_indicator = match node.node_type {
        NodeType::Idea => "[IDEA]",
        NodeType::Note => "[NOTE]",
        NodeType::Image => "[IMAGE]",
        NodeType::Md => "[MD]",
        NodeType::Link => "[LINK]",
        NodeType::Text | NodeType::Unknown => "[TEXT]",
    };
    ctx.set_fill_style_str(TEXT_DIM);
    let small_font = (9.0 * camera.zoom).max(6.0);
    ctx.set_font(&format!("{}px {}", small_font, FONT));
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    let pad = 4.0 * camera.zoom;
    let _ = ctx.fill_text(type_indicator, screen_x + pad, screen_y + pad);

    if let Some(priority) = node.priority {
        let p_text = format!("P{}", priority.clamp(1, 5));
        let type_width = ctx.measure_text(type_indicator).map(|m| m.width()).unwrap_or(30.0);
        let _ = ctx.fill_text(&p_text, screen_x + pad + type_width + pad, screen_y + pad);
    }

    if let Some(ref status) = node.status {
        ctx.set_text_align("right");
        let _ = ctx.fill_text(status, screen_x + screen_width - pad, screen_y + pad);
    }

    if !node.tags.is_empty() {
        let tags_text = node.tags.join(", ");
        let tag_font = (8.0 * camera.zoom).max(5.0);
        ctx.set_font(&format!("{}px {}", tag_font, FONT));
        ctx.set_text_align("left");
        ctx.set_text_baseline("bottom");
        let _ = ctx.fill_text_with_max_width(
            &tags_text,
            screen_x + pad,
            screen_y + screen_height - pad,
            screen_width - 2.0 * pad,
        );
    }

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
        Some(LoadState::Loaded(img)) => {
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
            let truncated = truncate_filename(filename);
            ctx.set_fill_style_str(TEXT_DIM);
            let small_font = (9.0 * camera.zoom).max(6.0);
            ctx.set_font(&format!("{}px {}", small_font, FONT));
            ctx.set_text_align("right");
            ctx.set_text_baseline("top");
            let _ = ctx.fill_text(&truncated, screen_x + screen_width - 4.0 * camera.zoom, screen_y + 4.0 * camera.zoom);
        }
        Some(LoadState::Loading) => {
            // Image fetch in progress
            ctx.set_fill_style_str(TEXT_DIM);
            let font_size = (12.0 * camera.zoom).max(8.0);
            ctx.set_font(&format!("{}px {}", font_size, FONT));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("Loading...", screen_x + screen_width / 2.0, screen_y + screen_height / 2.0);
        }
        Some(LoadState::Failed) => {
            // Fetch failed — distinct from loading so the user sees the error.
            ctx.set_fill_style_str(TEXT_DIM);
            let font_size = (12.0 * camera.zoom).max(8.0);
            ctx.set_font(&format!("{}px {}", font_size, FONT));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("[Image failed]", screen_x + screen_width / 2.0, screen_y + screen_height / 2.0);
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
        Some(LoadState::Loaded(preview)) => {
            // Draw preview image - OG images usually contain title/desc already
            if let Some(ref image_url) = preview.image {
                let img_cache = image_cache.borrow();
                if let Some(LoadState::Loaded(img)) = img_cache.get(image_url) {
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
        Some(LoadState::Loading) => {
            ctx.set_fill_style_str(TEXT_DIM);
            let font_size = (12.0 * camera.zoom).max(8.0);
            ctx.set_font(&format!("{}px {}", font_size, FONT));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("Loading...", screen_x + screen_width / 2.0, screen_y + screen_height / 2.0);
        }
        // Failed preview or not-yet-fetched: fall back to showing the raw URL so
        // the node is still useful (and a failed link doesn't show a stale spinner).
        Some(LoadState::Failed) | None => {
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

/// Find the point where a line from `from` toward the center of a rectangle
/// intersects the rectangle boundary.
fn clip_line_to_rect(
    from_x: f64, from_y: f64,
    rect_cx: f64, rect_cy: f64,
    half_w: f64, half_h: f64,
) -> (f64, f64) {
    let dx = from_x - rect_cx;
    let dy = from_y - rect_cy;

    if dx.abs() < 1e-10 && dy.abs() < 1e-10 {
        return (rect_cx, rect_cy);
    }

    let tx = if dx.abs() > 1e-10 { half_w / dx.abs() } else { f64::INFINITY };
    let ty = if dy.abs() > 1e-10 { half_h / dy.abs() } else { f64::INFINITY };
    let t = tx.min(ty);

    (rect_cx + t * dx, rect_cy + t * dy)
}

/// Draw a filled arrowhead triangle at (tip_x, tip_y) pointing in the given angle.
fn draw_arrowhead(ctx: &CanvasRenderingContext2d, tip_x: f64, tip_y: f64, angle: f64, size: f64) {
    let spread = 0.4; // ~23 degrees

    let x1 = tip_x - size * (angle - spread).cos();
    let y1 = tip_y - size * (angle - spread).sin();
    let x2 = tip_x - size * (angle + spread).cos();
    let y2 = tip_y - size * (angle + spread).sin();

    ctx.begin_path();
    ctx.move_to(tip_x, tip_y);
    ctx.line_to(x1, y1);
    ctx.line_to(x2, y2);
    ctx.close_path();
    ctx.fill();
}

fn draw_edge(ctx: &CanvasRenderingContext2d, node_map: &HashMap<&str, &Node>, edge: &crate::state::Edge, camera: &Camera, is_selected: bool) {
    let from_node = node_map.get(edge.from_node.as_str());
    let to_node = node_map.get(edge.to_node.as_str());

    if let (Some(from), Some(to)) = (from_node, to_node) {
        let from_cx = from.x + from.width / 2.0;
        let from_cy = from.y + from.height / 2.0;
        let to_cx = to.x + to.width / 2.0;
        let to_cy = to.y + to.height / 2.0;

        // Clip line to node boundaries (world coordinates)
        let (from_bx, from_by) = clip_line_to_rect(to_cx, to_cy, from_cx, from_cy, from.width / 2.0, from.height / 2.0);
        let (to_bx, to_by) = clip_line_to_rect(from_cx, from_cy, to_cx, to_cy, to.width / 2.0, to.height / 2.0);

        let (from_sx, from_sy) = camera.world_to_screen(from_bx, from_by);
        let (to_sx, to_sy) = camera.world_to_screen(to_bx, to_by);

        let angle = (to_sy - from_sy).atan2(to_sx - from_sx);
        let arrow_size = (10.0 * camera.zoom).clamp(5.0, 20.0);

        if is_selected {
            ctx.set_stroke_style_str(BORDER_SELECTED);
            ctx.set_fill_style_str(BORDER_SELECTED);
            ctx.set_line_width(2.0);
            ctx.set_shadow_color(BORDER_SELECTED);
            ctx.set_shadow_blur(8.0);
        } else {
            ctx.set_stroke_style_str(EDGE_COLOR);
            ctx.set_fill_style_str(EDGE_COLOR);
            ctx.set_line_width(1.0);
        }

        ctx.begin_path();
        ctx.move_to(from_sx, from_sy);
        ctx.line_to(to_sx, to_sy);
        ctx.stroke();

        draw_arrowhead(ctx, to_sx, to_sy, angle, arrow_size);

        ctx.set_shadow_blur(0.0);

        if let Some(ref label) = edge.label {
            let mid_x = (from_sx + to_sx) / 2.0;
            let mid_y = (from_sy + to_sy) / 2.0;
            let label_font_size = (10.0 * camera.zoom).max(7.0);
            ctx.set_font(&format!("{}px {}", label_font_size, FONT));
            let text_metrics = ctx.measure_text(label).ok();
            let text_w = text_metrics.map(|m| m.width()).unwrap_or(40.0);
            let pill_h = label_font_size + 6.0;
            let pill_w = text_w + 10.0;

            ctx.set_fill_style_str(EDGE_LABEL_BG);
            ctx.fill_rect(mid_x - pill_w / 2.0, mid_y - pill_h / 2.0, pill_w, pill_h);

            ctx.set_fill_style_str(TEXT_DIM);
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text(label, mid_x, mid_y);
        }
    }
}

fn draw_edge_preview(
    ctx: &CanvasRenderingContext2d,
    node_map: &HashMap<&str, &Node>,
    from_node_id: &str,
    to_screen_x: f64,
    to_screen_y: f64,
    camera: &Camera,
) {
    if let Some(from) = node_map.get(from_node_id) {
        let from_cx = from.x + from.width / 2.0;
        let from_cy = from.y + from.height / 2.0;

        // Clip line start to source node boundary
        let (to_wx, to_wy) = camera.screen_to_world(to_screen_x, to_screen_y);
        let (from_bx, from_by) = clip_line_to_rect(to_wx, to_wy, from_cx, from_cy, from.width / 2.0, from.height / 2.0);
        let (from_sx, from_sy) = camera.world_to_screen(from_bx, from_by);

        let angle = (to_screen_y - from_sy).atan2(to_screen_x - from_sx);
        let arrow_size = (10.0 * camera.zoom).clamp(5.0, 20.0);

        ctx.set_stroke_style_str(EDGE_PREVIEW);
        ctx.set_fill_style_str(EDGE_PREVIEW);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(from_sx, from_sy);
        ctx.line_to(to_screen_x, to_screen_y);
        ctx.stroke();

        draw_arrowhead(ctx, to_screen_x, to_screen_y, angle, arrow_size);
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

/// Draw wrapped text centered in a box. Uses the memoized [`wrap_text_cached`],
/// so a frame that doesn't change a node's text/width/zoom does no word
/// measurement at all.
#[allow(clippy::too_many_arguments)]
fn draw_wrapped_text(
    ctx: &CanvasRenderingContext2d,
    node_id: &str,
    text: &str,
    center_x: f64,
    center_y: f64,
    max_width: f64,
    max_height: f64,
    line_height: f64,
    font_px: u32,
) {
    let lines = wrap_text_cached(ctx, node_id, text, max_width, font_px);

    // Clamp to available height
    let visible_lines = ((max_height / line_height).floor() as usize).max(1);
    let lines_to_draw = lines.iter().take(visible_lines);
    let drawn_count = lines.len().min(visible_lines);
    let actual_height = drawn_count as f64 * line_height;

    // Start Y position to center the text block
    let start_y = center_y - actual_height / 2.0 + line_height / 2.0;

    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");

    for (i, line) in lines_to_draw.enumerate() {
        let y = start_y + i as f64 * line_height;
        let _ = ctx.fill_text(line, center_x, y);
    }
}

thread_local! {
    /// Caches the last `"<px>px <FONT>"` string set via [`set_font_px`]. The font
    /// string only changes when the bucketed pixel size changes (i.e. on zoom),
    /// so a pan frame builds zero font strings even across many nodes.
    static LAST_FONT_PX: Cell<u32> = const { Cell::new(0) };
    static LAST_FONT_STR: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Set the canvas font to `<font_px>px <FONT>`, reusing the previously-built
/// string when the pixel size is unchanged from the last call. Hoists the
/// per-node `format!` out of the hot path: within a frame every text node shares
/// the same bucketed size, so the string is formatted at most once per zoom level.
fn set_font_px(ctx: &CanvasRenderingContext2d, font_px: u32) {
    let unchanged = LAST_FONT_PX.with(|p| p.get() == font_px);
    if !unchanged {
        let s = format!("{}px {}", font_px, FONT);
        LAST_FONT_PX.with(|p| p.set(font_px));
        LAST_FONT_STR.with(|f| *f.borrow_mut() = s);
    }
    LAST_FONT_STR.with(|f| ctx.set_font(&f.borrow()));
}

pub fn get_canvas_context(
    canvas: &HtmlCanvasElement,
) -> Result<CanvasRenderingContext2d, JsValue> {
    Ok(canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("Failed to get 2d context"))?
        .dyn_into::<CanvasRenderingContext2d>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod clip_line_to_rect_tests {
        use super::*;

        // Rectangle centered at (100, 100), 200x100 → half_w=100, half_h=50

        #[test]
        fn from_right() {
            let (x, y) = clip_line_to_rect(300.0, 100.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 200.0).abs() < 1e-10);
            assert!((y - 100.0).abs() < 1e-10);
        }

        #[test]
        fn from_left() {
            let (x, y) = clip_line_to_rect(-100.0, 100.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 0.0).abs() < 1e-10);
            assert!((y - 100.0).abs() < 1e-10);
        }

        #[test]
        fn from_above() {
            let (x, y) = clip_line_to_rect(100.0, -100.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 100.0).abs() < 1e-10);
            assert!((y - 50.0).abs() < 1e-10);
        }

        #[test]
        fn from_below() {
            let (x, y) = clip_line_to_rect(100.0, 300.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 100.0).abs() < 1e-10);
            assert!((y - 150.0).abs() < 1e-10);
        }

        #[test]
        fn from_diagonal_hits_right_edge() {
            // From (400, 100) to rect center (100, 100) — horizontal, hits right edge
            let (x, y) = clip_line_to_rect(400.0, 100.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 200.0).abs() < 1e-10);
            assert!((y - 100.0).abs() < 1e-10);
        }

        #[test]
        fn from_diagonal_hits_top_edge() {
            // From (100, -200) — steep vertical approach, should hit top edge
            let (x, y) = clip_line_to_rect(100.0, -200.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 100.0).abs() < 1e-10);
            assert!((y - 50.0).abs() < 1e-10);
        }

        #[test]
        fn from_45_degrees_wide_rect() {
            // Rect is wider than tall (100x50 half-dims), 45-degree approach from top-right
            // From (300, 0) to center (100, 100): dx=200, dy=-100
            // tx = 100/200 = 0.5, ty = 50/100 = 0.5 → corner hit
            let (x, y) = clip_line_to_rect(300.0, 0.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 200.0).abs() < 1e-10);
            assert!((y - 50.0).abs() < 1e-10);
        }

        #[test]
        fn degenerate_same_point() {
            let (x, y) = clip_line_to_rect(100.0, 100.0, 100.0, 100.0, 100.0, 50.0);
            assert!((x - 100.0).abs() < 1e-10);
            assert!((y - 100.0).abs() < 1e-10);
        }

        #[test]
        fn square_rect_from_diagonal() {
            // Square: center (0,0), half=50. From (100, 100): 45 degrees
            // dx=100, dy=100. tx=50/100=0.5, ty=50/100=0.5 → corner
            let (x, y) = clip_line_to_rect(100.0, 100.0, 0.0, 0.0, 50.0, 50.0);
            assert!((x - 50.0).abs() < 1e-10);
            assert!((y - 50.0).abs() < 1e-10);
        }

        #[test]
        fn negative_coordinates() {
            // Rect at (-200, -200), half=100x50. From (0, -200) — approaches from right
            let (x, y) = clip_line_to_rect(0.0, -200.0, -200.0, -200.0, 100.0, 50.0);
            assert!((x - -100.0).abs() < 1e-10);
            assert!((y - -200.0).abs() < 1e-10);
        }

        #[test]
        fn symmetry_left_right() {
            // Approaching from left and right should give opposite boundary points
            let (lx, ly) = clip_line_to_rect(-500.0, 0.0, 0.0, 0.0, 100.0, 50.0);
            let (rx, ry) = clip_line_to_rect(500.0, 0.0, 0.0, 0.0, 100.0, 50.0);
            assert!((lx - -100.0).abs() < 1e-10);
            assert!((rx - 100.0).abs() < 1e-10);
            assert!((ly - 0.0).abs() < 1e-10);
            assert!((ry - 0.0).abs() < 1e-10);
        }
    }

    mod culling_tests {
        use super::*;

        // Viewport is 800x600 for these tests.
        const W: f64 = 800.0;
        const H: f64 = 600.0;

        #[test]
        fn fully_inside_is_kept() {
            assert!(!box_outside_viewport(100.0, 100.0, 300.0, 200.0, W, H));
        }

        #[test]
        fn fully_left_is_culled() {
            // Entirely past the left edge + margin.
            assert!(box_outside_viewport(-500.0, 100.0, -100.0, 200.0, W, H));
        }

        #[test]
        fn fully_right_is_culled() {
            assert!(box_outside_viewport(1000.0, 100.0, 1200.0, 200.0, W, H));
        }

        #[test]
        fn fully_above_is_culled() {
            assert!(box_outside_viewport(100.0, -400.0, 200.0, -100.0, W, H));
        }

        #[test]
        fn fully_below_is_culled() {
            assert!(box_outside_viewport(100.0, 800.0, 200.0, 1000.0, W, H));
        }

        #[test]
        fn straddling_left_edge_is_kept() {
            // Crosses x=0, so partially visible — must not be culled.
            assert!(!box_outside_viewport(-50.0, 100.0, 50.0, 200.0, W, H));
        }

        #[test]
        fn just_outside_but_within_margin_is_kept() {
            // Right edge sits at x=-CULL_MARGIN+1, still inside the kept band.
            assert!(!box_outside_viewport(-200.0, 100.0, -CULL_MARGIN + 1.0, 200.0, W, H));
        }

        #[test]
        fn unordered_coords_normalised() {
            // Caller may pass x1<x0 (e.g. an edge drawn right-to-left); result must
            // match the ordered case.
            assert!(!box_outside_viewport(300.0, 200.0, 100.0, 100.0, W, H));
            assert!(box_outside_viewport(-100.0, 200.0, -500.0, 100.0, W, H));
        }

        fn node_at(id: &str, x: f64, y: f64) -> Node {
            // Node::new defaults to 200x100 dimensions.
            Node::new(id.to_string(), x, y, "n".to_string())
        }

        #[test]
        fn edge_inside_is_kept() {
            let a = node_at("a", 100.0, 100.0);
            let b = node_at("b", 300.0, 200.0);
            let map: HashMap<&str, &Node> = [("a", &a), ("b", &b)].into_iter().collect();
            let edge = crate::state::Edge {
                id: "e".into(),
                from_node: "a".into(),
                to_node: "b".into(),
                label: None,
            };
            assert!(!edge_outside_viewport(&map, &edge, &Camera::new(), W, H));
        }

        #[test]
        fn edge_far_offscreen_is_culled() {
            let a = node_at("a", 5000.0, 5000.0);
            let b = node_at("b", 5300.0, 5200.0);
            let map: HashMap<&str, &Node> = [("a", &a), ("b", &b)].into_iter().collect();
            let edge = crate::state::Edge {
                id: "e".into(),
                from_node: "a".into(),
                to_node: "b".into(),
                label: None,
            };
            assert!(edge_outside_viewport(&map, &edge, &Camera::new(), W, H));
        }

        #[test]
        fn edge_with_missing_endpoint_is_culled() {
            let a = node_at("a", 100.0, 100.0);
            let map: HashMap<&str, &Node> = [("a", &a)].into_iter().collect();
            let edge = crate::state::Edge {
                id: "e".into(),
                from_node: "a".into(),
                to_node: "missing".into(),
                label: None,
            };
            assert!(edge_outside_viewport(&map, &edge, &Camera::new(), W, H));
        }

        #[test]
        fn edge_crossing_viewport_is_kept() {
            // One endpoint far off-screen left, the other far off-screen right —
            // the line crosses the viewport, so the bounding box overlaps and it
            // must be kept.
            let a = node_at("a", -5000.0, 300.0);
            let b = node_at("b", 5000.0, 300.0);
            let map: HashMap<&str, &Node> = [("a", &a), ("b", &b)].into_iter().collect();
            let edge = crate::state::Edge {
                id: "e".into(),
                from_node: "a".into(),
                to_node: "b".into(),
                label: None,
            };
            assert!(!edge_outside_viewport(&map, &edge, &Camera::new(), W, H));
        }
    }
}
