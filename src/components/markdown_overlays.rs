use leptos::prelude::*;
use crate::app::{BoardDataCtx, EditingCtx, is_local_md_file, parse_markdown};
use crate::state::NodeType;

#[component]
pub fn MarkdownOverlays() -> impl IntoView {
    let board_ctx = use_context::<BoardDataCtx>().unwrap();
    let editing_ctx = use_context::<EditingCtx>().unwrap();

    move || {
        let b = board_ctx.board.get();
        let cam = board_ctx.camera.get();
        let current_editing = editing_ctx.editing_node.get();
        let md_cache = editing_ctx.md_file_cache.get();

        b.nodes
            .iter()
            .filter(|n| {
                let is_md_node = n.node_type == NodeType::Md;
                let is_md_link = n.node_type == NodeType::Link && is_local_md_file(&n.text);
                (is_md_node || is_md_link) && current_editing.as_ref() != Some(&n.id)
            })
            .map(|node| {
                let (screen_x, screen_y) = cam.world_to_screen(node.x, node.y);
                let label_height = 16.0 * cam.zoom;

                let content = if node.node_type == NodeType::Md {
                    node.text.clone()
                } else {
                    md_cache
                        .get(&node.text)
                        .and_then(|opt: &Option<String>| opt.clone())
                        .unwrap_or_else(|| "Loading...".to_string())
                };
                let html_content = parse_markdown(&content);

                let base_w = node.width;
                let base_h = node.height - 16.0;
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
    }
}
