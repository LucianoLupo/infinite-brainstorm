# Image, Markdown & Link Nodes

---
category: features
component: canvas
tags:
  - image-rendering
  - markdown
  - canvas2d
  - html-overlay
  - zoom
  - modal
  - link-preview
  - open-graph
  - canvas-clipping
  - clipboard
related_files:
  - src/canvas.rs
  - src/app.rs
  - src-tauri/src/lib.rs
  - src-tauri/tauri.conf.json
  - Cargo.toml
  - src-tauri/Cargo.toml
date_solved: 2025-01-31
---

## Problem

The infinite canvas needed support for three new node types:
1. **Image nodes** - Display image thumbnails with full-screen modal preview
2. **Markdown nodes** - Render formatted markdown content with edit capability
3. **Link nodes** - Display URL preview cards with Open Graph metadata

Key challenges:
- Loading images asynchronously in WASM environment
- Accessing local files through Tauri's security model
- Rendering markdown in Canvas2D (complex text formatting)
- Maintaining zoom synchronization for HTML overlays
- Fetching and caching Open Graph metadata from URLs
- Preventing canvas content from overflowing node boundaries

## Solution

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Canvas Layer                              │
│  - Image nodes: Draw HtmlImageElement directly               │
│  - MD nodes: Draw background/border only                     │
│  - Link nodes: Draw OG image + domain with clipping          │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│                   HTML Overlay Layer                         │
│  - MD content: Positioned divs with rendered HTML            │
│  - Uses CSS transform for zoom synchronization               │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│                    Modal Layer                               │
│  - Image modal: Full-screen image view                       │
│  - MD modal: View/edit modes with markdown preview           │
└─────────────────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────────────────┐
│                    Backend (Tauri)                           │
│  - fetch_link_preview: Fetches OG metadata via reqwest       │
└─────────────────────────────────────────────────────────────┘
```

### Image Nodes

**Image Cache Pattern** (canvas.rs):
```rust
pub type ImageCache = Rc<RefCell<HashMap<String, Option<HtmlImageElement>>>>;
```

**Async Image Loading** (app.rs):
```rust
Effect::new(move || {
    let b = board.get();
    let cache = image_cache.clone();

    for node in b.nodes.iter().filter(|n| n.node_type == "image") {
        let url = node.text.clone();
        if cache.borrow().contains_key(&url) {
            continue;
        }

        // Mark as loading
        cache.borrow_mut().insert(url.clone(), None);

        let img = HtmlImageElement::new().unwrap();
        let final_url = if url.starts_with("/") || url.starts_with("~") {
            format!("asset://localhost{}", url.replace("~", "/Users/lucianolupo"))
        } else {
            url.clone()
        };

        // Setup onload callback
        let cache_clone = cache.clone();
        let url_clone = url.clone();
        let img_clone = img.clone();
        let onload = Closure::wrap(Box::new(move || {
            cache_clone.borrow_mut().insert(url_clone.clone(), Some(img_clone.clone()));
        }) as Box<dyn Fn()>);

        img.set_onload(Some(onload.as_ref().unchecked_ref()));
        onload.forget();
        img.set_src(&final_url);
    }
});
```

**Drawing with Zoom Support** (canvas.rs):
```rust
fn draw_image_content(...) {
    // Scale to fit available space, allowing upscaling when zoomed in
    let scale = (img_max_w / natural_w).min(img_max_h / natural_h);
    // Note: No .min(1.0) cap - allows images to scale up with zoom
    let draw_w = natural_w * scale;
    let draw_h = natural_h * scale;

    ctx.draw_image_with_html_image_element_and_dw_and_dh(
        img, img_x + offset_x, img_y + offset_y, draw_w, draw_h
    );
}
```

### Markdown Nodes

**HTML Overlay with CSS Transform for Zoom** (app.rs):
```rust
// MD overlays - use CSS transform for uniform scaling
{move || {
    let b = board.get();
    let cam = camera.get();

    b.nodes.iter()
        .filter(|n| n.node_type == "md")
        .map(|node| {
            let (sx, sy) = cam.world_to_screen(node.x, node.y);
            let html_content = parse_markdown(&node.text);

            view! {
                <div
                    style=format!(
                        "position: absolute; left: {}px; top: {}px;
                         width: {}px; height: {}px;
                         transform: scale({}); transform-origin: top left;
                         // ... other styles",
                        sx, sy, node.width, node.height, cam.zoom
                    )
                    inner_html=html_content
                />
            }
        })
        .collect::<Vec<_>>()
}}
```

**Key insight**: Using `transform: scale(zoom)` instead of scaling font-size ensures all elements (headings, bullets, etc.) scale uniformly.

### Modal Implementation

**MD Modal with View/Edit Modes**:
```rust
// State: (node_id, content, is_editing)
let (modal_md, set_modal_md) = signal::<Option<(String, String, bool)>>(None);

