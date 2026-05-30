use crate::canvas::{get_canvas_context, render_board, ImageCache, LinkPreviewCache};
use crate::components::{ErrorBanner, ImageModal, MarkdownModal, MarkdownOverlays, NodeEditor, SearchOverlay};
use crate::history::History;
use crate::interaction::{reduce, BoardAction, SideEffect};
use crate::state::{Board, Camera, Edge, LinkPreview, Node, NodeType, ResizeHandle, RESIZE_HANDLE_SIZE, MIN_NODE_WIDTH, MIN_NODE_HEIGHT};
use leptos::prelude::*;
use leptos::task::spawn_local;
use pulldown_cmark::{html, Event, Parser};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, HtmlImageElement};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &Closure<dyn Fn(JsValue)>) -> JsValue;
}

const LOCALSTORAGE_KEY: &str = "infinite-brainstorm-board";
/// Prefix for the per-board camera persistence key. The board's identity
/// (its file path in Tauri mode) is appended so distinct boards keep distinct
/// viewports; browser mode uses the bare prefix since it has a single board.
const CAMERA_KEY_PREFIX: &str = "infinite-brainstorm-camera";

fn is_tauri() -> bool {
    web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w, &JsValue::from_str("__TAURI__")).ok())
        .map(|v| !v.is_undefined())
        .unwrap_or(false)
}

/// Result of attempting to load a board from storage.
///
/// Distinguishes a missing/empty source (safe to fall back to an empty board)
/// from a present-but-invalid source (a parse error that must NOT be allowed to
/// blank out the existing board, otherwise the next save destroys the only data file).
#[derive(Debug, Clone)]
pub enum LoadOutcome {
    /// A valid board was parsed from storage.
    Loaded(Board),
    /// No board exists yet (file/key missing or empty) — a fresh `Board::default()` is appropriate.
    Absent,
    /// Storage held data but it failed to parse — carries the serde error message.
    ParseError(String),
}

/// Parse a localStorage JSON string into a [`LoadOutcome`].
///
/// Empty / whitespace-only input is treated as [`LoadOutcome::Absent`]; invalid
/// JSON yields [`LoadOutcome::ParseError`] with the serde message so we never
/// silently collapse a malformed board into an empty one.
fn parse_localstorage_board(json: &str) -> LoadOutcome {
    if json.trim().is_empty() {
        return LoadOutcome::Absent;
    }
    match serde_json::from_str::<Board>(json) {
        Ok(board) => LoadOutcome::Loaded(board),
        Err(e) => LoadOutcome::ParseError(e.to_string()),
    }
}

async fn load_board_storage() -> LoadOutcome {
    if is_tauri() {
        let result = invoke("load_board", JsValue::NULL).await;
        // The backend returns `Board::default()` (empty nodes/edges) when no file
        // exists. A genuine parse error here means the JS value the backend handed
        // back is not a Board shape — keep the existing board rather than blanking it.
        match serde_wasm_bindgen::from_value::<Board>(result) {
            Ok(board) => LoadOutcome::Loaded(board),
            Err(e) => LoadOutcome::ParseError(e.to_string()),
        }
    } else {
        match web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item(LOCALSTORAGE_KEY).ok().flatten())
        {
            Some(json) => parse_localstorage_board(&json),
            None => LoadOutcome::Absent,
        }
    }
}

/// Load the board from storage and commit it to the signals, applying the same
/// outcome handling as startup load: auto-size nodes on success, clear the load
/// error, and — crucially — leave the existing board untouched on a parse error
/// so a malformed file can't be overwritten by the next save.
///
/// Shared by both the initial-load effect and the file-watcher reload path
/// (immediate and deferred) so the three sites stay in lockstep.
async fn reload_board_into(
    set_board: WriteSignal<Board>,
    load_error: RwSignal<Option<String>>,
) {
    match load_board_storage().await {
        LoadOutcome::Loaded(mut loaded_board) => {
            auto_size_nodes(&mut loaded_board);
            load_error.set(None);
            set_board.set(loaded_board);
        }
        LoadOutcome::Absent => {
            load_error.set(None);
            set_board.set(Board::default());
        }
        LoadOutcome::ParseError(msg) => {
            // Keep the current board so the next save doesn't clobber the file
            // being edited (externally or otherwise).
            web_sys::console::error_1(&format!("Failed to parse board.json: {}", msg).into());
            load_error.set(Some(msg));
        }
    }
}

/// Fill in zero `width`/`height` on freshly-loaded nodes using text-based auto-sizing.
fn auto_size_nodes(board: &mut Board) {
    for node in &mut board.nodes {
        if node.width == 0.0 || node.height == 0.0 {
            let (w, h) = Node::auto_size(&node.text);
            if node.width == 0.0 {
                node.width = w;
            }
            if node.height == 0.0 {
                node.height = h;
            }
        }
    }
}

pub(crate) async fn save_board_storage(board: &Board) {
    if is_tauri() {
        let args = serde_wasm_bindgen::to_value(&SaveBoardArgs { board: board.clone() }).unwrap();
        let _ = invoke("save_board", args).await;
    } else if let Ok(json) = serde_json::to_string(board) {
        if let Some(storage) = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
        {
            let _ = storage.set_item(LOCALSTORAGE_KEY, &json);
        }
    }
}

/// Read the `window.localStorage` handle, if available. localStorage is present
/// in both the Tauri webview and a plain browser, so camera persistence works in
/// either mode without an IPC round-trip.
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

/// Persist the camera under `key`. Best-effort: a serialization or storage error
/// is silently ignored (a missing/quota-full Storage must not break panning).
fn save_camera_storage(key: &str, camera: &Camera) {
    if let (Some(storage), Ok(json)) = (
        local_storage(),
        serde_json::to_string(&CameraPersist::from_camera(camera)),
    ) {
        let _ = storage.set_item(key, &json);
    }
}

/// Restore a persisted camera for `key`, sanitizing a corrupt value via
/// [`CameraPersist::to_camera`]. Returns `None` if nothing was stored or the
/// stored value won't parse (in which case the caller keeps the default camera).
fn load_camera_storage(key: &str) -> Option<Camera> {
    let json = local_storage()?.get_item(key).ok().flatten()?;
    serde_json::from_str::<CameraPersist>(&json)
        .ok()
        .map(CameraPersist::to_camera)
}

/// Resolve the per-board camera key. In Tauri mode the board path is appended so
/// each `board.json` directory keeps its own viewport; in browser mode (single
/// board) the bare prefix is used.
async fn camera_storage_key() -> String {
    if is_tauri() {
        let result = invoke("get_board_path_cmd", JsValue::NULL).await;
        if let Some(path) = result.as_string() {
            return format!("{}:{}", CAMERA_KEY_PREFIX, path);
        }
    }
    CAMERA_KEY_PREFIX.to_string()
}

/// Trailing-edge debounce window for the persistence sink, in milliseconds.
///
/// Bursts of mutations (e.g. a drag emitting many intermediate states, or rapid
/// keystrokes) are coalesced into a single disk write that fires this long after
/// the last [`RequestSave::call`].
const SAVE_DEBOUNCE_MS: u32 = 220;

/// A `Copy` handle to the centralized, debounced persistence sink.
///
/// All mutation sites call [`RequestSave::call`] instead of invoking
/// `save_board_storage` directly. Calls mark the board dirty and (re)arm a single
/// trailing-edge timer; the actual write reads the latest board state at flush
/// time, so coalesced bursts persist exactly the final state once.
#[derive(Clone, Copy)]
pub struct RequestSave {
    // `Rc<dyn Fn()>` is `!Send`/`!Sync`, so it lives in thread-local arena storage
    // (`LocalStorage`). This is sound in the single-threaded CSR/WASM runtime.
    inner: StoredValue<Rc<dyn Fn()>, LocalStorage>,
}

impl RequestSave {
    /// Mark the board dirty and (re)schedule the single trailing-edge write.
    pub fn call(&self) {
        let f = self.inner.get_value();
        f();
    }
}

/// Build the debounced persistence sink.
///
/// Returns a [`RequestSave`] whose every call cancels any pending timer and arms
/// a fresh trailing-edge [`gloo_timers::callback::Timeout`]. When the timer fires
/// it reads `board` untracked, persists it, and clears `local_edit_pending`.
/// `local_edit_pending` is raised on every call so the file watcher (P1.4) can
/// distinguish our own in-flight edits from genuine external changes.
fn make_request_save(
    board: ReadSignal<Board>,
    local_edit_pending: RwSignal<bool>,
) -> RequestSave {
    // Holds the live timer so a subsequent call drops (cancels) it before arming
    // a new one — this is what coalesces a burst into one write.
    let pending: Rc<RefCell<Option<gloo_timers::callback::Timeout>>> =
        Rc::new(RefCell::new(None));

    let sink: Rc<dyn Fn()> = Rc::new(move || {
        local_edit_pending.set(true);
        let pending_for_timer = pending.clone();
        let timeout = gloo_timers::callback::Timeout::new(SAVE_DEBOUNCE_MS, move || {
            // Clear our own handle first so the closure can't keep the Timeout
            // alive after it fires.
            pending_for_timer.borrow_mut().take();
            let current_board = board.get_untracked();
            spawn_local(async move {
                save_board_storage(&current_board).await;
                local_edit_pending.set(false);
            });
        });
        // Dropping the previous Timeout (if any) cancels it.
        *pending.borrow_mut() = Some(timeout);
    });

    RequestSave {
        inner: StoredValue::new_local(sink),
    }
}

/// A single point on the undo/redo timeline: the full board plus the node
/// selection at that moment. Snapshotting selection (not just the board) lets
/// undo/redo *restore* what was selected instead of clearing it (F115).
pub type Snapshot = (Board, HashSet<String>);

/// Shared, non-reactive undo/redo stack. Mutations don't need reactivity, so it
/// lives behind `Rc<RefCell<..>>` rather than a signal.
type BoardHistory = Rc<RefCell<History<Snapshot>>>;

/// `Copy` handle that routes every board mutation through one place.
///
/// `apply` is the single entry point: it snapshots history exactly once, runs the
/// pure [`reduce`], commits the new board + selection to the signals, and
/// dispatches the returned [`SideEffect`]s (asset deletion, debounced save). This
/// is what collapses the previously-scattered `history.push` calls into one and
/// fixes undo dropping in-progress edits (F52/F109).
///
/// The history `Rc` is `!Send`, so — like [`RequestSave`] — it is parked in
/// thread-local `LocalStorage` arena storage, which keeps this struct `Copy` and
/// cheap to stash in [`BoardCtx`] for the editor components to dispatch through.
#[derive(Clone, Copy)]
pub struct Dispatcher {
    board: ReadSignal<Board>,
    set_board: WriteSignal<Board>,
    selected_nodes: ReadSignal<HashSet<String>>,
    set_selected_nodes: WriteSignal<HashSet<String>>,
    set_selected_edge: WriteSignal<Option<String>>,
    history: StoredValue<BoardHistory, LocalStorage>,
    request_save: RequestSave,
}

impl Dispatcher {
    /// Capture the current `(board, node selection)` onto the undo stack.
    ///
    /// Exposed for the deferred-snapshot path (F114): drag/resize call this once on
    /// the first actual movement (not on mouse-down) so a plain click never creates
    /// a junk undo entry. [`apply`](Self::apply) calls it internally for one-shot
    /// actions.
    pub fn snapshot(&self) {
        let snap = (self.board.get_untracked(), self.selected_nodes.get_untracked());
        self.history.get_value().borrow_mut().push(snap);
    }

