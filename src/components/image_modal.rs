use leptos::prelude::*;
use crate::app::BoardCtx;

#[component]
pub fn ImageModal() -> impl IntoView {
    let ctx = use_context::<BoardCtx>().unwrap();

    move || {
        ctx.modal_image.get().map(|image_url| {
            let set_modal_image = ctx.set_modal_image;
            view! {
                <div
                    style="position: fixed; inset: 0; background: rgba(0,0,0,0.9); \
                           display: flex; align-items: center; justify-content: center; \
                           z-index: 1000; cursor: pointer;"
                    on:click=move |_| set_modal_image.set(None)
                >
                    <button
                        style="position: fixed; top: 16px; right: 16px; z-index: 1001; \
                               background: transparent; color: #66cc88; border: 1px solid #66cc88; \
                               width: 32px; height: 32px; line-height: 1; cursor: pointer; \
                               font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                               font-size: 18px; border-radius: 4px;"
                        title="Close (Esc)"
                        on:click=move |ev: web_sys::MouseEvent| {
                            ev.stop_propagation();
                            set_modal_image.set(None);
                        }
                    >
                        "\u{00d7}"
                    </button>
                    <img
                        src=image_url
                        style="max-width: 90vw; max-height: 90vh; object-fit: contain; \
                               border: 1px solid #44dd66; box-shadow: 0 0 30px rgba(68, 221, 102, 0.3);"
                    />
                </div>
            }
        })
    }
}