// View mode: rendered markdown
// Edit mode: textarea with Save/Cancel buttons
```

### Tauri Configuration

**Enable Asset Protocol** (tauri.conf.json):
```json
{
  "security": {
    "csp": null,
    "assetProtocol": {
      "enable": true,
      "scope": ["**"]
    }
  }
}
```

### Link Nodes

Link nodes display URL preview cards with Open Graph metadata (image, title, site name).

**Link Preview Cache Pattern** (canvas.rs):
```rust
#[derive(Clone, Debug)]
pub struct LinkPreview {
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
}

pub type LinkPreviewCache = Rc<RefCell<HashMap<String, Option<LinkPreview>>>>;
```

**Backend Command** (src-tauri/src/lib.rs):
```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct LinkPreview {
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
}

#[tauri::command]
async fn fetch_link_preview(url: String) -> Result<LinkPreview, String> {
    let response = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    let html = response.text().await.map_err(|e| e.to_string())?;
    let document = scraper::Html::parse_document(&html);

    let og_selector = |property: &str| -> Option<String> {
        let selector = scraper::Selector::parse(&format!("meta[property='og:{}']", property)).ok()?;
        document.select(&selector).next()?.value().attr("content").map(|s| s.to_string())
    };

    Ok(LinkPreview {
        title: og_selector("title"),
        description: og_selector("description"),
        image: og_selector("image"),
        site_name: og_selector("site_name"),
    })
}
```

**Canvas Clipping Pattern** (canvas.rs):

The key challenge was preventing text/images from rendering outside node boundaries. Solution uses Canvas2D clipping:

```rust
fn draw_link_content(
    ctx: &CanvasRenderingContext2d,
    node: &Node,
    camera: &Camera,
    screen_x: f64, screen_y: f64,
    screen_width: f64, screen_height: f64,
    image_cache: &ImageCache,
    link_preview_cache: &LinkPreviewCache,
) {
    // Use clipping to prevent drawing outside node
    ctx.save();
    ctx.begin_path();
    ctx.rect(screen_x, screen_y, screen_width, screen_height);
    ctx.clip();

    // ... draw OG image and domain text ...

    ctx.restore();  // Remove clipping
}
```

**Click-to-Copy with web-sys Clipboard** (app.rs):
```rust
// Copy link URL to clipboard when clicking a link node
if node.node_type == "link" && !node.text.is_empty() {
    let url = node.text.clone();
    spawn_local(async move {
        if let Some(window) = web_sys::window() {
            let clipboard = window.navigator().clipboard();
            let _ = wasm_bindgen_futures::JsFuture::from(
                clipboard.write_text(&url)
            ).await;
        }
    });
}
```

**Required Cargo.toml Features**:
```toml
# Frontend (web-sys)
web-sys = { version = "0.3", features = ["Navigator", "Clipboard"] }

# Backend (src-tauri/Cargo.toml)
reqwest = { version = "0.12", features = ["rustls-tls"] }
scraper = "0.22"
```

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Canvas2D for images | Direct rendering with zoom control, no DOM overhead |
| HTML overlays for markdown | Complex text formatting impossible in Canvas2D |
| CSS transform for zoom | Uniform scaling of all HTML elements |
| Asset protocol for local files | Tauri security model requires explicit protocol |
| Image caching with Option | Distinguish between not-loaded (None in HashMap) and loading (Some(None)) |
| Canvas clipping for links | Guarantees content stays within node bounds regardless of zoom |
| Backend OG fetching | CORS prevents frontend from fetching arbitrary URLs |
| OG image as primary content | OG images typically contain title/description already |

## Gotchas

1. **Image scale cap**: Initially used `.min(1.0)` which prevented upscaling - removed to allow zoom
2. **MD font-size scaling**: Scaling font-size alone doesn't scale headings proportionally - use CSS transform
3. **Closure::forget()**: Required for async callbacks in WASM to prevent premature cleanup
4. **Local path conversion**: Must convert `/Users/...` to `asset://localhost/...` for Tauri
5. **Canvas text overflow**: Text drawn with `fill_text` can exceed boundaries - must use `ctx.clip()`
6. **Clipboard API in WASM**: `navigator().clipboard()` returns `Clipboard` directly, not `Option`

## Usage

**board.json examples**:
```json
{
  "nodes": [
    {
      "id": "img-1",
      "x": 0, "y": 0,
      "width": 220, "height": 180,
      "text": "https://picsum.photos/400/300",
      "node_type": "image"
    },
    {
      "id": "md-1",
      "x": 300, "y": 0,
      "width": 280, "height": 200,
      "text": "# Title\n\n- Item 1\n- **Bold**\n\n> Quote",
      "node_type": "md"
    },
    {
      "id": "link-1",
      "x": 650, "y": 0,
      "width": 280, "height": 200,
      "text": "https://github.com/anthropics/claude-code",
      "node_type": "link"
    }
  ]
}
```

**Interactions**:
- Double-click image node → Opens 90% viewport modal
- Double-click MD node → Opens modal with Edit button
- Click link node → Copies URL to clipboard
- Double-click link node → Opens URL in browser
- Click outside modal → Closes modal
- T key → Cycles node type (text → idea → note → image → md → link → text)