    /// Run the side effects a [`reduce`] call produced.
    fn run_effects(&self, effects: Vec<SideEffect>) {
        let mut asset_paths = Vec::new();
        let mut wants_save = false;
        for effect in effects {
            match effect {
                SideEffect::DeleteAsset(path) => asset_paths.push(path),
                SideEffect::RequestSave => wants_save = true,
            }
        }

        if asset_paths.is_empty() {
            if wants_save {
                self.request_save.call();
            }
        } else {
            // Asset deletion is async (Tauri filesystem); save only after the
            // deletions are issued so a single coalesced write reflects the result.
            let request_save = self.request_save;
            spawn_local(async move {
                if is_tauri() {
                    for path in asset_paths {
                        #[derive(Serialize)]
                        struct DeleteAssetArgs {
                            path: String,
                        }
                        let args = serde_wasm_bindgen::to_value(&DeleteAssetArgs {
                            path: path.clone(),
                        })
                        .unwrap();
                        let _ = invoke("delete_asset", args).await;
                    }
                }
                if wants_save {
                    request_save.call();
                }
            });
        }
    }

    /// Commit the reduced board + optional new selection and run side effects,
    /// WITHOUT taking a new history snapshot. Used by continuous gestures
    /// (drag/resize) where [`snapshot`](Self::snapshot) was already taken on the
    /// first movement.
    fn commit(&self, action: BoardAction, new_selection: Option<HashSet<String>>) {
        let (next_board, effects) = reduce(self.board.get_untracked(), action);
        self.set_board.set(next_board);
        if let Some(selection) = new_selection {
            self.set_selected_nodes.set(selection);
        }
        self.run_effects(effects);
    }

    /// The single mutation entry point: snapshot once, reduce, commit, dispatch.
    ///
    /// `new_selection` replaces the node selection when `Some` (e.g. select the
    /// freshly created/pasted node, or clear selection after a delete); pass `None`
    /// to leave selection untouched.
    pub fn apply(&self, action: BoardAction, new_selection: Option<HashSet<String>>) {
        self.snapshot();
        self.commit(action, new_selection);
    }

    /// Undo the last mutation, restoring both the board and the selection that was
    /// live when the snapshot was taken (F115). Returns `true` if anything changed.
    pub fn undo(&self) -> bool {
        let current = (self.board.get_untracked(), self.selected_nodes.get_untracked());
        if let Some((board, selection)) = self.history.get_value().borrow_mut().undo(current) {
            self.set_board.set(board);
            self.set_selected_nodes.set(selection);
            self.set_selected_edge.set(None);
            self.request_save.call();
            true
        } else {
            false
        }
    }

    /// Redo the last undone mutation, restoring board + selection. Returns `true`
    /// if anything changed.
    pub fn redo(&self) -> bool {
        let current = (self.board.get_untracked(), self.selected_nodes.get_untracked());
        if let Some((board, selection)) = self.history.get_value().borrow_mut().redo(current) {
            self.set_board.set(board);
            self.set_selected_nodes.set(selection);
            self.set_selected_edge.set(None);
            self.request_save.call();
            true
        } else {
            false
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SaveBoardArgs {
    board: Board,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PasteImageResult {
    path: String,
    width: u32,
    height: u32,
}

#[derive(Serialize, Deserialize)]
struct FetchLinkPreviewArgs {
    url: String,
}

#[derive(Serialize, Deserialize)]
struct ReadMarkdownFileArgs {
    path: String,
}

#[derive(Clone, Default)]
struct DragState {
    is_dragging: bool,
    is_box_selecting: bool,
    start_x: f64,
    start_y: f64,
    node_start_positions: HashMap<String, (f64, f64)>,
    /// Whether an undo snapshot has been taken for this drag yet. Deferred to the
    /// first actual movement (not mouse-down) so a plain click never creates a junk
    /// undo entry (F114).
    snapshotted: bool,
}

#[derive(Clone)]
struct PanState {
    is_panning: bool,
    start_x: f64,
    start_y: f64,
    camera_start_x: f64,
    camera_start_y: f64,
}

impl Default for PanState {
    fn default() -> Self {
        Self {
            is_panning: false,
            start_x: 0.0,
            start_y: 0.0,
            camera_start_x: 0.0,
            camera_start_y: 0.0,
        }
    }
}

#[derive(Clone, Default)]
struct EdgeCreationState {
    is_creating: bool,
    from_node_id: Option<String>,
    current_x: f64,
    current_y: f64,
}

#[derive(Clone, Default)]
struct ResizeState {
    is_resizing: bool,
    node_id: Option<String>,
    handle: Option<ResizeHandle>,
    start_mouse_x: f64,
    start_mouse_y: f64,
    original_x: f64,
    original_y: f64,
    original_width: f64,
    original_height: f64,
    /// Whether an undo snapshot has been taken for this resize yet. Deferred to the
    /// first actual movement so a click on a handle without dragging creates no junk
    /// undo entry (F114).
    snapshotted: bool,
}

pub(crate) fn parse_markdown(md: &str) -> String {
    // Sanitize: map any raw-HTML events to escaped Text so author-controlled
    // markup (e.g. `<img onerror=...>`) is rendered as literal text rather than
    // reaching the inner_html sink as active HTML. push_html HTML-escapes Text
    // events, so the angle brackets show and no attributes/handlers execute.
    let parser = Parser::new(md).map(|event| match event {
        Event::Html(html) | Event::InlineHtml(html) => Event::Text(html),
        other => other,
    });
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Check if a path points to a local .md file (not HTTP URL)
pub fn is_local_md_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    if !path_lower.ends_with(".md") {
        return false;
    }
    path.starts_with('/') || path.starts_with("file://") || path.starts_with('~')
}

/// Extract the lowercased host portion of an `http(s)://` URL, or `None` if the
/// URL is not http(s) or has no host. Pure string parsing — no allocation of a
/// full URL parser, kept small so it is easy to unit-test.
fn http_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    // Host ends at the first '/', '?', '#', or end of string. Strip userinfo
    // ("user:pass@host") and the port (":443") if present.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host_port.starts_with('[') {
        // IPv6 literal: "[::1]:443" -> "[::1]"
        host_port.split(']').next().map(|h| format!("{}]", h)).unwrap_or_else(|| host_port.to_string())
    } else {
        host_port.split(':').next().unwrap_or(host_port).to_string()
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// Decide whether `url` points at a *clearly public* host that is safe to
/// auto-fetch a link preview for on board load. This is a conservative
/// allowlist policy: we only auto-fetch hostnames that look like registrable
/// public domains. Bare IP literals, `localhost`, and internal/private TLDs
/// (`.local`, `.internal`, `.lan`, `.home`, `.corp`, `.intranet`) are NOT
/// auto-fetched — the backend SSRF guard is the hard enforcement, but this
/// stops a board.json link node from silently driving any request to an
/// internal host on load. The backend remains the source of truth; explicit
/// user interaction can still trigger a fetch through the normal command path.
pub fn is_public_http_host(url: &str) -> bool {
    let host = match http_host(url) {
        Some(h) => h,
        None => return false,
    };

    // Reject IPv6 literals outright (private classification can't be done by
    // simple string match; never auto-fetch them).
    if host.starts_with('[') {
        return false;
    }

    // Reject bare IPv4 literals: if every dot-separated label is numeric, it's
    // an IP, not a hostname — don't auto-fetch (backend still validates).
    let labels: Vec<&str> = host.split('.').collect();
    let all_numeric = !labels.is_empty()
        && labels.iter().all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()));
    if all_numeric {
        return false;
    }

    // Reject localhost and known internal/private TLDs.
    if host == "localhost" {
        return false;
    }
    const INTERNAL_SUFFIXES: [&str; 7] = [
        ".local",
        ".localhost",
        ".internal",
        ".intranet",
        ".lan",
        ".home",
        ".corp",
    ];
    if INTERNAL_SUFFIXES.iter().any(|s| host.ends_with(s)) {
        return false;
    }

    // Require a registrable domain: at least one dot with non-empty labels on
    // both sides (e.g. "example.com"). A single-label host ("intranet-box")
    // is treated as internal and not auto-fetched.
    labels.len() >= 2 && labels.iter().all(|l| !l.is_empty())
}

fn intersects_box(node: &Node, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
    let node_right = node.x + node.width;
    let node_bottom = node.y + node.height;
    !(node.x > max_x || node_right < min_x || node.y > max_y || node_bottom < min_y)
}

fn point_near_line(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64, threshold: f64) -> bool {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt() < threshold;
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let closest_x = x1 + t * dx;
    let closest_y = y1 + t * dy;
    let dist = ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt();
    dist < threshold
}

/// Case-insensitive substring match of `query` against a node's searchable text:
/// its body text, any of its tags, and its status. An empty/whitespace-only query
/// matches nothing (so a blank search box doesn't select every node).
///
/// Pure and allocation-light so the search overlay can filter a 100+ node board on
/// every keystroke without touching the DOM or signals.
pub fn node_matches_query(node: &Node, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return false;
    }
    if node.text.to_lowercase().contains(&q) {
        return true;
    }
    if node.tags.iter().any(|t| t.to_lowercase().contains(&q)) {
        return true;
    }
    if let Some(status) = &node.status {
        if status.to_lowercase().contains(&q) {
            return true;
        }
    }
    false
}

/// Axis-aligned bounding box `(min_x, min_y, max_x, max_y)` enclosing every node
/// (each node spans `x..x+width`, `y..y+height`). Returns `None` for an empty
/// slice. Pure so fit-to-view math is unit-testable without a canvas.
pub fn nodes_bounding_box(nodes: &[Node]) -> Option<(f64, f64, f64, f64)> {
    let mut iter = nodes.iter();
    let first = iter.next()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x + first.width;
    let mut max_y = first.y + first.height;
    for n in iter {
        min_x = min_x.min(n.x);
        min_y = min_y.min(n.y);
        max_x = max_x.max(n.x + n.width);
        max_y = max_y.max(n.y + n.height);
    }
    Some((min_x, min_y, max_x, max_y))
}

/// Compute a [`Camera`] that frames `bbox` within a `canvas_w` x `canvas_h`
/// viewport, leaving ~`margin_frac` (e.g. 0.1 = 10%) padding on every side. The
/// zoom fits the larger relative dimension and is clamped to the app's `0.1..=5.0`
/// range; the camera origin is positioned so the padded box is centered.
///
/// Pure: takes the box + viewport, returns a Camera — no DOM, easy to test.
pub fn fit_camera(
    bbox: (f64, f64, f64, f64),
    canvas_w: f64,
    canvas_h: f64,
    margin_frac: f64,
) -> Camera {
    let (min_x, min_y, max_x, max_y) = bbox;
    let box_w = (max_x - min_x).max(1.0);
    let box_h = (max_y - min_y).max(1.0);

    // Pad the box on all sides so nodes don't touch the viewport edge.
    let pad_x = box_w * margin_frac;
    let pad_y = box_h * margin_frac;
    let padded_w = box_w + pad_x * 2.0;
    let padded_h = box_h + pad_y * 2.0;

    // Guard a degenerate viewport (e.g. canvas not yet laid out) so we never
    // divide by zero or produce a non-finite zoom.
    let cw = if canvas_w.is_finite() && canvas_w > 0.0 { canvas_w } else { 1.0 };
    let ch = if canvas_h.is_finite() && canvas_h > 0.0 { canvas_h } else { 1.0 };

    let zoom = (cw / padded_w).min(ch / padded_h).clamp(0.1, 5.0);

    // Center the padded box: world coords of the viewport's top-left corner.
    let center_x = (min_x + max_x) / 2.0;
    let center_y = (min_y + max_y) / 2.0;
    let cam_x = center_x - (cw / zoom) / 2.0;
    let cam_y = center_y - (ch / zoom) / 2.0;

    Camera { x: cam_x, y: cam_y, zoom }
}

/// Serializable camera snapshot persisted to localStorage so a reopened board
/// restores its last pan/zoom (F105). Kept separate from [`Camera`] (which is not
/// `Serialize`) to avoid widening the shared type's derives.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct CameraPersist {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl CameraPersist {
    pub fn from_camera(c: &Camera) -> Self {
        Self { x: c.x, y: c.y, zoom: c.zoom }
    }

