use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_clipboard_manager::ClipboardExt;

// Flag to skip file watcher emission after our own saves
static SKIP_NEXT_EMIT: AtomicBool = AtomicBool::new(false);

pub use brainstorm_types::{Board, Edge, LinkPreview, Node};

fn get_board_path(_app: &AppHandle) -> PathBuf {
    // Use parent of src-tauri (project root) during dev, or current dir in production
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // If we're in src-tauri, go up one level to project root
    if cwd.ends_with("src-tauri") {
        cwd.parent().unwrap_or(&cwd).join("board.json")
    } else {
        cwd.join("board.json")
    }
}

/// Pure IO core for loading a board from a concrete path. Kept free of
/// `AppHandle` so it is directly unit/integration testable (see
/// `tests/board_roundtrip.rs`).
///
/// - A missing file yields an empty `Board` (we don't create the file until the
///   user saves), mirroring the `load_board` command's behavior.
/// - Malformed JSON returns `Err` rather than silently swallowing it into an
///   empty board (see P0.1 / F73).
pub fn load_board_at(path: &std::path::Path) -> Result<Board, String> {
    if !path.exists() {
        return Ok(Board::default());
    }

    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let board: Board = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(board)
}

#[tauri::command]
fn load_board(app: AppHandle) -> Result<Board, String> {
    let path = get_board_path(&app);
    load_board_at(&path)
}

/// Atomically write a board to `path`.
///
/// Strategy: serialize JSON, write it to a sibling temp file (`<path>.tmp`) in
/// the SAME directory, `fsync` it, then `rename` it over `path`. Because rename
/// is atomic on the same filesystem, readers (and the file watcher) never
/// observe a partially-written file. Before the rename, the prior on-disk
/// contents are copied to `<path>.bak` (best-effort). The `SKIP_NEXT_EMIT` flag
/// is set immediately before the rename — the atomic commit point — so the file
/// watcher's debounce window only opens once the new contents are visible.
pub fn write_board_atomic(path: &std::path::Path, board: &Board) -> Result<(), String> {
    use std::io::Write;

    // Create parent directory if needed (only on actual save, not on load)
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    // Compact (single-line) JSON: board.json is primarily machine-read by agents,
    // so we drop pretty-printing to keep writes small and fast.
    let json = serde_json::to_string(board).map_err(|e| e.to_string())?;

    // Write the serialized JSON to a sibling temp file in the same directory.
    let tmp_path = {
        let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(".tmp");
        path.with_file_name(name)
    };

    {
        let mut file = fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
        file.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        // fsync: flush the temp file's contents to disk before the rename so a
        // crash mid-write can't leave a truncated file at the final path.
        file.sync_all().map_err(|e| e.to_string())?;
    }

    // Best-effort backup of the prior on-disk contents. Ignore failures (e.g.
    // no prior file yet, or a permissions hiccup) — the backup is advisory.
    if path.exists() {
        let bak_path = {
            let mut name = path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_default();
            name.push(".bak");
            path.with_file_name(name)
        };
        let _ = fs::copy(path, &bak_path);
    }

    // Set the skip flag at the atomic commit point — immediately before rename.
    SKIP_NEXT_EMIT.store(true, Ordering::SeqCst);

    fs::rename(&tmp_path, path).map_err(|e| {
        // The rename failed, so we never actually committed — undo the skip flag
        // and clean up the temp file so we don't leave litter behind.
        SKIP_NEXT_EMIT.store(false, Ordering::SeqCst);
        let _ = fs::remove_file(&tmp_path);
        e.to_string()
    })?;

    Ok(())
}

#[tauri::command]
fn save_board(app: AppHandle, board: Board) -> Result<(), String> {
    let path = get_board_path(&app);
    write_board_atomic(&path, &board)
}

#[tauri::command]
fn get_board_path_cmd(app: AppHandle) -> Result<String, String> {
    let path = get_board_path(&app);
    Ok(path.to_string_lossy().to_string())
}

/// Maximum number of redirect hops we will follow before giving up. Each hop is
/// independently re-resolved and re-checked against the IP policy, so this is a
/// hard bound on the redirect chain a malicious server can drive us through.
const MAX_REDIRECTS: usize = 3;

/// Maximum HTML body size we will buffer before parsing. A crafted (or
/// compromised) server could otherwise stream gigabytes and OOM the process.
const MAX_PREVIEW_BODY_BYTES: usize = 2 * 1024 * 1024; // 2 MB

