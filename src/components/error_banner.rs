use crate::app::EditingCtx;
use leptos::prelude::*;

/// Non-blocking banner that surfaces a board.json parse error.
///
/// Reads `load_error` from [`EditingCtx`]. While set, it renders a dismissible
/// banner explaining that the board failed to parse and that the current
/// in-memory board is being preserved. It clears automatically on the next
/// successful load (the load path resets `load_error` to `None`).
#[component]
pub fn ErrorBanner() -> impl IntoView {
    let ctx = use_context::<EditingCtx>().unwrap();
    let load_error = ctx.load_error;

    move || {
        load_error.get().map(|msg| {
            view! {
                <div style="position: fixed; top: 12px; left: 50%; transform: translateX(-50%); \
                            max-width: 80vw; z-index: 200; background: #2a0606; \
                            border: 1px solid #cc4444; border-radius: 6px; \
                            padding: 10px 14px; color: #ffb3b3; \
                            font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
                            font-size: 12px; line-height: 1.5; \
                            box-shadow: 0 4px 16px rgba(0,0,0,0.5); \
                            display: flex; align-items: flex-start; gap: 12px;">
                    <div style="flex: 1;">
                        <div style="font-weight: bold; color: #ff8888; margin-bottom: 4px;">
                            "Failed to load board.json — current board preserved"
                        </div>
                        <div style="color: #e0a0a0; word-break: break-word;">
                            {msg}
                        </div>
                    </div>
                    <button
                        style="background: transparent; border: 1px solid #cc4444; color: #ffb3b3; \
                               border-radius: 4px; cursor: pointer; padding: 2px 8px; \
                               font-family: inherit; font-size: 12px;"
                        on:click=move |_| load_error.set(None)
                    >
                        "Dismiss"
                    </button>
                </div>
            }
        })
    }
}