    /// Rebuild a [`Camera`], sanitizing a corrupt/hand-edited persisted zoom: a
    /// non-finite or out-of-range zoom falls back to `1.0` so a bad localStorage
    /// value can't strand the viewport at an unusable scale.
    pub fn to_camera(self) -> Camera {
        let zoom = if self.zoom.is_finite() && (0.1..=5.0).contains(&self.zoom) {
            self.zoom
        } else {
            1.0
        };
        let x = if self.x.is_finite() { self.x } else { 0.0 };
        let y = if self.y.is_finite() { self.y } else { 0.0 };
        Camera { x, y, zoom }
    }
}

#[derive(Clone, Copy)]
pub struct BoardCtx {
    pub board: ReadSignal<Board>,
    pub set_board: WriteSignal<Board>,
    pub camera: ReadSignal<Camera>,
    pub set_camera: WriteSignal<Camera>,
    pub selected_nodes: ReadSignal<HashSet<String>>,
    pub set_selected_nodes: WriteSignal<HashSet<String>>,
    pub selected_edge: ReadSignal<Option<String>>,
    pub set_selected_edge: WriteSignal<Option<String>>,
    pub editing_node: ReadSignal<Option<String>>,
    pub set_editing_node: WriteSignal<Option<String>>,
    pub modal_image: ReadSignal<Option<String>>,
    pub set_modal_image: WriteSignal<Option<String>>,
    pub modal_md: ReadSignal<Option<(String, bool)>>,
    pub set_modal_md: WriteSignal<Option<(String, bool)>>,
    pub md_edit_text: ReadSignal<String>,
    pub set_md_edit_text: WriteSignal<String>,
    pub md_file_cache: ReadSignal<HashMap<String, Option<String>>>,
    /// Most recent board.json parse error (if any). Set on a failed load so the
    /// error banner can surface it; cleared on the next successful load.
    pub load_error: RwSignal<Option<String>>,
    /// Centralized debounced persistence sink. Call this after mutating the board
    /// signal instead of invoking `save_board_storage` directly.
    pub request_save: RequestSave,
    /// True while a debounced local write is queued or in flight. The file watcher
    /// can check this to avoid reloading over the user's own pending edits.
    pub local_edit_pending: RwSignal<bool>,
    /// Single mutation entry point. Editor components dispatch text edits through
    /// this so each commit snapshots undo history (fixes undo dropping typed text).
    pub dispatch: Dispatcher,
    /// Search overlay query (P2.4 / F99). `Some(query)` while open; `None` closed.
    pub search_query: ReadSignal<Option<String>>,
    pub set_search_query: WriteSignal<Option<String>>,
}

