use crate::app::{is_local_md_file, parse_markdown, BoardDataCtx, EditingCtx};
use crate::canvas::LoadState;
use crate::state::NodeType;
use leptos::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Memoized markdown render keyed by `node_id -> (source_content, parsed_html)`.
    /// Parsing the same `(node_id, content)` always yields the same HTML, so pan/zoom
    /// (which re-runs this overlay closure every frame but only changes transforms)
    /// reuses the cached HTML instead of re-parsing every md node. Keyed on content
    /// too, so an external edit to the node correctly invalidates the stale HTML.
    ///
    /// Held in a thread-local (not a captured `Rc`) because the Leptos view closure
    /// must be `Send`; the WASM frontend is single-threaded so this is effectively a
    /// component-lifetime cache.
    static MD_HTML_CACHE: RefCell<HashMap<String, (String, String)>> =
        RefCell::new(HashMap::new());
}

#[component]
pub fn MarkdownOverlays() -> impl IntoView {
    let board_ctx = use_context::<BoardDataCtx>().unwrap();
    let editing_ctx = use_context::<EditingCtx>().unwrap();

    move || {
        let b = board_ctx.board.get();
        let cam = board_ctx.camera.get();
        let current_editing = editing_ctx.editing_node.get();
        let md_cache = editing_ctx.md_file_cache.get();

        // Drop cache entries for nodes that no longer exist so the map can't grow
        // unbounded as md nodes are deleted over a session.
        {
            let live_ids: std::collections::HashSet<&str> =
                b.nodes.iter().map(|n| n.id.as_str()).collect();
            MD_HTML_CACHE.with(|c| {
                c.borrow_mut()
                    .retain(|id, _| live_ids.contains(id.as_str()));
            });
        }

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
                    match md_cache.get(&node.text) {
                        Some(LoadState::Loaded(c)) => c.clone(),
                        Some(LoadState::Failed) => "*Failed to load file.*".to_string(),
                        // Loading or not-yet-requested.
                        _ => "Loading...".to_string(),
                    }
                };

                // Memoized parse: reuse cached HTML when the source content for
                // this node is unchanged; otherwise (re)parse and store.
                let html_content = MD_HTML_CACHE.with(|c| {
                    let mut cache = c.borrow_mut();
                    match cache.get(&node.id) {
                        Some((cached_src, cached_html)) if cached_src == &content => {
                            cached_html.clone()
                        }
                        _ => {
                            let html = parse_markdown(&content);
                            cache.insert(node.id.clone(), (content.clone(), html.clone()));
                            html
                        }
                    }
                });

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
                             color: var(--text); font-size: 12px; line-height: 1.4; \
                             font-family: var(--mono); \
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
