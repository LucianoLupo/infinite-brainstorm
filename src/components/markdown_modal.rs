use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::app::{BoardCtx, is_local_md_file, parse_markdown, save_board_storage};

#[component]
pub fn MarkdownModal() -> impl IntoView {
    let ctx = use_context::<BoardCtx>().unwrap();

    move || {
        if let Some((node_id, is_editing)) = ctx.modal_md.get() {
            let node_id_for_edit = node_id.clone();
            let node_id_for_save = node_id.clone();
            let node_id_for_content = node_id.clone();

            Some(view! {
                <div
                    style="position: fixed; inset: 0; background: rgba(0,0,0,0.9); \
                           display: flex; align-items: center; justify-content: center; \
                           z-index: 1000;"
                    on:click=move |_| ctx.set_modal_md.set(None)
                >
                    <div
                        style="width: 90vw; max-width: 800px; height: 80vh; \
                               background: #020202; border: 1px solid #44dd66; \
                               box-shadow: 0 0 30px rgba(68, 221, 102, 0.3); \
                               padding: 24px; display: flex; flex-direction: column; \
                               font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                               color: #ccffdd; font-size: 14px; line-height: 1.6;"
                        on:click=move |ev: web_sys::MouseEvent| ev.stop_propagation()
                    >
                        <div
                            style="margin-bottom: 16px; padding-bottom: 16px; \
                                   border-bottom: 1px solid #44dd66; \
                                   display: flex; justify-content: flex-end; gap: 8px;"
                        >
                            {move || {
                                let node_id = node_id_for_edit.clone();
                                let node_id_save = node_id_for_save.clone();
                                if is_editing {
                                    view! {
                                        <button
                                            style="background: transparent; color: #66cc88; border: 1px solid #66cc88; \
                                                   padding: 8px 16px; cursor: pointer; \
                                                   font-family: inherit; font-size: 12px;"
                                            on:click=move |_| {
                                                ctx.set_modal_md.set(Some((node_id.clone(), false)));
                                            }
                                        >
                                            "Cancel"
                                        </button>
                                        <button
                                            style="background: #44dd66; color: #020202; border: none; \
                                                   padding: 8px 16px; cursor: pointer; \
                                                   font-family: inherit; font-size: 12px; font-weight: bold;"
                                            on:click=move |_| {
                                                let new_content = ctx.md_edit_text.get_untracked();
                                                let nid = node_id_save.clone();
                                                ctx.set_board.update(|b| {
                                                    if let Some(node) = b.nodes.iter_mut().find(|n| n.id == nid) {
                                                        node.text = new_content;
                                                    }
                                                });

                                                let current_board = ctx.board.get_untracked();
                                                spawn_local(async move {
                                                    save_board_storage(&current_board).await;
                                                });

                                                ctx.set_modal_md.set(Some((node_id_save.clone(), false)));
                                            }
                                        >
                                            "Save"
                                        </button>
                                    }.into_any()
                                } else {
                                    let b = ctx.board.get();
                                    let is_md_link = b.nodes.iter()
                                        .find(|n| n.id == node_id)
                                        .map(|n| n.node_type == "link" && is_local_md_file(&n.text))
                                        .unwrap_or(false);

                                    if is_md_link {
                                        view! {
                                            <span style="color: #66cc88; font-size: 11px;">"[read-only]"</span>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <button
                                                style="background: #44dd66; color: #020202; border: none; \
                                                       padding: 8px 16px; cursor: pointer; \
                                                       font-family: inherit; font-size: 12px; font-weight: bold;"
                                                on:click=move |_| {
                                                    let b = ctx.board.get_untracked();
                                                    if let Some((id, _)) = ctx.modal_md.get_untracked() {
                                                        if let Some(n) = b.nodes.iter().find(|n| n.id == id) {
                                                            ctx.set_md_edit_text.set(n.text.clone());
                                                        }
                                                        ctx.set_modal_md.set(Some((id, true)));
                                                    }
                                                }
                                            >
                                                "Edit"
                                            </button>
                                        }.into_any()
                                    }
                                }
                            }}
                        </div>
                        <div style="flex: 1; overflow-y: auto; min-height: 0;">
                            {move || {
                                let nid = node_id_for_content.clone();
                                if is_editing {
                                    view! {
                                        <textarea
                                            style="width: 100%; height: 100%; background: #020202; \
                                                   color: #ccffdd; border: 1px solid #33aa55; \
                                                   font-family: inherit; font-size: 14px; \
                                                   padding: 12px; box-sizing: border-box; resize: none; \
                                                   outline: none;"
                                            prop:value=move || ctx.md_edit_text.get()
                                            on:input=move |ev| {
                                                let value = event_target_value(&ev);
                                                ctx.set_md_edit_text.set(value);
                                            }
                                        />
                                    }.into_any()
                                } else {
                                    let b = ctx.board.get();
                                    let md_cache_content = ctx.md_file_cache.get();
                                    let content = b.nodes.iter()
                                        .find(|n| n.id == nid)
                                        .map(|n| {
                                            if n.node_type == "link" && is_local_md_file(&n.text) {
                                                md_cache_content
                                                    .get(&n.text)
                                                    .and_then(|opt: &Option<String>| opt.clone())
                                                    .unwrap_or_else(|| "Loading...".to_string())
                                            } else {
                                                n.text.clone()
                                            }
                                        })
                                        .unwrap_or_default();
                                    let html_content = parse_markdown(&content);
                                    view! {
                                        <div inner_html=html_content />
                                    }.into_any()
                                }
                            }}
                        </div>
                    </div>
                </div>
            })
        } else {
            None
        }
    }
}