#[component]
pub fn App() -> impl IntoView {
    let (board, set_board) = signal(Board::default());
    let (camera, set_camera) = signal(Camera::new());
    let (selected_nodes, set_selected_nodes) = signal::<HashSet<String>>(HashSet::new());
    let (selected_edge, set_selected_edge) = signal::<Option<String>>(None);
    let (drag_state, set_drag_state) = signal(DragState::default());
    let (pan_state, set_pan_state) = signal(PanState::default());
    let (editing_node, set_editing_node) = signal::<Option<String>>(None);
    let (edge_creation, set_edge_creation) = signal(EdgeCreationState::default());
    let (resize_state, set_resize_state) = signal(ResizeState::default());
    let (cursor_style, set_cursor_style) = signal("crosshair".to_string());
    let (last_mouse_world_pos, set_last_mouse_world_pos) = signal((0.0f64, 0.0f64));
    let (selection_box, set_selection_box) = signal::<Option<(f64, f64, f64, f64)>>(None);
    let (modal_image, set_modal_image) = signal::<Option<String>>(None);
    let (modal_md, set_modal_md) = signal::<Option<(String, bool)>>(None); // (node_id, is_editing)
    let (md_edit_text, set_md_edit_text) = signal::<String>(String::new()); // Separate signal to avoid re-render on typing
    let (node_clipboard, set_node_clipboard) = signal::<Option<(Vec<Node>, Vec<Edge>)>>(None);
    // Search overlay (P2.4 / F99): `Some(query)` while the Cmd/Ctrl+F overlay is
    // open; `None` when closed. Matches are reflected into `selected_nodes` so they
    // render with the existing selection highlight.
    let (search_query, set_search_query) = signal::<Option<String>>(None);
    // Resolved per-board key for camera persistence. Defaults to the browser key
    // and is refined to the Tauri board-path key once it resolves on startup.
    let camera_key: StoredValue<String> = StoredValue::new(CAMERA_KEY_PREFIX.to_string());

    // Undo/redo history - using Rc<RefCell> since mutations don't need reactivity.
    // Snapshots are (Board, node selection) so undo/redo restore the selection too.
    let history: BoardHistory = Rc::new(RefCell::new(History::new(100)));

    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let file_input_ref = NodeRef::<leptos::html::Input>::new();
    let image_cache: ImageCache = Rc::new(RefCell::new(HashMap::new()));
    let image_cache_for_render = image_cache.clone();
    let image_cache_for_load = image_cache.clone();
    let image_cache_for_link_preview = image_cache.clone();
    let image_cache_for_modal = image_cache.clone();
    let link_preview_cache: LinkPreviewCache = Rc::new(RefCell::new(HashMap::new()));
    let link_preview_cache_for_render = link_preview_cache.clone();
    let link_preview_cache_for_fetch = link_preview_cache.clone();
    // Markdown file cache stored as a signal (for local .md files in link nodes)
    let (md_file_cache, set_md_file_cache) = signal::<HashMap<String, Option<String>>>(HashMap::new());
    let (image_load_trigger, set_image_load_trigger) = signal(0u32);
    let (link_preview_trigger, set_link_preview_trigger) = signal(0u32);
    let load_error = RwSignal::<Option<String>>::new(None);
    let local_edit_pending = RwSignal::<bool>::new(false);
    // Set when an external board-changed event arrives while a local interaction
    // (drag/resize/edge-creation/text-edit) or a queued save is in flight. The
    // reload is deferred and flushed by an effect once the interaction settles,
    // so the watcher can never clobber an edit mid-gesture (P1.4 / F50).
    let pending_external_reload = RwSignal::<bool>::new(false);
    let request_save = make_request_save(board, local_edit_pending);

    // Debounced camera persistence (F105). Pan/zoom end-points call this; a burst
    // of wheel ticks coalesces into one localStorage write 200ms after the last
    // change. The closure reads the freshest camera + resolved key at flush time.
    let persist_camera: StoredValue<Rc<dyn Fn()>, LocalStorage> = {
        let pending: Rc<RefCell<Option<gloo_timers::callback::Timeout>>> =
            Rc::new(RefCell::new(None));
        let sink: Rc<dyn Fn()> = Rc::new(move || {
            let pending_for_timer = pending.clone();
            let timeout = gloo_timers::callback::Timeout::new(200, move || {
                pending_for_timer.borrow_mut().take();
                let cam = camera.get_untracked();
                let key = camera_key.get_value();
                save_camera_storage(&key, &cam);
            });
            *pending.borrow_mut() = Some(timeout);
        });
        StoredValue::new_local(sink)
    };
    let persist_camera_now = move || {
        (persist_camera.get_value())();
    };

    // Single mutation entry point shared by handlers and editor components.
    let dispatch = Dispatcher {
        board,
        set_board,
        selected_nodes,
        set_selected_nodes,
        set_selected_edge,
        history: StoredValue::new_local(history),
        request_save,
    };

    provide_context(BoardCtx {
        board,
        set_board,
        camera,
        set_camera,
        selected_nodes,
        set_selected_nodes,
        selected_edge,
        set_selected_edge,
        editing_node,
        set_editing_node,
        modal_image,
        set_modal_image,
        modal_md,
        set_modal_md,
        md_edit_text,
        set_md_edit_text,
        md_file_cache,
        load_error,
        request_save,
        local_edit_pending,
        dispatch,
        search_query,
        set_search_query,
    });

    // Load board on startup (with small delay to ensure Tauri is ready).
    // Camera persistence (F105) is restored ONLY here — the file-watcher reload
    // path deliberately leaves the live viewport alone so an external board edit
    // never yanks the user's pan/zoom.
    Effect::new(move || {
        spawn_local(async move {
            // Small delay to ensure Tauri's __TAURI__ is injected
            gloo_timers::future::TimeoutFuture::new(50).await;
            // Resolve the per-board camera key before restoring so subsequent
            // pan/zoom writes land under the right (board-specific) key.
            let key = camera_storage_key().await;
            camera_key.set_value(key.clone());
            if let Some(restored) = load_camera_storage(&key) {
                set_camera.set(restored);
            }
            reload_board_into(set_board, load_error).await;
        });
    });

    // True while a local interaction is mid-flight and a watcher reload would
    // clobber the user's in-progress edit: an active drag/resize/edge-creation,
    // inline text editing, or a queued/in-flight local save (P1.4 / F50). Read
    // untracked so callers don't accidentally subscribe.
    let interaction_in_flight = move || {
        drag_state.get_untracked().is_dragging
            || resize_state.get_untracked().is_resizing
            || edge_creation.get_untracked().is_creating
            || editing_node.get_untracked().is_some()
            || local_edit_pending.get_untracked()
    };

    // File watcher listener (Tauri only)
    // Note: Backend skips emissions for our own saves (content-hash match). Any
    // event that reaches here is a genuine external change — but we still defer
    // applying it if a local interaction is in flight so we don't overwrite an
    // edit the user is actively making.
    Effect::new(move || {
        if !is_tauri() {
            return; // Skip file watching in browser mode
        }

        let handler = Closure::new(move |_event: JsValue| {
            if interaction_in_flight() {
                // Defer: record that an external change is waiting and let the
                // flush effect apply it once the interaction settles. We do NOT
                // reload now, or we'd clobber the in-progress edit (F50).
                web_sys::console::log_1(
                    &"External board change during interaction — deferring reload".into(),
                );
                pending_external_reload.set(true);
                return;
            }

            web_sys::console::log_1(&"External board change detected, reloading...".into());
            spawn_local(async move {
                reload_board_into(set_board, load_error).await;
            });
        });

        spawn_local(async move {
            let _ = listen("board-changed", &handler).await;
            handler.forget();
        });
    });

    // Deferred-reload flush: when an external change was deferred during an
    // interaction, re-run the reload once the interaction settles. This effect
    // subscribes (tracked) to every interaction signal plus the pending flag, so
    // it re-evaluates whenever any of them change — e.g. on mouse-up ending a
    // drag, on edit-commit clearing `editing_node`, or when the debounced save
    // clears `local_edit_pending`.
    Effect::new(move || {
        // Tracked reads: re-run when any interaction state OR the pending flag
        // changes.
        let pending = pending_external_reload.get();
        let busy = drag_state.get().is_dragging
            || resize_state.get().is_resizing
            || edge_creation.get().is_creating
            || editing_node.get().is_some()
            || local_edit_pending.get();

        if pending && !busy {
            pending_external_reload.set(false);
            spawn_local(async move {
                reload_board_into(set_board, load_error).await;
            });
        }
    });

    // Image loading effect
    Effect::new({
        let image_cache = image_cache_for_load.clone();
        move || {
            let current_board = board.get();

            for node in &current_board.nodes {
                if node.node_type == NodeType::Image && !node.text.is_empty() {
                    let url = node.text.clone();

                    let needs_load = {
                        let cache = image_cache.borrow();
                        !cache.contains_key(&url)
                    };

                    if needs_load {
                        // Mark as loading
                        web_sys::console::log_1(&format!("Loading image: {}", url).into());
                        image_cache.borrow_mut().insert(url.clone(), None);

                        let cache_for_async = image_cache.clone();
                        let url_for_async = url.clone();
                        let trigger = set_image_load_trigger;

                        spawn_local(async move {
                            // Determine image source URL
                            let image_src = if url_for_async.starts_with("http://") || url_for_async.starts_with("https://") {
                                // HTTP URL - use directly
                                url_for_async.clone()
                            } else if is_tauri() {
                                // Local file - use Tauri command to convert to base64
                                #[derive(Serialize)]
                                struct ReadImageArgs { path: String }
                                let args = serde_wasm_bindgen::to_value(&ReadImageArgs { path: url_for_async.clone() }).unwrap();
                                match invoke("read_image_base64", args).await.as_string() {
                                    Some(data_url) => data_url,
                                    None => {
                                        web_sys::console::error_1(&format!("Failed to read image: {}", url_for_async).into());
                                        return;
                                    }
                                }
                            } else {
                                // Browser mode - can't load local files
                                web_sys::console::error_1(&"Local files not supported in browser mode".into());
                                return;
                            };

                            // Create image element and load
                            let img = HtmlImageElement::new().unwrap();
                            let url_for_closure = url_for_async.clone();
                            let cache_for_onload = cache_for_async.clone();

                            let onload_ref = Closure::wrap(Box::new({
                                let img = img.clone();
                                let cache = cache_for_onload.clone();
                                let url = url_for_closure.clone();
                                move || {
                                    web_sys::console::log_1(&format!("Image loaded successfully: {}", url).into());
                                    cache.borrow_mut().insert(url.clone(), Some(img.clone()));
                                    trigger.update(|n| *n = n.wrapping_add(1));
                                }
                            }) as Box<dyn Fn()>);

                            img.set_onload(Some(onload_ref.as_ref().unchecked_ref()));
                            onload_ref.forget();

                            let onerror = Closure::wrap(Box::new({
                                let url = url_for_async.clone();
                                move || {
                                    web_sys::console::error_1(&format!("Image load FAILED: {}", url).into());
                                }
                            }) as Box<dyn Fn()>);

                            img.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                            onerror.forget();

                            img.set_src(&image_src);
                        });
                    }
                }
            }
        }
    });

    // Link preview fetching effect
    Effect::new({
        let link_cache = link_preview_cache_for_fetch.clone();
        let image_cache = image_cache_for_link_preview.clone();
        move || {
            let current_board = board.get();

            for node in &current_board.nodes {
                if node.node_type == NodeType::Link && !node.text.is_empty() {
                    let url = node.text.clone();

                    // SSRF gate: only auto-fetch previews for clearly-public
                    // hosts on board load. Internal hosts / IP literals /
                    // localhost are skipped so a board.json link node can't
                    // silently drive a server-side request to an internal
                    // address. The backend command remains the hard guard for
                    // any explicit (user-triggered) fetch.
                    if !is_public_http_host(&url) {
                        continue;
                    }

                    let needs_fetch = {
                        let cache = link_cache.borrow();
                        !cache.contains_key(&url)
                    };

                    if needs_fetch {
                        // Mark as loading
                        link_cache.borrow_mut().insert(url.clone(), None);

                        let cache_for_result = link_cache.clone();
                        let image_cache_for_result = image_cache.clone();
                        let trigger = set_link_preview_trigger;
                        let img_trigger = set_image_load_trigger;

                        spawn_local(async move {
                            let args = serde_wasm_bindgen::to_value(&FetchLinkPreviewArgs { url: url.clone() }).unwrap();
                            let result = invoke("fetch_link_preview", args).await;

                            if let Ok(preview) = serde_wasm_bindgen::from_value::<LinkPreview>(result) {
                                // If preview has an image, start loading it
                                if let Some(ref image_url) = preview.image {
                                    let img_url = image_url.clone();
                                    let needs_img_load = {
                                        let cache = image_cache_for_result.borrow();
                                        !cache.contains_key(&img_url)
                                    };

                                    if needs_img_load {
                                        image_cache_for_result.borrow_mut().insert(img_url.clone(), None);

                                        let img = HtmlImageElement::new().unwrap();
                                        let cache_for_onload = image_cache_for_result.clone();
                                        let url_for_closure = img_url.clone();

                                        let onload = Closure::wrap(Box::new({
                                            let img = img.clone();
                                            let cache = cache_for_onload.clone();
                                            let url = url_for_closure.clone();
                                            move || {
                                                cache.borrow_mut().insert(url.clone(), Some(img.clone()));
                                                img_trigger.update(|n| *n = n.wrapping_add(1));
                                            }
                                        }) as Box<dyn Fn()>);

                                        img.set_onload(Some(onload.as_ref().unchecked_ref()));
                                        onload.forget();
                                        img.set_src(&img_url);
                                    }
                                }

                                cache_for_result.borrow_mut().insert(url, Some(preview));
                                trigger.update(|n| *n = n.wrapping_add(1));
                            }
                        });
                    }
                }
            }
        }
    });

    // Markdown file fetching effect (for local .md files in link nodes)
    Effect::new(move || {
        let current_board = board.get();
        let current_cache = md_file_cache.get();

        for node in &current_board.nodes {
            if node.node_type == NodeType::Link && is_local_md_file(&node.text) {
                let path = node.text.clone();

                if !current_cache.contains_key(&path) {
                    // Mark as loading
                    set_md_file_cache.update(|c| {
                        c.insert(path.clone(), None);
                    });

                    spawn_local(async move {
                        let args = serde_wasm_bindgen::to_value(&ReadMarkdownFileArgs { path: path.clone() }).unwrap();
                        let result = invoke("read_markdown_file", args).await;

                        let content = result.as_string();
                        set_md_file_cache.update(|c| {
                            c.insert(path, content);
                        });
                    });
                }
            }
        }
    });

    // Render coalescer (P2.1): instead of drawing synchronously on every signal
    // change (once per mousemove during a drag), each change marks the canvas
    // dirty and schedules a SINGLE requestAnimationFrame. The rAF callback reads
    // the freshest signal values via `get_untracked()` and renders once per
    // frame, so a burst of mutations within one frame collapses to one draw.
    let render_scheduled: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    // Holds the rAF callback so it isn't dropped while the browser owns it.
    let render_closure: Rc<RefCell<Option<Closure<dyn FnMut()>>>> =
        Rc::new(RefCell::new(None));

    {
        let render_scheduled = render_scheduled.clone();
        let render_closure_store = render_closure.clone();
        let image_cache_for_render = image_cache_for_render.clone();
        let link_preview_cache_for_render = link_preview_cache_for_render.clone();

        let closure = Closure::wrap(Box::new(move || {
            // Allow the next frame to be scheduled even if this render bails early.
            render_scheduled.set(false);

            let current_board = board.get_untracked();
            let current_camera = camera.get_untracked();
            let current_selected = selected_nodes.get_untracked();
            let current_selected_edge = selected_edge.get_untracked();
            let current_editing = editing_node.get_untracked();
            let current_edge_creation = edge_creation.get_untracked();
            let current_selection_box = selection_box.get_untracked();

            if let Some(canvas) = canvas_ref.get_untracked() {
                let canvas_el: &HtmlCanvasElement = &canvas;

                let rect = canvas_el.get_bounding_client_rect();
                let display_width = rect.width() as u32;
                let display_height = rect.height() as u32;

                if canvas_el.width() != display_width {
                    canvas_el.set_width(display_width);
                }
                if canvas_el.height() != display_height {
                    canvas_el.set_height(display_height);
                }

                if let Ok(ctx) = get_canvas_context(canvas_el) {
                    render_board(
                        &ctx,
                        canvas_el,
                        &current_board,
                        &current_camera,
                        &current_selected,
                        current_selected_edge.as_ref(),
                        current_editing.as_ref(),
                        current_edge_creation.is_creating.then_some({
                            (
                                current_edge_creation.from_node_id.as_ref(),
                                current_edge_creation.current_x,
                                current_edge_creation.current_y,
                            )
                        }),
                        current_selection_box,
                        &image_cache_for_render,
                        &link_preview_cache_for_render,
                    );
                }
            }
        }) as Box<dyn FnMut()>);

        *render_closure_store.borrow_mut() = Some(closure);
    }

    // Subscribe to every render input; on any change, schedule at most one frame.
    Effect::new(move || {
        // Touch all render-affecting signals so this effect re-runs on any change.
        board.track();
        camera.track();
        selected_nodes.track();
        selected_edge.track();
        editing_node.track();
        edge_creation.track();
        selection_box.track();
        image_load_trigger.track(); // image loads
        link_preview_trigger.track(); // link preview loads

        if render_scheduled.replace(true) {
            // A frame is already queued; the rAF callback will pick up the latest
            // signal values, so there's nothing more to do.
            return;
        }

        if let Some(closure) = render_closure.borrow().as_ref() {
            if let Some(win) = web_sys::window() {
                if win
                    .request_animation_frame(closure.as_ref().unchecked_ref())
                    .is_err()
                {
                    // Scheduling failed — clear the flag so a later change can retry.
                    render_scheduled.set(false);
                }
            } else {
                render_scheduled.set(false);
            }
        } else {
            render_scheduled.set(false);
        }
    });

    let on_mouse_down = move |ev: web_sys::MouseEvent| {
        if editing_node.get_untracked().is_some() {
            return;
        }

        let canvas = canvas_ref.get().unwrap();
        let _ = canvas.focus();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let cam = camera.get_untracked();
        let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);

        let current_board = board.get_untracked();
        let current_selected = selected_nodes.get_untracked();
        let handle_size = RESIZE_HANDLE_SIZE / cam.zoom;

        // First check if clicking on a resize handle of any selected node
        // (handles extend outside node bounds, so check before contains_point)
        let resize_hit = current_board.nodes.iter()
            .filter(|n| current_selected.contains(&n.id))
            .find_map(|n| n.resize_handle_at(world_x, world_y, handle_size).map(|h| (n, h)));

        if let Some((node, handle)) = resize_hit {
            // History is NOT snapshotted here — it's deferred to the first actual
            // resize movement in on_mouse_move (F114), so merely clicking a handle
            // without dragging leaves no junk undo entry.
            set_resize_state.set(ResizeState {
                is_resizing: true,
                node_id: Some(node.id.clone()),
                handle: Some(handle),
                start_mouse_x: world_x,
                start_mouse_y: world_y,
                original_x: node.x,
                original_y: node.y,
                original_width: node.width,
                original_height: node.height,
                snapshotted: false,
            });
            return;
        }

        let clicked_node = current_board
            .nodes
            .iter()
            .rev()
            .find(|n| n.contains_point(world_x, world_y));

        if let Some(node) = clicked_node {
            set_selected_edge.set(None);
            if ev.shift_key() {
                set_edge_creation.set(EdgeCreationState {
                    is_creating: true,
                    from_node_id: Some(node.id.clone()),
                    current_x: canvas_x,
                    current_y: canvas_y,
                });
            } else {
                if ev.meta_key() || ev.ctrl_key() {
                    set_selected_nodes.update(|s| {
                        if !s.remove(&node.id) {
                            s.insert(node.id.clone());
                        }
                    });
                } else if !current_selected.contains(&node.id) {
                    set_selected_nodes.set([node.id.clone()].into_iter().collect());
                }

                // Copy link URL to clipboard when clicking a link node
                if node.node_type == NodeType::Link && !node.text.is_empty() {
                    let url = node.text.clone();
                    spawn_local(async move {
                        if let Some(window) = web_sys::window() {
                            let clipboard = window.navigator().clipboard();
                            let _ = wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&url)).await;
                        }
                    });
                }

                let selected = selected_nodes.get_untracked();
                let mut start_positions = HashMap::new();
                for n in &current_board.nodes {
                    if selected.contains(&n.id) {
                        start_positions.insert(n.id.clone(), (n.x, n.y));
                    }
                }
                if start_positions.is_empty() {
                    start_positions.insert(node.id.clone(), (node.x, node.y));
                    set_selected_nodes.set([node.id.clone()].into_iter().collect());
                }

                // History is NOT snapshotted here — it's deferred to the first actual
                // drag movement in on_mouse_move (F114), so a plain click (mouse down
                // + up without moving) leaves no junk undo entry.
                set_drag_state.set(DragState {
                    is_dragging: true,
                    is_box_selecting: false,
                    start_x: canvas_x,
                    start_y: canvas_y,
                    node_start_positions: start_positions,
                    snapshotted: false,
                });
            }
        } else {
            let node_map: HashMap<&str, &Node> =
                current_board.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
            let clicked_edge = current_board.edges.iter().find(|edge| {
                let from = node_map.get(edge.from_node.as_str());
                let to = node_map.get(edge.to_node.as_str());
                if let (Some(from), Some(to)) = (from, to) {
                    let from_cx = from.x + from.width / 2.0;
                    let from_cy = from.y + from.height / 2.0;
                    let to_cx = to.x + to.width / 2.0;
                    let to_cy = to.y + to.height / 2.0;
                    point_near_line(world_x, world_y, from_cx, from_cy, to_cx, to_cy, 10.0 / cam.zoom)
                } else {
                    false
                }
            });

            if let Some(edge) = clicked_edge {
                set_selected_nodes.set(HashSet::new());
                set_selected_edge.set(Some(edge.id.clone()));
            } else {
                set_selected_edge.set(None);
                if !ev.shift_key() && !ev.meta_key() && !ev.ctrl_key() {
                    set_selected_nodes.set(HashSet::new());
                }
                if ev.ctrl_key() || ev.meta_key() {
                    set_drag_state.set(DragState {
                        is_dragging: false,
                        is_box_selecting: true,
                        start_x: canvas_x,
                        start_y: canvas_y,
                        node_start_positions: HashMap::new(),
                        snapshotted: false,
                    });
                } else {
                    set_pan_state.set(PanState {
                        is_panning: true,
                        start_x: canvas_x,
                        start_y: canvas_y,
                        camera_start_x: cam.x,
                        camera_start_y: cam.y,
                    });
                }
            }
        }
    };

    let on_mouse_move = move |ev: web_sys::MouseEvent| {
        let canvas = canvas_ref.get().unwrap();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let current_drag = drag_state.get_untracked();
        let current_pan = pan_state.get_untracked();
        let edge_state = edge_creation.get_untracked();
        let current_resize = resize_state.get_untracked();

        if current_resize.is_resizing {
            let cam = camera.get_untracked();
            let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);
            let dx = world_x - current_resize.start_mouse_x;
            let dy = world_y - current_resize.start_mouse_y;

            // Deferred undo snapshot: take it once, on the first actual resize move,
            // capturing the board+selection BEFORE any geometry change (F114).
            if !current_resize.snapshotted {
                dispatch.snapshot();
                set_resize_state.update(|s| s.snapshotted = true);
            }

            set_board.update(|b| {
                if let Some(node_id) = &current_resize.node_id {
                    if let Some(node) = b.nodes.iter_mut().find(|n| &n.id == node_id) {
                        match current_resize.handle {
                            Some(ResizeHandle::TopLeft) => {
                                let new_width = (current_resize.original_width - dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height - dy).max(MIN_NODE_HEIGHT);
                                let actual_dx = current_resize.original_width - new_width;
                                let actual_dy = current_resize.original_height - new_height;
                                node.x = current_resize.original_x + actual_dx;
                                node.y = current_resize.original_y + actual_dy;
                                node.width = new_width;
                                node.height = new_height;
                            }
                            Some(ResizeHandle::TopRight) => {
                                let new_width = (current_resize.original_width + dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height - dy).max(MIN_NODE_HEIGHT);
                                let actual_dy = current_resize.original_height - new_height;
                                node.y = current_resize.original_y + actual_dy;
                                node.width = new_width;
                                node.height = new_height;
                            }
                            Some(ResizeHandle::BottomLeft) => {
                                let new_width = (current_resize.original_width - dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height + dy).max(MIN_NODE_HEIGHT);
                                let actual_dx = current_resize.original_width - new_width;
                                node.x = current_resize.original_x + actual_dx;
                                node.width = new_width;
                                node.height = new_height;
                            }
                            Some(ResizeHandle::BottomRight) => {
                                let new_width = (current_resize.original_width + dx).max(MIN_NODE_WIDTH);
                                let new_height = (current_resize.original_height + dy).max(MIN_NODE_HEIGHT);
                                node.width = new_width;
                                node.height = new_height;
                            }
                            None => {}
                        }
                    }
                }
            });
        } else if edge_state.is_creating {
            set_edge_creation.update(|s| {
                s.current_x = canvas_x;
                s.current_y = canvas_y;
            });
        } else if current_drag.is_dragging {
            let cam = camera.get_untracked();
            let dx = (canvas_x - current_drag.start_x) / cam.zoom;
            let dy = (canvas_y - current_drag.start_y) / cam.zoom;

            // Deferred undo snapshot: take it once, on the first actual drag move,
            // capturing the board+selection BEFORE any position change (F114).
            if !current_drag.snapshotted {
                dispatch.snapshot();
                set_drag_state.update(|s| s.snapshotted = true);
            }

            set_board.update(|b| {
                for (id, (start_x, start_y)) in &current_drag.node_start_positions {
                    if let Some(node) = b.nodes.iter_mut().find(|n| &n.id == id) {
                        node.x = start_x + dx;
                        node.y = start_y + dy;
                    }
                }
            });
        } else if current_drag.is_box_selecting {
            let cam = camera.get_untracked();
            let (start_wx, start_wy) = cam.screen_to_world(current_drag.start_x, current_drag.start_y);
            let (end_wx, end_wy) = cam.screen_to_world(canvas_x, canvas_y);
            set_selection_box.set(Some((
                start_wx.min(end_wx),
                start_wy.min(end_wy),
                start_wx.max(end_wx),
                start_wy.max(end_wy),
            )));
        } else if current_pan.is_panning {
            let cam = camera.get_untracked();
            let dx = (canvas_x - current_pan.start_x) / cam.zoom;
            let dy = (canvas_y - current_pan.start_y) / cam.zoom;

            set_camera.update(|c| {
                c.x = current_pan.camera_start_x - dx;
                c.y = current_pan.camera_start_y - dy;
            });
        } else {
            // Update cursor based on what we're hovering over
            let cam = camera.get_untracked();
            let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);
            let current_selected = selected_nodes.get_untracked();
            let current_board = board.get_untracked();
            let handle_size = RESIZE_HANDLE_SIZE / cam.zoom;

            // Track mouse position for paste operations
            set_last_mouse_world_pos.set((world_x, world_y));

            let mut new_cursor = "crosshair";

            // Check if over a resize handle on a selected node
            for node in current_board.nodes.iter().rev() {
                if current_selected.contains(&node.id) {
                    if let Some(handle) = node.resize_handle_at(world_x, world_y, handle_size) {
                        new_cursor = match handle {
                            ResizeHandle::TopLeft | ResizeHandle::BottomRight => "nwse-resize",
                            ResizeHandle::TopRight | ResizeHandle::BottomLeft => "nesw-resize",
                        };
                        break;
                    }
                }
                if node.contains_point(world_x, world_y) {
                    new_cursor = "move";
                    break;
                }
            }

            set_cursor_style.set(new_cursor.to_string());
        }
    };

    let on_mouse_up = move |ev: web_sys::MouseEvent| {
        let was_panning = pan_state.get_untracked().is_panning;
        let was_dragging = drag_state.get_untracked().is_dragging;
        let was_resizing = resize_state.get_untracked().is_resizing;
        let resize_snapshotted = resize_state.get_untracked().snapshotted;
        let drag_snapshotted = drag_state.get_untracked().snapshotted;
        let current_drag = drag_state.get_untracked();
        let edge_state = edge_creation.get_untracked();

        if was_resizing {
            set_resize_state.set(ResizeState::default());

            // Only persist if a snapshot was taken, i.e. the resize actually moved
            // the node — a bare handle click without dragging changes nothing.
            if resize_snapshotted {
                request_save.call();
            }
            return;
        }

        if edge_state.is_creating {
            if let Some(from_id) = &edge_state.from_node_id {
                let canvas = canvas_ref.get().unwrap();
                let rect = canvas.get_bounding_client_rect();
                let canvas_x = ev.client_x() as f64 - rect.left();
                let canvas_y = ev.client_y() as f64 - rect.top();
                let cam = camera.get_untracked();
                let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);

                let current_board = board.get_untracked();
                if let Some(target) = current_board.nodes.iter().rev().find(|n| n.contains_point(world_x, world_y)) {
                    if &target.id != from_id {
                        dispatch.apply(
                            BoardAction::CreateEdge {
                                id: uuid::Uuid::new_v4().to_string(),
                                from_node: from_id.clone(),
                                to_node: target.id.clone(),
                            },
                            None,
                        );
                    }
                }
            }
            set_edge_creation.set(EdgeCreationState::default());
            return;
        }

        if current_drag.is_box_selecting {
            if let Some((min_x, min_y, max_x, max_y)) = selection_box.get_untracked() {
                let current_board = board.get_untracked();
                let nodes_in_box: HashSet<String> = current_board
                    .nodes
                    .iter()
                    .filter(|n| intersects_box(n, min_x, min_y, max_x, max_y))
                    .map(|n| n.id.clone())
                    .collect();

                if ev.shift_key() {
                    set_selected_nodes.update(|s| s.extend(nodes_in_box));
                } else {
                    set_selected_nodes.set(nodes_in_box);
                }
            }
            set_selection_box.set(None);
        }

        set_drag_state.set(DragState::default());
        set_pan_state.set(PanState::default());

        // Only persist if the drag actually moved nodes (a snapshot was taken).
        // A plain click (mouse down + up without moving) changes nothing (F114).
        if was_dragging && drag_snapshotted {
            request_save.call();
        }

        // Pan-end: persist the new viewport (F105).
        if was_panning {
            persist_camera_now();
        }
    };

    // True while any pointer gesture is in flight. Used to drive document-level
    // mousemove/mouseup continuation so a drag that leaves the canvas keeps
    // tracking and finalizes exactly once on release off-canvas (F20).
    let gesture_active = move || {
        drag_state.get_untracked().is_dragging
            || drag_state.get_untracked().is_box_selecting
            || pan_state.get_untracked().is_panning
            || resize_state.get_untracked().is_resizing
            || edge_creation.get_untracked().is_creating
    };

    // mouseleave gets its OWN handler: it must NOT finalize edge-create/box-select
    // or trigger a save (that's what made dragging to the window edge drop the
    // gesture, F20). It only resets the transient hover cursor; the gesture itself
    // continues via the document-level listeners registered below.
    let on_mouse_leave = move |_ev: web_sys::MouseEvent| {
        if !gesture_active() {
            set_cursor_style.set("crosshair".to_string());
        }
    };

    // Document-level continuation (F20). While a gesture is active, mouse events
    // that land outside the canvas (off the element, including past the window
    // edge) still reach `document`. We forward those to the same move/up handlers
    // so the drag keeps tracking and releases finalize once. On-canvas events are
    // already handled by the canvas listeners, so we skip them here to avoid
    // double-processing.
    {
        let on_mouse_move_doc = on_mouse_move;
        let on_mouse_up_doc = on_mouse_up;
        Effect::new(move |prev: Option<()>| {
            // Register exactly once.
            if prev.is_some() {
                return;
            }
            let Some(window) = web_sys::window() else { return };
            let Some(document) = window.document() else { return };

            let is_outside_canvas = move |ev: &web_sys::MouseEvent| {
                match canvas_ref.get_untracked() {
                    Some(canvas) => {
                        let canvas_el: &web_sys::Element = canvas.as_ref();
                        ev.target()
                            .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                            .map(|node| !canvas_el.contains(Some(&node)))
                            .unwrap_or(true)
                    }
                    None => true,
                }
            };

            let move_cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(
                move |ev: web_sys::MouseEvent| {
                    if gesture_active() && is_outside_canvas(&ev) {
                        on_mouse_move_doc(ev);
                    }
                },
            );
            let up_cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(
                move |ev: web_sys::MouseEvent| {
                    if gesture_active() && is_outside_canvas(&ev) {
                        on_mouse_up_doc(ev);
                    }
                },
            );

            let _ = document.add_event_listener_with_callback(
                "mousemove",
                move_cb.as_ref().unchecked_ref(),
            );
            let _ = document.add_event_listener_with_callback(
                "mouseup",
                up_cb.as_ref().unchecked_ref(),
            );
            move_cb.forget();
            up_cb.forget();
        });
    }

    // Document-level Escape handler (F58/F107/F113): closes the active modal even
    // when canvas focus has been lost (e.g. after clicking inside the modal). The
    // canvas keydown only fires while the canvas is focused, so modals need their
    // own listener to stay closeable.
    {
        Effect::new(move |prev: Option<()>| {
            if prev.is_some() {
                return;
            }
            let Some(window) = web_sys::window() else { return };
            let Some(document) = window.document() else { return };

            let esc_cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
                move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Escape"
                        && (modal_image.get_untracked().is_some()
                            || modal_md.get_untracked().is_some())
                    {
                        set_modal_image.set(None);
                        set_modal_md.set(None);
                    }
                },
            );

            let _ = document.add_event_listener_with_callback(
                "keydown",
                esc_cb.as_ref().unchecked_ref(),
            );
            esc_cb.forget();
        });
    }

    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();

        let canvas = canvas_ref.get().unwrap();
        let rect = canvas.get_bounding_client_rect();
        let canvas_x = ev.client_x() as f64 - rect.left();
        let canvas_y = ev.client_y() as f64 - rect.top();

        let zoom_factor = if ev.delta_y() < 0.0 { 1.1 } else { 0.9 };

        set_camera.update(|c| {
            let (world_x, world_y) = c.screen_to_world(canvas_x, canvas_y);

            c.zoom = (c.zoom * zoom_factor).clamp(0.1, 5.0);

            c.x = world_x - canvas_x / c.zoom;
            c.y = world_y - canvas_y / c.zoom;
        });

        // Zoom-end: debounced so a scroll burst writes once (F105).
        persist_camera_now();
    };

    let on_double_click = {
        let image_cache_for_modal = image_cache_for_modal.clone();
        move |ev: web_sys::MouseEvent| {
            let canvas = canvas_ref.get().unwrap();
            let rect = canvas.get_bounding_client_rect();
            let canvas_x = ev.client_x() as f64 - rect.left();
            let canvas_y = ev.client_y() as f64 - rect.top();

            let cam = camera.get_untracked();
            let (world_x, world_y) = cam.screen_to_world(canvas_x, canvas_y);

            let current_board = board.get_untracked();
            let clicked_node = current_board
                .nodes
                .iter()
                .rev()
                .find(|n| n.contains_point(world_x, world_y));

            if let Some(node) = clicked_node {
                if node.node_type == NodeType::Image {
                    // Open image in modal - get src from cached HtmlImageElement
                    let cache = image_cache_for_modal.borrow();
                    if let Some(Some(img)) = cache.get(&node.text) {
                        set_modal_image.set(Some(img.src()));
                    }
                } else if node.node_type == NodeType::Md {
                    // Open MD in modal (view mode)
                    set_modal_md.set(Some((node.id.clone(), false)));
                } else if node.node_type == NodeType::Link && is_local_md_file(&node.text) {
                    // Open local .md file in modal (view mode)
                    set_modal_md.set(Some((node.id.clone(), false)));
                } else if node.node_type == NodeType::Link {
                    // Open regular link in browser
                    if let Some(window) = web_sys::window() {
                        let _ = window.open_with_url_and_target(&node.text, "_blank");
                    }
                } else {
                    // Edit mode for text, idea, note nodes
                    set_editing_node.set(Some(node.id.clone()));
                }
            } else {
                let new_node = Node::new(
                    uuid::Uuid::new_v4().to_string(),
                    world_x - 100.0,
                    world_y - 50.0,
                    "New Node".to_string(),
                );
                let new_id = new_node.id.clone();

                dispatch.apply(
                    BoardAction::CreateNode(new_node),
                    Some([new_id.clone()].into_iter().collect()),
                );
                set_editing_node.set(Some(new_id));
            }
        }
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if editing_node.get_untracked().is_some() {
            return;
        }
        // While a modal is open, swallow canvas shortcuts (F113). The document-level
        // Escape listener handles closing the modal; everything else (delete, copy,
        // type-cycle, fit, etc.) must not fire and mutate the board behind the modal.
        if modal_md.get_untracked().is_some() || modal_image.get_untracked().is_some() {
            return;
        }

        let key = ev.key();
        let selected = selected_nodes.get_untracked();
        let edge_sel = selected_edge.get_untracked();

        match key.as_str() {
            "z" if ev.meta_key() || ev.ctrl_key() => {
                ev.prevent_default();
                if ev.shift_key() {
                    // Redo: Ctrl+Shift+Z / Cmd+Shift+Z
                    dispatch.redo();
                } else {
                    // Undo: Ctrl+Z / Cmd+Z
                    dispatch.undo();
                }
            }
            "Backspace" | "Delete" => {
                if let Some(edge_id) = edge_sel {
                    dispatch.apply(
                        BoardAction::DeleteSelected {
                            node_ids: vec![],
                            edge_id: Some(edge_id),
                        },
                        None,
                    );
                    set_selected_edge.set(None);
                } else if !selected.is_empty() {
                    // Asset cleanup is modeled as a SideEffect by the reducer.
                    dispatch.apply(
                        BoardAction::DeleteSelected {
                            node_ids: selected.into_iter().collect(),
                            edge_id: None,
                        },
                        Some(HashSet::new()),
                    );
                }
            }
            "c" if ev.meta_key() || ev.ctrl_key() => {
                if !selected.is_empty() {
                    let current_board = board.get_untracked();
                    let copied_nodes: Vec<Node> = current_board.nodes.iter()
                        .filter(|n| selected.contains(&n.id))
                        .cloned()
                        .collect();
                    let copied_edges: Vec<Edge> = current_board.edges.iter()
                        .filter(|e| selected.contains(&e.from_node) && selected.contains(&e.to_node))
                        .cloned()
                        .collect();
                    set_node_clipboard.set(Some((copied_nodes, copied_edges)));
                }
            }
            "v" if ev.meta_key() || ev.ctrl_key() => {
                if let Some((ref nodes, ref edges)) = node_clipboard.get_untracked() {
                    if !nodes.is_empty() {
                        ev.prevent_default();

                        // Calculate center of copied nodes
                        let cx = nodes.iter().map(|n| n.x + n.width / 2.0).sum::<f64>() / nodes.len() as f64;
                        let cy = nodes.iter().map(|n| n.y + n.height / 2.0).sum::<f64>() / nodes.len() as f64;
                        let (mouse_x, mouse_y) = last_mouse_world_pos.get_untracked();

                        // Build old_id -> new_id mapping
                        let id_map: HashMap<String, String> = nodes.iter()
                            .map(|n| (n.id.clone(), uuid::Uuid::new_v4().to_string()))
                            .collect();

                        let new_nodes: Vec<Node> = nodes.iter().map(|n| {
                            Node {
                                id: id_map[&n.id].clone(),
                                x: n.x - cx + mouse_x,
                                y: n.y - cy + mouse_y,
                                ..n.clone()
                            }
                        }).collect();

                        let new_edges: Vec<Edge> = edges.iter().map(|e| {
                            Edge {
                                id: uuid::Uuid::new_v4().to_string(),
                                from_node: id_map[&e.from_node].clone(),
                                to_node: id_map[&e.to_node].clone(),
                                label: e.label.clone(),
                            }
                        }).collect();

                        let new_ids: HashSet<String> = new_nodes.iter().map(|n| n.id.clone()).collect();

                        dispatch.apply(
                            BoardAction::PasteNodes {
                                nodes: new_nodes,
                                edges: new_edges,
                            },
                            Some(new_ids),
                        );
                    }
                }
                // If no internal clipboard, let ClipboardEvent fire for image paste
            }
            "t" | "T" => {
                if !selected.is_empty() {
                    dispatch.apply(
                        BoardAction::CycleType(selected.into_iter().collect()),
                        None,
                    );
                }
            }
            "a" | "A" if ev.meta_key() || ev.ctrl_key() => {
                // Select all nodes (F103). Edge selection is mutually exclusive
                // with a node multi-selection, so clear it.
                ev.prevent_default();
                let all_ids: HashSet<String> = board
                    .get_untracked()
                    .nodes
                    .iter()
                    .map(|n| n.id.clone())
                    .collect();
                set_selected_nodes.set(all_ids);
                set_selected_edge.set(None);
            }
            "f" | "F" if ev.meta_key() || ev.ctrl_key() => {
                // Open the search overlay (F99). Seed with an empty query; the
                // overlay input autofocuses.
                ev.prevent_default();
                set_search_query.set(Some(String::new()));
            }
            "f" | "F" => {
                // Fit all nodes into view (F102). No-op on an empty board.
                if let Some(bbox) = nodes_bounding_box(&board.get_untracked().nodes) {
                    if let Some(canvas) = canvas_ref.get_untracked() {
                        let rect = canvas.get_bounding_client_rect();
                        let cam = fit_camera(bbox, rect.width(), rect.height(), 0.1);
                        set_camera.set(cam);
                        persist_camera_now();
                    }
                }
            }
            "0" if ev.meta_key() || ev.ctrl_key() => {
                // Reset zoom to 1.0, keeping the viewport center fixed (F102).
                ev.prevent_default();
                if let Some(canvas) = canvas_ref.get_untracked() {
                    let rect = canvas.get_bounding_client_rect();
                    let (cw, ch) = (rect.width(), rect.height());
                    set_camera.update(|c| {
                        let (center_wx, center_wy) = c.screen_to_world(cw / 2.0, ch / 2.0);
                        c.zoom = 1.0;
                        c.x = center_wx - cw / 2.0;
                        c.y = center_wy - ch / 2.0;
                    });
                    persist_camera_now();
                }
            }
            "Escape" => {
                set_selected_nodes.set(HashSet::new());
                set_selected_edge.set(None);
                set_editing_node.set(None);
                set_edge_creation.set(EdgeCreationState::default());
                set_selection_box.set(None);
                set_modal_image.set(None);
                set_modal_md.set(None);
            }
            _ => {}
        }
    };

    let on_paste = move |ev: web_sys::ClipboardEvent| {
        // If internal node clipboard was used, keydown already handled it
        if node_clipboard.get_untracked().as_ref().is_some_and(|(n, _)| !n.is_empty()) {
            return;
        }

        ev.prevent_default();

        if !is_tauri() {
            return; // Image paste only works in Tauri mode
        }

        let (world_x, world_y) = last_mouse_world_pos.get_untracked();

        spawn_local(async move {
            let result = invoke("paste_image", JsValue::NULL).await;

            // Debug: log the raw result
            web_sys::console::log_2(&"paste_image result:".into(), &result);

            match serde_wasm_bindgen::from_value::<PasteImageResult>(result.clone()) {
                Ok(paste_result) => {
                    web_sys::console::log_1(&format!("Paste success: path={}, {}x{}", paste_result.path, paste_result.width, paste_result.height).into());

                    let node_width = (paste_result.width as f64).min(400.0).max(100.0);
                    let node_height = (paste_result.height as f64).min(400.0).max(100.0);

                    let new_node = Node {
                        id: uuid::Uuid::new_v4().to_string(),
                        x: world_x - node_width / 2.0,
                        y: world_y - node_height / 2.0,
                        width: node_width,
                        height: node_height,
                        text: paste_result.path,
                        node_type: NodeType::Image,
                        color: None,
                        tags: Vec::new(),
                        status: None,
                        group: None,
                        priority: None,
                    };
                    let new_id = new_node.id.clone();

                    dispatch.apply(
                        BoardAction::CreateNode(new_node),
                        Some([new_id].into_iter().collect()),
                    );
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Paste failed: {:?}", e).into());
                }
            }
        });
    };

    let on_upload = move |_ev: web_sys::MouseEvent| {
        if let Some(input) = file_input_ref.get() {
            let el: &web_sys::HtmlElement = &input;
            el.click();
        }
    };

    let on_file_selected = move |_ev: web_sys::Event| {
        let input = file_input_ref.get().unwrap();
        let input_el: &web_sys::HtmlInputElement = (*input).unchecked_ref();
        let files = input_el.files().unwrap();
        if files.length() == 0 {
            return;
        }
        let file = files.get(0).unwrap();
        let reader = web_sys::FileReader::new().unwrap();
        let reader_clone = reader.clone();

        let onload = Closure::wrap(Box::new(move || {
            if let Ok(result) = reader_clone.result() {
                if let Some(text) = result.as_string() {
                    if let Ok(parsed) = serde_json::from_str::<Board>(&text) {
                        set_board.set(parsed);
                        request_save.call();
                    }
                }
            }
        }) as Box<dyn Fn()>);

        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
        onload.forget();
        let _ = reader.read_as_text(&file);

        // Reset input so re-uploading same file triggers change
        input_el.set_value("");
    };

    let on_download = move |_ev: web_sys::MouseEvent| {
        let current_board = board.get_untracked();
        let json = serde_json::to_string_pretty(&current_board).unwrap_or_default();

        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();

        let array = js_sys::Array::new();
        array.push(&JsValue::from_str(&json));
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type("application/json");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&array, &opts).unwrap();

        let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
        let a: web_sys::HtmlAnchorElement = document
            .create_element("a")
            .unwrap()
            .unchecked_into();
        a.set_href(&url);
        a.set_download("board.json");
        a.click();
        let _ = web_sys::Url::revoke_object_url(&url);
    };

    let button_style = "background: #0a0a0a; color: #66cc88; border: 1px solid #2a4a3a; \
        padding: 6px 14px; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; \
        font-size: 12px; cursor: pointer; border-radius: 4px;";

    view! {
        <div style="width: 100vw; height: 100vh; overflow: hidden; background: #020202; position: relative;">
            <canvas
                node_ref=canvas_ref
                tabindex="0"
                style=move || format!("width: 100%; height: 100%; display: block; cursor: {}; outline: none;", cursor_style.get())
                on:mousedown=on_mouse_down
                on:mousemove=on_mouse_move
                on:mouseup=on_mouse_up
                on:mouseleave=on_mouse_leave
                on:wheel=on_wheel
                on:dblclick=on_double_click
                on:keydown=on_keydown
                on:paste=on_paste
            />
            <NodeEditor/>
            <MarkdownOverlays/>
            <ImageModal/>
            <MarkdownModal/>
            <ErrorBanner/>
            <SearchOverlay/>
            <Show when=move || !is_tauri()>
                <div style="position: fixed; top: 12px; right: 12px; display: flex; gap: 8px; z-index: 100;">
                    <button style=button_style on:click=on_upload>"Upload board.json"</button>
                    <button style=button_style on:click=on_download>"Download board.json"</button>
                </div>
                <input type="file" accept=".json" node_ref=file_input_ref style="display:none"
                       on:change=on_file_selected />
            </Show>
            <div style="position: fixed; bottom: 12px; left: 12px; color: #66cc88; font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace; font-size: 11px; letter-spacing: 0.5px;">
                "[DBLCLK] add/edit  [DRAG corner] resize  [SHIFT+DRAG] connect  [CMD+DRAG] box  [CMD+C] copy  [CMD+V] paste  [T] type  [DEL] delete  [CMD+Z] undo  [CMD+SHIFT+Z] redo  [CMD+F] search  [F] fit  [CMD+0] reset zoom  [CMD+A] select all"
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod load_outcome_tests {
        use super::*;

        #[test]
        fn valid_json_yields_loaded() {
            let json = r#"{"nodes":[{"id":"n1","x":0.0,"y":0.0,"width":200.0,"height":100.0,"text":"hi","node_type":"text"}],"edges":[]}"#;
            match parse_localstorage_board(json) {
                LoadOutcome::Loaded(board) => {
                    assert_eq!(board.nodes.len(), 1);
                    assert_eq!(board.nodes[0].id, "n1");
                }
                other => panic!("expected Loaded, got {:?}", other),
            }
        }

        #[test]
        fn empty_string_yields_absent() {
            assert!(matches!(parse_localstorage_board(""), LoadOutcome::Absent));
            assert!(matches!(parse_localstorage_board("   \n\t "), LoadOutcome::Absent));
        }

        #[test]
        fn malformed_json_yields_parse_error_not_empty_board() {
            // Truncated / invalid JSON — the exact failure mode that previously
            // collapsed into Board::default() and let the next save destroy data.
            let malformed = r#"{"nodes": [{"id": "n1", "x": 0, "#;
            match parse_localstorage_board(malformed) {
                LoadOutcome::ParseError(msg) => {
                    assert!(!msg.is_empty(), "parse error should carry a message");
                }
                LoadOutcome::Loaded(board) => {
                    panic!("malformed input must not parse into a board ({} nodes)", board.nodes.len());
                }
                LoadOutcome::Absent => panic!("malformed (non-empty) input must not be Absent"),
            }
        }

        #[test]
        fn wrong_shape_json_yields_parse_error() {
            // Valid JSON, but not a Board shape.
            let wrong = r#"{"totally": "different", "schema": 42}"#;
            assert!(matches!(parse_localstorage_board(wrong), LoadOutcome::ParseError(_)));
        }

        #[test]
        fn parse_error_does_not_replace_non_empty_board() {
            // Simulate the load path's contract: a non-empty board must survive a
            // ParseError. We only call set_board on Loaded/Absent, never ParseError.
            let existing = Board {
                nodes: vec![Node::new("text".into(), 0.0, 0.0, "keep me".into())],
                edges: vec![],
            };
            let outcome = parse_localstorage_board("{ broken");
            let mut current = existing.clone();
            match outcome {
                LoadOutcome::Loaded(b) => current = b,
                LoadOutcome::Absent => current = Board::default(),
                LoadOutcome::ParseError(_) => { /* keep current untouched */ }
            }
            assert_eq!(current.nodes.len(), 1, "ParseError must not blank the board");
            assert_eq!(current.nodes[0].text, "keep me");
        }
    }

    mod is_local_md_file_tests {
        use super::*;

        #[test]
        fn absolute_path() {
            assert!(is_local_md_file("/Users/me/vault/note.md"));
            assert!(is_local_md_file("/path/to/file.md"));
        }

        #[test]
        fn file_url() {
            assert!(is_local_md_file("file:///Users/me/vault/note.md"));
            assert!(is_local_md_file("file:///path/to/file.md"));
        }

        #[test]
        fn file_url_with_encoded_spaces() {
            assert!(is_local_md_file("file:///Users/me/Obsidian%20Vault/note.md"));
        }

        #[test]
        fn home_relative_path() {
            assert!(is_local_md_file("~/Documents/note.md"));
            assert!(is_local_md_file("~/vault/subfolder/note.md"));
        }

        #[test]
        fn case_insensitive_extension() {
            assert!(is_local_md_file("/path/to/file.MD"));
            assert!(is_local_md_file("/path/to/file.Md"));
            assert!(is_local_md_file("~/note.MD"));
        }

        #[test]
        fn rejects_http_urls() {
            assert!(!is_local_md_file("http://example.com/file.md"));
            assert!(!is_local_md_file("https://example.com/file.md"));
        }

        #[test]
        fn rejects_non_md_files() {
            assert!(!is_local_md_file("/path/to/file.txt"));
            assert!(!is_local_md_file("/path/to/file.pdf"));
            assert!(!is_local_md_file("~/document.docx"));
            assert!(!is_local_md_file("file:///path/to/image.png"));
        }

        #[test]
        fn rejects_relative_paths() {
            assert!(!is_local_md_file("./note.md"));
            assert!(!is_local_md_file("../note.md"));
            assert!(!is_local_md_file("note.md"));
        }

        #[test]
        fn rejects_empty_string() {
            assert!(!is_local_md_file(""));
        }

        #[test]
        fn handles_md_in_path_but_wrong_extension() {
            assert!(!is_local_md_file("/path/to/markdown/file.txt"));
            assert!(!is_local_md_file("~/Documents/md-files/note.pdf"));
        }
    }

    mod public_http_host_tests {
        use super::*;

        #[test]
        fn allows_public_domains() {
            assert!(is_public_http_host("https://example.com"));
            assert!(is_public_http_host("http://github.com/anthropics/claude-code"));
            assert!(is_public_http_host("https://sub.domain.example.org/path?q=1#frag"));
            assert!(is_public_http_host("https://example.com:8443/x"));
            assert!(is_public_http_host("https://user:pass@example.com/x"));
        }

        #[test]
        fn rejects_non_http_schemes() {
            assert!(!is_public_http_host("file:///etc/passwd"));
            assert!(!is_public_http_host("ftp://example.com"));
            assert!(!is_public_http_host("/local/path.md"));
            assert!(!is_public_http_host(""));
        }

        #[test]
        fn rejects_localhost_and_internal_tlds() {
            assert!(!is_public_http_host("http://localhost"));
            assert!(!is_public_http_host("http://localhost:3000/admin"));
            assert!(!is_public_http_host("http://printer.local"));
            assert!(!is_public_http_host("http://db.internal/health"));
            assert!(!is_public_http_host("http://server.lan"));
            assert!(!is_public_http_host("http://nas.home"));
            assert!(!is_public_http_host("http://wiki.corp"));
            assert!(!is_public_http_host("http://x.intranet"));
        }

        #[test]
        fn rejects_ipv4_literals() {
            assert!(!is_public_http_host("http://169.254.169.254/latest/meta-data/"));
            assert!(!is_public_http_host("http://127.0.0.1:8080"));
            assert!(!is_public_http_host("http://10.0.0.5"));
            assert!(!is_public_http_host("http://192.168.1.1/admin"));
            // Even a public IP literal is skipped for auto-fetch (backend guards).
            assert!(!is_public_http_host("http://8.8.8.8"));
        }

        #[test]
        fn rejects_decimal_and_ipv6_literals() {
            // Decimal-encoded 127.0.0.1 — single all-numeric label.
            assert!(!is_public_http_host("http://2130706433/"));
            // IPv6 literals are never auto-fetched.
            assert!(!is_public_http_host("http://[::1]/"));
            assert!(!is_public_http_host("http://[::ffff:127.0.0.1]/"));
        }

        #[test]
        fn rejects_single_label_hosts() {
            assert!(!is_public_http_host("http://intranet-box/dashboard"));
            assert!(!is_public_http_host("http://router/"));
        }
    }

    mod cycle_node_type_tests {
        // `cycle_node_type` moved to the reducer module (interaction.rs) as part of
        // the P1.3 reducer extraction; this asserts the app's view of that behavior.
        use crate::interaction::cycle_node_type;

        #[test]
        fn cycles_through_all_types() {
            assert_eq!(cycle_node_type("text"), "idea");
            assert_eq!(cycle_node_type("idea"), "note");
            assert_eq!(cycle_node_type("note"), "image");
            assert_eq!(cycle_node_type("image"), "md");
            assert_eq!(cycle_node_type("md"), "link");
            assert_eq!(cycle_node_type("link"), "text");
        }

        #[test]
        fn unknown_type_wraps_to_text() {
            assert_eq!(cycle_node_type("unknown"), "text");
            assert_eq!(cycle_node_type(""), "text");
        }
    }

    mod intersects_box_tests {
        use super::*;
        use crate::state::Node;

        fn node_at(x: f64, y: f64, w: f64, h: f64) -> Node {
            Node { x, y, width: w, height: h, ..Node::new("t".into(), x, y, String::new()) }
        }

        #[test]
        fn fully_inside() {
            assert!(intersects_box(&node_at(10.0, 10.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_right() {
            assert!(!intersects_box(&node_at(200.0, 10.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_left() {
            assert!(!intersects_box(&node_at(-50.0, 10.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_above() {
            assert!(!intersects_box(&node_at(10.0, -50.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn fully_outside_below() {
            assert!(!intersects_box(&node_at(10.0, 200.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn partially_overlapping() {
            assert!(intersects_box(&node_at(90.0, 90.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }

        #[test]
        fn touching_edge() {
            assert!(intersects_box(&node_at(100.0, 0.0, 20.0, 20.0), 0.0, 0.0, 100.0, 100.0));
        }
    }

    mod point_near_line_tests {
        use super::*;

        #[test]
        fn point_on_line() {
            assert!(point_near_line(5.0, 5.0, 0.0, 0.0, 10.0, 10.0, 1.0));
        }

        #[test]
        fn point_far_from_line() {
            assert!(!point_near_line(50.0, 50.0, 0.0, 0.0, 10.0, 0.0, 5.0));
        }

        #[test]
        fn point_near_midpoint() {
            assert!(point_near_line(5.0, 1.0, 0.0, 0.0, 10.0, 0.0, 2.0));
        }

        #[test]
        fn point_near_endpoint() {
            assert!(point_near_line(0.5, 0.0, 0.0, 0.0, 10.0, 0.0, 1.0));
        }

        #[test]
        fn point_beyond_segment_end() {
            assert!(!point_near_line(15.0, 0.0, 0.0, 0.0, 10.0, 0.0, 1.0));
        }

        #[test]
        fn degenerate_zero_length_line() {
            assert!(point_near_line(0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0));
            assert!(!point_near_line(5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0));
        }
    }

    mod parse_markdown_tests {
        use super::*;

        #[test]
        fn renders_heading() {
            let html = parse_markdown("# Hello");
            assert!(html.contains("<h1>Hello</h1>"));
        }

        #[test]
        fn renders_bold() {
            let html = parse_markdown("**bold**");
            assert!(html.contains("<strong>bold</strong>"));
        }

        #[test]
        fn renders_list() {
            let html = parse_markdown("- item 1\n- item 2");
            assert!(html.contains("<li>item 1</li>"));
            assert!(html.contains("<li>item 2</li>"));
        }

        #[test]
        fn empty_input() {
            assert_eq!(parse_markdown(""), "");
        }

        #[test]
        fn strips_raw_html_xss() {
            // Stored-XSS payload: the raw <img onerror=...> must not reach the
            // inner_html sink as an active element. It is escaped to literal text,
            // so the angle brackets render and no element/handler executes.
            let html = parse_markdown("<img src=x onerror=alert(1)>");
            // No active <img> element: the opening angle bracket is escaped, so the
            // browser parses the payload as inert text, not a tag with a handler.
            assert!(
                !html.contains("<img"),
                "active <img> element leaked: {html}"
            );
            // The whole payload is escaped — the literal angle brackets survive.
            assert!(html.contains("&lt;img"), "expected escaped markup: {html}");
            assert!(html.contains("&gt;"), "expected escaped closing bracket: {html}");
        }

        #[test]
        fn strips_inline_html_script() {
            let html = parse_markdown("hello <script>alert(1)</script> world");
            assert!(!html.contains("<script>"), "raw <script> leaked: {html}");
            assert!(html.contains("&lt;script&gt;"), "expected escaped script: {html}");
        }
    }

    mod node_matches_query_tests {
        use super::*;

        fn node(text: &str) -> Node {
            Node::new("n".to_string(), 0.0, 0.0, text.to_string())
        }

        #[test]
        fn matches_text_case_insensitive() {
            let n = node("Pricing Strategy");
            assert!(node_matches_query(&n, "pricing"));
            assert!(node_matches_query(&n, "STRATEGY"));
            assert!(!node_matches_query(&n, "roadmap"));
        }

        #[test]
        fn matches_tags() {
            let mut n = node("body");
            n.tags = vec!["urgent".to_string(), "v2".to_string()];
            assert!(node_matches_query(&n, "urgent"));
            assert!(node_matches_query(&n, "V2"));
            assert!(!node_matches_query(&n, "v3"));
        }

        #[test]
        fn matches_status() {
            let mut n = node("body");
            n.status = Some("in-progress".to_string());
            assert!(node_matches_query(&n, "progress"));
            assert!(!node_matches_query(&n, "done"));
        }

        #[test]
        fn empty_query_matches_nothing() {
            let n = node("anything");
            assert!(!node_matches_query(&n, ""));
            assert!(!node_matches_query(&n, "   \t"));
        }
    }

    mod bounding_box_tests {
        use super::*;

        #[test]
        fn empty_slice_is_none() {
            assert!(nodes_bounding_box(&[]).is_none());
        }

        #[test]
        fn single_node_box_is_its_rect() {
            let n = Node::new("n".to_string(), 10.0, 20.0, "".to_string());
            // Default 200x100.
            let bbox = nodes_bounding_box(std::slice::from_ref(&n)).unwrap();
            assert_eq!(bbox, (10.0, 20.0, 210.0, 120.0));
        }

        #[test]
        fn spans_all_nodes_including_far_outlier() {
            let near = Node::new("a".to_string(), 0.0, 0.0, "".to_string());
            let far = Node::new("b".to_string(), 50000.0, 50000.0, "".to_string());
            let bbox = nodes_bounding_box(&[near, far]).unwrap();
            assert_eq!(bbox.0, 0.0);
            assert_eq!(bbox.1, 0.0);
            assert_eq!(bbox.2, 50200.0);
            assert_eq!(bbox.3, 50100.0);
        }
    }

    mod fit_camera_tests {
        use super::*;

        #[test]
        fn distant_node_lands_inside_viewport() {
            // The verify gate's scenario: a node placed at (50000, 50000). After
            // fit-to-view it must be visible — its center maps to a screen point
            // inside the canvas.
            let node = Node::new("n".to_string(), 50000.0, 50000.0, "".to_string());
            let bbox = nodes_bounding_box(std::slice::from_ref(&node)).unwrap();
            let (cw, ch) = (800.0, 600.0);
            let cam = fit_camera(bbox, cw, ch, 0.1);

            let (center_wx, center_wy) = node.center();
            let (sx, sy) = cam.world_to_screen(center_wx, center_wy);
            assert!(sx >= 0.0 && sx <= cw, "x off-screen: {sx}");
            assert!(sy >= 0.0 && sy <= ch, "y off-screen: {sy}");
        }

        #[test]
        fn zoom_is_clamped_to_range() {
            // A tiny box would otherwise demand a huge zoom; clamp caps at 5.0.
            let tiny = (0.0, 0.0, 1.0, 1.0);
            let cam = fit_camera(tiny, 800.0, 600.0, 0.1);
            assert!(cam.zoom <= 5.0 && cam.zoom >= 0.1);
        }

        #[test]
        fn huge_box_clamps_to_min_zoom() {
            let huge = (0.0, 0.0, 1_000_000.0, 1_000_000.0);
            let cam = fit_camera(huge, 800.0, 600.0, 0.1);
            assert_eq!(cam.zoom, 0.1);
        }

        #[test]
        fn degenerate_viewport_does_not_panic() {
            let bbox = (0.0, 0.0, 100.0, 100.0);
            let cam = fit_camera(bbox, 0.0, 0.0, 0.1);
            assert!(cam.zoom.is_finite() && cam.x.is_finite() && cam.y.is_finite());
        }

        #[test]
        fn multi_node_box_is_centered() {
            let a = Node::new("a".to_string(), 0.0, 0.0, "".to_string());
            let b = Node::new("b".to_string(), 1000.0, 1000.0, "".to_string());
            let bbox = nodes_bounding_box(&[a, b]).unwrap();
            let (cw, ch) = (800.0, 600.0);
            let cam = fit_camera(bbox, cw, ch, 0.1);
            // Viewport center should map to the box center.
            let box_cx = (bbox.0 + bbox.2) / 2.0;
            let box_cy = (bbox.1 + bbox.3) / 2.0;
            let (vx, vy) = cam.screen_to_world(cw / 2.0, ch / 2.0);
            assert!((vx - box_cx).abs() < 1e-6, "x center off: {vx} vs {box_cx}");
            assert!((vy - box_cy).abs() < 1e-6, "y center off: {vy} vs {box_cy}");
        }
    }

    mod camera_persist_tests {
        use super::*;

        #[test]
        fn round_trips_a_camera() {
            let cam = Camera { x: 123.0, y: -456.0, zoom: 1.75 };
            let restored = CameraPersist::from_camera(&cam).to_camera();
            assert_eq!(restored.x, 123.0);
            assert_eq!(restored.y, -456.0);
            assert_eq!(restored.zoom, 1.75);
        }

        #[test]
        fn json_round_trip() {
            let p = CameraPersist { x: 1.0, y: 2.0, zoom: 3.0 };
            let json = serde_json::to_string(&p).unwrap();
            let back: CameraPersist = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }

        #[test]
        fn sanitizes_out_of_range_zoom() {
            for bad in [0.0, -1.0, 99.0, f64::NAN, f64::INFINITY] {
                let cam = CameraPersist { x: 5.0, y: 6.0, zoom: bad }.to_camera();
                assert_eq!(cam.zoom, 1.0, "zoom {bad} not sanitized");
                assert_eq!(cam.x, 5.0);
                assert_eq!(cam.y, 6.0);
            }
        }

        #[test]
        fn sanitizes_non_finite_position() {
            let cam = CameraPersist { x: f64::NAN, y: f64::INFINITY, zoom: 2.0 }.to_camera();
            assert_eq!(cam.x, 0.0);
            assert_eq!(cam.y, 0.0);
            assert_eq!(cam.zoom, 2.0);
        }
    }
}