/// Return `true` if `addr` points at a host we must never issue a server-side
/// request to: loopback, link-local, RFC1918 private ranges, CGNAT (100.64/10),
/// or IPv6 ULA (fc00::/7). Enforced at the *resolved IP* level (not the
/// hostname) so it is robust against DNS rebinding and obfuscated literals.
///
/// Pure function — unit-tested directly.
fn is_blocked_ip(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_blocked_ipv4(v4),
        IpAddr::V6(v6) => {
            // Normalize IPv4-mapped (::ffff:a.b.c.d) addresses to their v4 form
            // and apply the v4 policy, so an attacker can't tunnel 127.0.0.1
            // through [::ffff:127.0.0.1]. We deliberately do NOT use
            // `to_ipv4()`, which also matches deprecated IPv4-*compatible*
            // addresses (e.g. `::1` -> `0.0.0.1`) and would mis-route the IPv6
            // loopback away from the v6 check below.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_ipv4(v4);
            }
            is_blocked_ipv6(v6)
        }
    }
}

fn is_blocked_ipv4(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    v4.is_loopback()              // 127.0.0.0/8
        || v4.is_link_local()     // 169.254.0.0/16
        || v4.is_private()        // 10/8, 172.16/12, 192.168/16
        || v4.is_unspecified()    // 0.0.0.0
        || v4.is_broadcast()      // 255.255.255.255
        || (o[0] == 100 && (o[1] & 0xc0) == 0x40) // 100.64.0.0/10 (CGNAT)
}

fn is_blocked_ipv6(v6: Ipv6Addr) -> bool {
    v6.is_loopback()              // ::1
        || v6.is_unspecified()    // ::
        || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 (link-local)
        || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7  (ULA)
}

/// Resolve `host:port` and reject if any resolved address is blocked. Returns
/// `Ok(())` only when the host resolves to at least one address and *every*
/// resolved address is publicly routable. We reject if ANY address is blocked,
/// so a hostname with mixed public/private records can't be used to slip past
/// the check via record ordering.
fn check_host_allowed(host: &str, port: u16) -> Result<(), String> {
    // A bare IP literal still goes through ToSocketAddrs, which parses it
    // without a DNS round-trip — covers decimal/hex/mapped literals once parsed.
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve host: {}", e))?
        .collect();

    if addrs.is_empty() {
        return Err("Host did not resolve to any address".to_string());
    }

    for sa in &addrs {
        if is_blocked_ip(sa.ip()) {
            return Err(format!(
                "Refusing to fetch internal/private address ({})",
                sa.ip()
            ));
        }
    }

    Ok(())
}

/// Validate the host of a parsed URL against the IP policy. Extracts the host
/// and effective port, then defers to `check_host_allowed`. Decimal/hex/mapped
/// literal hosts are normalized by `Url`/`ToSocketAddrs` parsing before the
/// check, so `http://2130706433/` and `http://[::ffff:127.0.0.1]/` are rejected.
fn validate_url_host(parsed: &reqwest::Url) -> Result<(), String> {
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    // Strip brackets from IPv6 literals so ToSocketAddrs parses them.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "URL has no port".to_string())?;
    check_host_allowed(host, port)
}

