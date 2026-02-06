use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use crate::app::{BoardCtx, save_board_storage};

#[component]
pub fn NodeEditor() -> impl IntoView {
    let ctx = use_context::<BoardCtx>().unwrap();

    move || {
        if let Some(node_id) = ctx.editing_node.get() {
            let b = ctx.board.get();
            let cam = ctx.camera.get();
            if let Some(node) = b.nodes.iter().find(|n| n.id == node_id) {
                let (screen_x, screen_y) = cam.world_to_screen(node.x, node.y);
                let screen_w = node.width * cam.zoom;
                let screen_h = node.height * cam.zoom;
                let font_size = (14.0 * cam.zoom).max(8.0);
                let initial_text = node.text.clone();
                let is_md = node.node_type == "md";

                if is_md {
                    let node_id_for_blur = node_id.clone();
                    let on_blur_textarea = move |ev: web_sys::FocusEvent| {
                        if let Some(target) = ev.target() {
                            if let Ok(textarea) = target.dyn_into::<web_sys::HtmlTextAreaElement>() {
                                let new_text = textarea.value();
                                let node_id_clone = node_id_for_blur.clone();
                                ctx.set_board.update(|b| {
                                    if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                        node.text = new_text;
                                    }
                                });

                                let current_board = ctx.board.get_untracked();
                                spawn_local(async move {
                                    save_board_storage(&current_board).await;
                                });
                            }
                        }
                        ctx.set_editing_node.set(None);
                    };

                    let node_id_for_keydown = node_id.clone();
                    let on_keydown_textarea = move |ev: web_sys::KeyboardEvent| {
                        if ev.key().as_str() == "Escape" {
                            if let Some(target) = ev.target() {
                                if let Ok(textarea) = target.dyn_into::<web_sys::HtmlTextAreaElement>() {
                                    let new_text = textarea.value();
                                    let node_id_clone = node_id_for_keydown.clone();
                                    ctx.set_board.update(|b| {
                                        if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                            node.text = new_text;
                                        }
                                    });

                                    let current_board = ctx.board.get_untracked();
                                    spawn_local(async move {
                                        save_board_storage(&current_board).await;
                                    });
                                }
                            }
                            ctx.set_editing_node.set(None);
                        }
                    };

                    return Some(view! {
                        <textarea
                            autofocus=true
                            style=format!(
                                "position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; \
                                 font-size: {}px; background: #020202; resize: none; \
                                 color: #ccffdd; border: 1px solid #aaffbb; outline: none; \
                                 box-sizing: border-box; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                                 text-shadow: 0 0 6px #aaffbb; padding: 8px;",
                                screen_x, screen_y, screen_w, screen_h, font_size
                            )
                            on:blur=on_blur_textarea
                            on:keydown=on_keydown_textarea
                        >{initial_text}</textarea>
                    }.into_any());
                } else {
                    let node_id_for_blur = node_id.clone();
                    let on_blur = move |ev: web_sys::FocusEvent| {
                        if let Some(target) = ev.target() {
                            if let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() {
                                let new_text = input.value();
                                let node_id_clone = node_id_for_blur.clone();
                                ctx.set_board.update(|b| {
                                    if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                        node.text = new_text;
                                    }
                                });

                                let current_board = ctx.board.get_untracked();
                                spawn_local(async move {
                                    save_board_storage(&current_board).await;
                                });
                            }
                        }
                        ctx.set_editing_node.set(None);
                    };

                    let node_id_for_keydown = node_id.clone();
                    let on_keydown = move |ev: web_sys::KeyboardEvent| {
                        match ev.key().as_str() {
                            "Enter" => {
                                if let Some(target) = ev.target() {
                                    if let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() {
                                        let new_text = input.value();
                                        let node_id_clone = node_id_for_keydown.clone();
                                        ctx.set_board.update(|b| {
                                            if let Some(node) = b.nodes.iter_mut().find(|n| n.id == node_id_clone) {
                                                node.text = new_text;
                                            }
                                        });

                                        let current_board = ctx.board.get_untracked();
                                        spawn_local(async move {
                                            save_board_storage(&current_board).await;
                                        });
                                        ctx.set_editing_node.set(None);
                                    }
                                }
                            }
                            "Escape" => {
                                ctx.set_editing_node.set(None);
                            }
                            _ => {}
                        }
                    };

                    return Some(view! {
                        <input
                            type="text"
                            value=initial_text
                            autofocus=true
                            style=format!(
                                "position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; \
                                 font-size: {}px; text-align: center; background: #020202; \
                                 color: #ccffdd; border: 1px solid #aaffbb; outline: none; \
                                 box-sizing: border-box; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                                 text-shadow: 0 0 6px #aaffbb;",
                                screen_x, screen_y, screen_w, screen_h, font_size
                            )
                            on:blur=on_blur
                            on:keydown=on_keydown
                        />
                    }.into_any());
                }
            }
        }
        None
    }
}