#[tauri::command]
async fn fetch_link_preview(url: String) -> Result<LinkPreview, String> {
    // Skip non-HTTP URLs (file://, etc.)
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Ok(LinkPreview {
            url: url.clone(),
            title: Some(url),
            description: None,
            image: None,
            site_name: Some("Local File".to_string()),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        // Disable reqwest's automatic redirect handling: we follow redirects
        // manually so we can re-resolve and re-validate every hop's host
        // against the IP policy (DNS-rebinding-safe). The hop count is capped
        // by MAX_REDIRECTS.
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .map_err(|e| e.to_string())?;

    let mut current_url = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    let mut response;
    let mut hops = 0usize;

    loop {
        // Re-validate the host on every hop, including the initial request, so
        // a redirect to an internal address (or a rebind between resolution and
        // connection) is rejected before any request leaves the machine.
        validate_url_host(&current_url)?;

        response = client
            .get(current_url.clone())
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if response.status().is_redirection() {
            if hops >= MAX_REDIRECTS {
                return Err("Too many redirects".to_string());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "Redirect without Location header".to_string())?;
            // Resolve the (possibly relative) Location against the current URL.
            current_url = current_url
                .join(location)
                .map_err(|e| format!("Invalid redirect target: {}", e))?;
            // Only http(s) redirects are followed; reject scheme downgrades to
            // file://, data://, etc.
            if current_url.scheme() != "http" && current_url.scheme() != "https" {
                return Err("Refusing to follow non-http redirect".to_string());
            }
            hops += 1;
            continue;
        }

        break;
    }

    // Stream the body with a running byte cap so a huge response cannot OOM the
    // process before we parse it.
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if buf.len() + chunk.len() > MAX_PREVIEW_BODY_BYTES {
            let remaining = MAX_PREVIEW_BODY_BYTES - buf.len();
            buf.extend_from_slice(&chunk[..remaining]);
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    let html = String::from_utf8_lossy(&buf).into_owned();
    let document = Html::parse_document(&html);

    // Selectors for Open Graph and fallback meta tags
    let og_title = Selector::parse(r#"meta[property="og:title"]"#).ok();
    let og_desc = Selector::parse(r#"meta[property="og:description"]"#).ok();
    let og_image = Selector::parse(r#"meta[property="og:image"]"#).ok();
    let og_site = Selector::parse(r#"meta[property="og:site_name"]"#).ok();
    let meta_desc = Selector::parse(r#"meta[name="description"]"#).ok();
    let title_tag = Selector::parse("title").ok();
    let twitter_image = Selector::parse(r#"meta[name="twitter:image"]"#).ok();

    let get_content = |sel: Option<Selector>| -> Option<String> {
        sel.and_then(|s| {
            document
                .select(&s)
                .next()
                .and_then(|el| el.value().attr("content").map(|s| s.to_string()))
        })
    };

    let title = get_content(og_title).or_else(|| {
        title_tag.and_then(|s| document.select(&s).next().map(|el| el.text().collect()))
    });

    let description = get_content(og_desc.clone()).or_else(|| get_content(meta_desc));

    let mut image = get_content(og_image).or_else(|| get_content(twitter_image));

    // Make relative image URLs absolute against the final (post-redirect) URL.
    if let Some(ref img) = image {
        if img.starts_with('/') {
            if let Ok(absolute) = current_url.join(img) {
                image = Some(absolute.to_string());
            }
        }
    }

    let site_name = get_content(og_site);

    Ok(LinkPreview {
        url,
        title,
        description,
        image,
        site_name,
    })
}

fn get_assets_dir(app: &AppHandle) -> PathBuf {
    let board_path = get_board_path(app);
    let parent = board_path.parent().unwrap_or(&board_path);
    parent.join("assets")
}

/// Maximum byte size for an image we will base64-encode and hand to the
/// webview. Guards against memory exhaustion / DoS via a crafted board.json
/// pointing at a huge file.
const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024; // 25 MB

/// Expand `~`, strip a `file://` prefix, and URL-decode an input path string
/// into a concrete filesystem path. Shared by the file-reading commands so
/// their accepted path syntax stays consistent.
fn expand_path(path: &str) -> PathBuf {
    let expanded = if let Some(rest) = path.strip_prefix('~') {
        match dirs::home_dir() {
            Some(home) => home.join(rest.trim_start_matches('/')).to_string_lossy().into_owned(),
            None => path.to_string(),
        }
    } else if let Some(stripped) = path.strip_prefix("file://") {
        urlencoding::decode(stripped)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| stripped.to_string())
    } else {
        path.to_string()
    };
    PathBuf::from(expanded)
}

/// Resolve `input` to a canonical path and reject it unless it lives inside one
/// of `allowed_roots`. Canonicalization (which also resolves `..` and symlinks)
/// is what stops path-traversal exfiltration: a crafted board.json pointing at
/// `/etc/passwd`, `~/.ssh/id_rsa`, or `<board-dir>/../../secret` resolves to a
/// path that does not start with any allowed root, so we return `Err`.
fn scope_path(input: &str, allowed_roots: &[PathBuf]) -> Result<PathBuf, String> {
    let expanded = expand_path(input);

    let canonical = expanded
        .canonicalize()
        .map_err(|_| format!("File not found: {}", expanded.display()))?;

    let in_scope = allowed_roots.iter().any(|root| {
        root.canonicalize()
            .map(|r| canonical.starts_with(&r))
            .unwrap_or(false)
    });

    if !in_scope {
        return Err("Access denied: path is outside the allowed directories".to_string());
    }

    Ok(canonical)
}

/// The directory that holds the active `board.json`. Local images and assets
/// referenced by the board are scoped to live inside this directory.
fn board_dir(app: &AppHandle) -> PathBuf {
    let board_path = get_board_path(app);
    board_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Sniff the leading magic bytes of `data` and return the matching image MIME
/// type, or `None` if the content is not a supported image format. We trust the
/// file *content*, not its extension — a crafted board.json can rename
/// `/etc/passwd` to `evil.png`, but its bytes won't match any signature here.
fn sniff_image_mime(data: &[u8]) -> Option<&'static str> {
    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    // JPEG: FF D8 FF
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    // GIF: "GIF87a" or "GIF89a"
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // WebP: "RIFF" .... "WEBP"
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // BMP: "BM"
    if data.starts_with(b"BM") {
        return Some("image/bmp");
    }
    None
}

fn ensure_assets_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let assets_dir = get_assets_dir(app);
    if !assets_dir.exists() {
        fs::create_dir_all(&assets_dir).map_err(|e| format!("Failed to create assets dir: {}", e))?;
    }
    Ok(assets_dir)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PasteImageResult {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// Validate, read, and base64-encode an image. Pure (no AppHandle) so it can be
/// unit-tested: the caller passes the directories the path is allowed to live in.
fn read_image_base64_scoped(path: &str, allowed_roots: &[PathBuf]) -> Result<String, String> {
    // Reject any path that resolves outside the allowed roots (path traversal,
    // absolute paths to system files, etc.).
    let canonical = scope_path(path, allowed_roots)?;

    // Cap file size BEFORE reading the bytes into memory.
    let meta = fs::metadata(&canonical)
        .map_err(|e| format!("Failed to stat file: {}", e))?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "Image too large: {} bytes (max {} bytes)",
            meta.len(),
            MAX_IMAGE_BYTES
        ));
    }

    let data = fs::read(&canonical)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Derive MIME from detected magic bytes, not the file extension. Reject any
    // file whose content is not a supported image format.
    let mime = sniff_image_mime(&data)
        .ok_or_else(|| "Unsupported or non-image file content".to_string())?;

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let b64 = STANDARD.encode(&data);

    Ok(format!("data:{};base64,{}", mime, b64))
}

#[tauri::command]
fn read_image_base64(app: AppHandle, path: String) -> Result<String, String> {
    read_image_base64_scoped(&path, &[board_dir(&app)])
}

/// Validate and read a local Markdown file. Pure (no AppHandle) so it can be
/// unit-tested. Only `.md` files inside an allowed root are readable — this both
/// preserves the Obsidian-vault integration (vault files live under `$HOME`) and
/// blocks exfiltration of non-Markdown system files like `/etc/passwd` or
/// `~/.ssh/id_rsa`.
fn read_markdown_file_scoped(path: &str, allowed_roots: &[PathBuf]) -> Result<String, String> {
    let canonical = scope_path(path, allowed_roots)?;

    let is_md = canonical
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false);
    if !is_md {
        return Err("Access denied: only .md files can be read".to_string());
    }

    std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read {}: {}", canonical.display(), e))
}

#[tauri::command]
fn read_markdown_file(app: AppHandle, path: String) -> Result<String, String> {
    let mut roots = vec![board_dir(&app)];
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    read_markdown_file_scoped(&path, &roots)
}

#[tauri::command]
fn delete_asset(_app: AppHandle, path: String) -> Result<(), String> {
    let file_path = PathBuf::from(&path);

    // Only allow deleting files in the assets folder (safety check)
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let assets_dir = cwd.join("assets");

    // Canonicalize paths to prevent path traversal attacks
    let canonical_file = file_path.canonicalize()
        .map_err(|_| "File not found".to_string())?;
    let canonical_assets = assets_dir.canonicalize()
        .map_err(|_| "Assets folder not found".to_string())?;

    if !canonical_file.starts_with(&canonical_assets) {
        return Err("Can only delete files from assets folder".to_string());
    }

    fs::remove_file(&canonical_file)
        .map_err(|e| format!("Failed to delete file: {}", e))?;

    Ok(())
}

#[tauri::command]
fn paste_image(app: AppHandle) -> Result<PasteImageResult, String> {
    let clipboard = app.clipboard();

    // Try to read image from clipboard
    if let Ok(tauri_image) = clipboard.read_image() {
        let width = tauri_image.width();
        let height = tauri_image.height();
        let rgba_data = tauri_image.rgba();

        // Convert RGBA to PNG using image crate
        let img_buffer: image::RgbaImage = image::ImageBuffer::from_raw(width, height, rgba_data.to_vec())
            .ok_or_else(|| "Failed to create image buffer".to_string())?;

        // Generate unique filename
        let filename = format!("{}.png", uuid::Uuid::new_v4());
        let assets_dir = ensure_assets_dir(&app)?;
        let dest_path = assets_dir.join(&filename);

        // Save as PNG
        img_buffer.save_with_format(&dest_path, image::ImageFormat::Png)
            .map_err(|e| format!("Failed to save image: {}", e))?;

        return Ok(PasteImageResult {
            path: dest_path.to_string_lossy().to_string(),
            width,
            height,
        });
    }

    // Try to read text (might be a file path)
    if let Ok(text) = clipboard.read_text() {
        let text = text.trim();

        // Check if it's a file path to an image
        let path = PathBuf::from(text);
        if path.exists() && path.is_file() {
            let ext = path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if ["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&ext.as_str()) {
                // Read and decode to get dimensions
                let data = fs::read(&path)
                    .map_err(|e| format!("Failed to read file: {}", e))?;
                let img = image::load_from_memory(&data)
                    .map_err(|e| format!("Failed to decode image: {}", e))?;

                let width = img.width();
                let height = img.height();

                // Copy to assets folder
                let filename = format!("{}.png", uuid::Uuid::new_v4());
                let assets_dir = ensure_assets_dir(&app)?;
                let dest_path = assets_dir.join(&filename);

                // Save as PNG to normalize format
                img.save_with_format(&dest_path, image::ImageFormat::Png)
                    .map_err(|e| format!("Failed to save image: {}", e))?;

                return Ok(PasteImageResult {
                    path: dest_path.to_string_lossy().to_string(),
                    width,
                    height,
                });
            }
        }
    }

    Err("No image found in clipboard".to_string())
}

/// Pure decision core for the file watcher: given a `board.json` change event,
/// decide whether to emit a `board-changed` notification to the frontend.
///
/// Two rules, in order:
/// 1. **Skip-our-own-save**: if `skip` is set (the app just wrote the file via
///    `write_board_atomic`, which flips `SKIP_NEXT_EMIT`), we never emit — that
///    change originated from us and re-emitting would cause a reload feedback
///    loop. The caller is responsible for *consuming* (swapping) the flag.
/// 2. **Debounce**: a single save can produce several filesystem events. We only
///    emit if at least `debounce` has elapsed since `last_emit` (or there was no
///    prior emit). `now == last_emit + debounce` exactly is treated as elapsed.
///
/// Pure (no globals, no `AppHandle`, no clock) so it is unit-tested directly in
/// `tests/watcher.rs`.
pub fn should_emit_change(
    skip: bool,
    last_emit: Option<std::time::Instant>,
    now: std::time::Instant,
    debounce: Duration,
) -> bool {
    if skip {
        return false;
    }
    match last_emit {
        Some(t) => now.duration_since(t) >= debounce,
        None => true,
    }
}

fn setup_file_watcher(app: AppHandle) {
    let board_path = get_board_path(&app);
    // Don't create board.json here - let user create it by adding nodes

    std::thread::spawn(move || {
        let (tx, rx) = channel();

        let mut watcher: RecommendedWatcher = Watcher::new(
            tx,
            Config::default().with_poll_interval(Duration::from_millis(500)),
        )
        .expect("Failed to create watcher");

        if let Some(parent) = board_path.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .expect("Failed to watch directory");
        }

        // Debounce: track last emit time to avoid multiple emissions for one save
        let mut last_emit: Option<std::time::Instant> = None;
        let debounce_duration = Duration::from_millis(500);

        loop {
            match rx.recv() {
                Ok(event) => {
                    if let Ok(event) = event {
                        let is_board_file = event.paths.iter().any(|p| {
                            p.file_name()
                                .map(|n| n == "board.json")
                                .unwrap_or(false)
                        });

                        if is_board_file {
                            match event.kind {
                                notify::EventKind::Modify(_) | notify::EventKind::Create(_) => {
                                    // Consume the skip flag (set by our own save) and let the
                                    // pure decision core apply the skip + debounce rules.
                                    let was_skip_set = SKIP_NEXT_EMIT.swap(false, Ordering::SeqCst);
                                    let now = std::time::Instant::now();

                                    if should_emit_change(
                                        was_skip_set,
                                        last_emit,
                                        now,
                                        debounce_duration,
                                    ) {
                                        last_emit = Some(now);
                                        std::thread::sleep(Duration::from_millis(100));
                                        let _ = app.emit("board-changed", ());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Watch error: {:?}", e);
                    break;
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            setup_file_watcher(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![load_board, save_board, get_board_path_cmd, fetch_link_preview, paste_image, read_image_base64, read_markdown_file, delete_asset])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    mod read_markdown_file_tests {
        use super::*;

        // Each test owns a fresh `tempfile::tempdir()` so parallel runs never
        // collide on a shared fixed file name (was the F80 race). The TempDir is
        // RAII-cleaned on drop — no manual removal needed.

        #[test]
        fn reads_absolute_path() {
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            let path = dir.path().join("test_read_absolute.md");
            let content = "# Test\nHello world";
            std::fs::write(&path, content).unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &roots);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);
        }

        #[test]
        fn reads_file_url() {
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            let path = dir.path().join("test_read_file_url.md");
            let content = "# File URL Test";
            std::fs::write(&path, content).unwrap();

            let file_url = format!("file://{}", path.to_string_lossy());
            let result = read_markdown_file_scoped(&file_url, &roots);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);
        }

        #[test]
        fn decodes_url_encoded_spaces() {
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            let subdir = dir.path().join("test folder");
            std::fs::create_dir_all(&subdir).ok();
            let path = subdir.join("test file.md");
            let content = "# Spaces in path";
            std::fs::write(&path, content).unwrap();

            // URL encode the path with %20 for spaces
            let encoded_path = format!(
                "file://{}",
                path.to_string_lossy().replace(' ', "%20")
            );
            let result = read_markdown_file_scoped(&encoded_path, &roots);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);
        }

        #[test]
        fn expands_home_tilde() {
            // This test verifies tilde expansion works. It must write to the real
            // home directory (that's what `~` resolves to), so the file name is
            // UUID-suffixed to stay unique across parallel test binaries.
            let home = dirs::home_dir().unwrap();
            let name = format!(".brainstorm_test_temp_{}.md", uuid::Uuid::new_v4());
            let test_file = home.join(&name);
            let content = "# Home test";
            std::fs::write(&test_file, content).unwrap();

            let result = read_markdown_file_scoped(&format!("~/{}", name), &[home]);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);

            std::fs::remove_file(&test_file).ok();
        }

        #[test]
        fn returns_error_for_nonexistent_file() {
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            let missing = dir.path().join("does_not_exist.md");
            let result = read_markdown_file_scoped(&missing.to_string_lossy(), &roots);
            assert!(result.is_err());
        }

        #[test]
        fn handles_unicode_content() {
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            let path = dir.path().join("test_unicode.md");
            let content = "# Unicode Test\n日本語 中文 한국어 🎉";
            std::fs::write(&path, content).unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &roots);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);
        }

        #[test]
        fn handles_empty_file() {
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            let path = dir.path().join("test_empty.md");
            std::fs::write(&path, "").unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &roots);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "");
        }

        #[test]
        fn handles_multiple_encoded_characters() {
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            // Create a path with multiple special chars that need encoding
            let subdir = dir.path().join("test & folder");
            std::fs::create_dir_all(&subdir).ok();
            let path = subdir.join("notes (copy).md");
            let content = "# Special chars in path";
            std::fs::write(&path, content).unwrap();

            // URL encode special characters
            let encoded_path = format!(
                "file://{}",
                path.to_string_lossy()
                    .replace(' ', "%20")
                    .replace('&', "%26")
                    .replace('(', "%28")
                    .replace(')', "%29")
            );
            let result = read_markdown_file_scoped(&encoded_path, &roots);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), content);
        }

        #[test]
        fn rejects_md_file_outside_allowed_roots() {
            // A .md file that exists but lives outside the allowed roots is denied.
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test_outside_scope.md");
            std::fs::write(&path, "# secret").unwrap();

            // Allowed root is an unrelated subdirectory, NOT the dir holding the file.
            let other_root = dir.path().join("brainstorm_unrelated_root");
            std::fs::create_dir_all(&other_root).ok();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &[other_root.clone()]);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("Access denied"));
        }

        #[test]
        fn rejects_non_md_file_inside_allowed_roots() {
            // Even inside an allowed root, a non-.md file (e.g. an imitation of a
            // system secret) is rejected by the extension guard.
            let dir = tempfile::tempdir().unwrap();
            let roots = vec![dir.path().to_path_buf()];
            let path = dir.path().join("test_secret_creds.txt");
            std::fs::write(&path, "topsecret").unwrap();

            let result = read_markdown_file_scoped(&path.to_string_lossy(), &roots);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("only .md"));
        }
    }

    mod path_scope_tests {
        use super::*;

        #[test]
        fn rejects_etc_passwd_for_image_read() {
            // /etc/passwd is outside any board dir and not image bytes anyway.
            let dir = tempfile::tempdir().unwrap();
            let board = dir.path().to_path_buf();

            let result = read_image_base64_scoped("/etc/passwd", &[board]);
            assert!(result.is_err(), "/etc/passwd must be rejected");
        }

        #[test]
        fn rejects_ssh_key_for_markdown_read() {
            let home = dirs::home_dir().unwrap();
            // ~/.ssh/id_rsa: even if it existed under the home root, it is not a
            // .md file, so the extension guard rejects it. And on machines where
            // it doesn't exist, canonicalize fails first. Either way -> Err.
            let result = read_markdown_file_scoped("~/.ssh/id_rsa", &[home]);
            assert!(result.is_err(), "~/.ssh/id_rsa must be rejected");
        }

        #[test]
        fn rejects_path_traversal_escape() {
            // A path that climbs out of the board dir via `..` must be rejected
            // because canonicalization resolves it outside the allowed root.
            let dir = tempfile::tempdir().unwrap();
            let board = dir.path().join("board");
            std::fs::create_dir_all(&board).ok();

            // Create a real .md file OUTSIDE the board dir, then reference it via ..
            let secret = dir.path().join("brainstorm_outside_secret.md");
            std::fs::write(&secret, "# leak").unwrap();

            let traversal = format!("{}/../brainstorm_outside_secret.md", board.to_string_lossy());
            let result = read_markdown_file_scoped(&traversal, &[board.clone()]);
            assert!(result.is_err(), "traversal escape must be rejected");
        }

        #[test]
        fn allows_image_inside_board_dir() {
            // A real PNG inside the board dir is accepted and base64-encoded.
            let dir = tempfile::tempdir().unwrap();
            let board = dir.path().to_path_buf();

            // Minimal valid PNG magic-byte header (signature is enough for sniffing).
            let png_sig = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01];
            let img = board.join("pic.png");
            std::fs::write(&img, png_sig).unwrap();

            let result = read_image_base64_scoped(&img.to_string_lossy(), &[board.clone()]);
            assert!(result.is_ok(), "board-dir image should load: {:?}", result);
            assert!(result.unwrap().starts_with("data:image/png;base64,"));
        }

        #[test]
        fn rejects_non_image_content_with_image_extension() {
            // A text file renamed to .png is rejected by magic-byte sniffing.
            let dir = tempfile::tempdir().unwrap();
            let board = dir.path().to_path_buf();

            let fake = board.join("evil.png");
            std::fs::write(&fake, b"root:x:0:0:root:/root:/bin/bash\n").unwrap();

            let result = read_image_base64_scoped(&fake.to_string_lossy(), &[board.clone()]);
            assert!(result.is_err(), "non-image content must be rejected");
            assert!(result.unwrap_err().contains("non-image"));
        }

        #[test]
        fn rejects_oversized_image() {
            let dir = tempfile::tempdir().unwrap();
            let board = dir.path().to_path_buf();

            // Write a file larger than the cap. Start with a PNG signature so the
            // size check (which runs first) is unambiguously what rejects it.
            let big = board.join("huge.png");
            let mut data = vec![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
            data.resize((MAX_IMAGE_BYTES + 1) as usize, 0u8);
            std::fs::write(&big, &data).unwrap();

            let result = read_image_base64_scoped(&big.to_string_lossy(), &[board.clone()]);
            assert!(result.is_err(), "oversized image must be rejected");
            assert!(result.unwrap_err().contains("too large"));
        }

        #[test]
        fn sniff_detects_supported_formats() {
            assert_eq!(sniff_image_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]), Some("image/png"));
            assert_eq!(sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
            assert_eq!(sniff_image_mime(b"GIF89a..."), Some("image/gif"));
            assert_eq!(sniff_image_mime(b"GIF87a..."), Some("image/gif"));
            assert_eq!(sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
            assert_eq!(sniff_image_mime(b"BM\0\0"), Some("image/bmp"));
            assert_eq!(sniff_image_mime(b"not an image"), None);
            assert_eq!(sniff_image_mime(&[]), None);
        }
    }

    mod ssrf_tests {
        use super::*;
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
        use std::str::FromStr;

        fn v4(s: &str) -> IpAddr {
            IpAddr::V4(Ipv4Addr::from_str(s).unwrap())
        }
        fn v6(s: &str) -> IpAddr {
            IpAddr::V6(Ipv6Addr::from_str(s).unwrap())
        }

        #[test]
        fn blocks_loopback() {
            assert!(is_blocked_ip(v4("127.0.0.1")));
            assert!(is_blocked_ip(v4("127.255.255.255")));
            assert!(is_blocked_ip(v6("::1")));
        }

        #[test]
        fn blocks_link_local() {
            // 169.254.0.0/16 includes the cloud metadata endpoint 169.254.169.254
            assert!(is_blocked_ip(v4("169.254.0.1")));
            assert!(is_blocked_ip(v4("169.254.169.254")));
            // fe80::/10
            assert!(is_blocked_ip(v6("fe80::1")));
            assert!(is_blocked_ip(v6("febf::1")));
        }

        #[test]
        fn blocks_rfc1918_private() {
            assert!(is_blocked_ip(v4("10.0.0.1")));
            assert!(is_blocked_ip(v4("10.255.255.255")));
            assert!(is_blocked_ip(v4("172.16.0.1")));
            assert!(is_blocked_ip(v4("172.31.255.255")));
            assert!(is_blocked_ip(v4("192.168.1.1")));
        }

        #[test]
        fn blocks_cgnat() {
            // 100.64.0.0/10 spans 100.64.0.0 .. 100.127.255.255
            assert!(is_blocked_ip(v4("100.64.0.1")));
            assert!(is_blocked_ip(v4("100.127.255.255")));
        }

        #[test]
        fn blocks_ula_and_unspecified() {
            // fc00::/7 (fc00:: .. fdff::)
            assert!(is_blocked_ip(v6("fc00::1")));
            assert!(is_blocked_ip(v6("fd12:3456::1")));
            assert!(is_blocked_ip(v4("0.0.0.0")));
            assert!(is_blocked_ip(v6("::")));
            assert!(is_blocked_ip(v4("255.255.255.255")));
        }

        #[test]
        fn blocks_ipv4_mapped_loopback() {
            // ::ffff:127.0.0.1 must normalize to the v4 loopback and be blocked.
            assert!(is_blocked_ip(v6("::ffff:127.0.0.1")));
            assert!(is_blocked_ip(v6("::ffff:10.0.0.1")));
            assert!(is_blocked_ip(v6("::ffff:169.254.169.254")));
        }

        #[test]
        fn allows_public_addresses() {
            assert!(!is_blocked_ip(v4("8.8.8.8")));
            assert!(!is_blocked_ip(v4("1.1.1.1")));
            assert!(!is_blocked_ip(v4("172.15.255.255"))); // just below 172.16/12
            assert!(!is_blocked_ip(v4("172.32.0.1"))); // just above 172.16/12
            assert!(!is_blocked_ip(v4("100.63.255.255"))); // just below CGNAT
            assert!(!is_blocked_ip(v4("100.128.0.0"))); // just above CGNAT
            assert!(!is_blocked_ip(v4("93.184.216.34"))); // example.com
            assert!(!is_blocked_ip(v6("2606:4700:4700::1111"))); // cloudflare
        }

        #[test]
        fn check_host_allowed_rejects_loopback_literal() {
            assert!(check_host_allowed("127.0.0.1", 80).is_err());
            assert!(check_host_allowed("169.254.169.254", 80).is_err());
        }

        #[test]
        fn check_host_allowed_rejects_decimal_literal() {
            // http://2130706433/ == 127.0.0.1
            assert!(check_host_allowed("2130706433", 80).is_err());
        }

        #[test]
        fn validate_url_host_rejects_metadata_endpoint() {
            let u = reqwest::Url::parse("http://169.254.169.254/latest/meta-data/").unwrap();
            assert!(validate_url_host(&u).is_err());
        }

        #[test]
        fn validate_url_host_rejects_decimal_loopback() {
            let u = reqwest::Url::parse("http://2130706433/").unwrap();
            assert!(validate_url_host(&u).is_err());
        }

        #[test]
        fn validate_url_host_rejects_ipv6_mapped_loopback() {
            let u = reqwest::Url::parse("http://[::ffff:127.0.0.1]/").unwrap();
            assert!(validate_url_host(&u).is_err());
        }

        #[test]
        fn validate_url_host_rejects_localhost() {
            let u = reqwest::Url::parse("http://localhost:8080/admin").unwrap();
            assert!(validate_url_host(&u).is_err());
        }
    }
}
