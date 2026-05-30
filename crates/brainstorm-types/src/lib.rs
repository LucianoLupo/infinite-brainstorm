use serde::{Deserialize, Serialize};

pub const RESIZE_HANDLE_SIZE: f64 = 8.0;

/// Char-safe filename truncation for display labels.
///
/// Truncates on `char` boundaries (never byte offsets) so multibyte names
/// (emoji, CJK, accented latin) cannot panic on a mid-codepoint slice. Names with
/// more than 20 characters are cut to the first 17 chars plus an ellipsis.
pub fn truncate_filename(filename: &str) -> String {
    if filename.chars().count() > 20 {
        format!("{}...", filename.chars().take(17).collect::<String>())
    } else {
        filename.to_string()
    }
}
pub const MIN_NODE_WIDTH: f64 = 50.0;
pub const MIN_NODE_HEIGHT: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The visual/behavioral kind of a node.
///
/// Serializes lowercase (`"text"`, `"idea"`, …) to match the on-disk `board.json`
/// schema. An unrecognized value deserializes to [`NodeType::Unknown`] rather than
/// failing, preserving forward-compatibility: agents can write a future node type
/// and the app round-trips the board without dropping nodes (the kind just renders
/// with the neutral `text` fallback styling).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    #[default]
    Text,
    Idea,
    Note,
    Image,
    Md,
    Link,
    #[serde(other)]
    Unknown,
}

impl NodeType {
    /// Lowercase wire/string form, matching the serde representation.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Text => "text",
            NodeType::Idea => "idea",
            NodeType::Note => "note",
            NodeType::Image => "image",
            NodeType::Md => "md",
            NodeType::Link => "link",
            NodeType::Unknown => "unknown",
        }
    }

    /// Next type in the `T`-key cycle. Single source of truth for the progression.
    /// `Unknown` (and the tail `Link`) wrap back to `Text`.
    pub fn cycle(self) -> NodeType {
        match self {
            NodeType::Text => NodeType::Idea,
            NodeType::Idea => NodeType::Note,
            NodeType::Note => NodeType::Image,
            NodeType::Image => NodeType::Md,
            NodeType::Md => NodeType::Link,
            NodeType::Link | NodeType::Unknown => NodeType::Text,
        }
    }
}

impl std::str::FromStr for NodeType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "text" => NodeType::Text,
            "idea" => NodeType::Idea,
            "note" => NodeType::Note,
            "image" => NodeType::Image,
            "md" => NodeType::Md,
            "link" => NodeType::Link,
            _ => NodeType::Unknown,
        })
    }
}

/// The current on-disk board schema version. A board with no explicit `version`
/// key (every board written before this field existed) is treated as the current
/// version: the field is optional and `None`-means-current, so old files keep
/// loading unchanged and re-serialize without gaining a `version` key.
pub const CURRENT_BOARD_VERSION: u32 = 1;

/// A single problem found by [`Board::validate`]. Each variant carries the
/// offending id(s) so callers (the CLI, the watcher) can report precisely which
/// node or edge is at fault. `Display` renders a one-line human message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// Two or more nodes share the same `id`.
    DuplicateNodeId(String),
    /// Two or more edges share the same `id`.
    DuplicateEdgeId(String),
    /// An edge references a `from_node`/`to_node` id that no node has (dangling).
    DanglingEdge {
        edge_id: String,
        missing_node: String,
    },
    /// A node coordinate or dimension is NaN or infinite, which would break
    /// rendering and camera math.
    NonFiniteCoord { node_id: String, field: String },
    /// A node `priority` is outside the documented 1..=5 range.
    PriorityOutOfRange { node_id: String, priority: u8 },
    /// A board declares a schema `version` newer than this build understands.
    /// Non-fatal: surfaced as a forward-compat warning, not a rejection.
    UnknownVersion(u32),
}

impl ValidationError {
    /// Whether this finding is a non-fatal forward-compatibility *warning* rather
    /// than a hard structural error. Currently only [`ValidationError::UnknownVersion`]
    /// is a warning: a board declaring a future schema version still loads (we
    /// surface it so it's visible), whereas duplicate/dangling/invalid data are
    /// real errors that should fail `brainstorm validate`.
    pub fn is_warning(&self) -> bool {
        matches!(self, ValidationError::UnknownVersion(_))
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::DuplicateNodeId(id) => {
                write!(f, "duplicate node id: {id}")
            }
            ValidationError::DuplicateEdgeId(id) => {
                write!(f, "duplicate edge id: {id}")
            }
            ValidationError::DanglingEdge {
                edge_id,
                missing_node,
            } => write!(
                f,
                "dangling edge {edge_id}: references missing node {missing_node}"
            ),
            ValidationError::NonFiniteCoord { node_id, field } => {
                write!(f, "node {node_id}: non-finite {field}")
            }
            ValidationError::PriorityOutOfRange { node_id, priority } => write!(
                f,
                "node {node_id}: priority {priority} out of range (expected 1-5)"
            ),
            ValidationError::UnknownVersion(v) => write!(
                f,
                "board version {v} is newer than this build (supports {CURRENT_BOARD_VERSION})"
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Board {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Board {
    /// The board's effective schema version. A `None` (absent) `version` key
    /// means the board predates versioning and is treated as the current
    /// version, so old files validate clean and migration logic can branch on a
    /// concrete number.
    pub fn schema_version(&self) -> u32 {
        self.version.unwrap_or(CURRENT_BOARD_VERSION)
    }

    /// Structurally validate the board, returning every problem found (empty Vec
    /// == clean). Pure and side-effect-free so it can run anywhere — the CLI
    /// (`brainstorm validate`), the file-watcher reload path, or tests.
    ///
    /// Detects, in order:
    /// - duplicate node ids
    /// - duplicate edge ids
    /// - dangling edges (an endpoint references a node id that does not exist)
    /// - non-finite node coordinates/dimensions (NaN/inf)
    /// - out-of-range node priority (outside 1..=5)
    /// - an unknown (future) schema version — reported as a non-fatal warning
    ///
    /// Forward-compatibility note: unknown *keys* are silently ignored by serde
    /// at deserialize time (we never reject a board for carrying extra fields).
    /// A future *version number* is reported here as `UnknownVersion` so callers
    /// can warn without refusing to load.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        // Future-version warning (non-fatal). Only a version strictly newer than
        // what we understand is flagged; a missing version is current.
        if self.schema_version() > CURRENT_BOARD_VERSION {
            errors.push(ValidationError::UnknownVersion(self.schema_version()));
        }

        // Duplicate node ids, and collect the set of known ids for the dangling
        // check below.
        let mut seen_node_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut node_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for node in &self.nodes {
            node_ids.insert(node.id.as_str());
            if !seen_node_ids.insert(node.id.as_str()) {
                errors.push(ValidationError::DuplicateNodeId(node.id.clone()));
            }

            // Non-finite coordinates / dimensions.
            for (field, value) in [
                ("x", node.x),
                ("y", node.y),
                ("width", node.width),
                ("height", node.height),
            ] {
                if !value.is_finite() {
                    errors.push(ValidationError::NonFiniteCoord {
                        node_id: node.id.clone(),
                        field: field.to_string(),
                    });
                }
            }

            // Out-of-range priority (documented as 1..=5).
            if let Some(p) = node.priority {
                if !(1..=5).contains(&p) {
                    errors.push(ValidationError::PriorityOutOfRange {
                        node_id: node.id.clone(),
                        priority: p,
                    });
                }
            }
        }

        // Duplicate edge ids + dangling endpoints.
        let mut seen_edge_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for edge in &self.edges {
            if !seen_edge_ids.insert(edge.id.as_str()) {
                errors.push(ValidationError::DuplicateEdgeId(edge.id.clone()));
            }
            for endpoint in [&edge.from_node, &edge.to_node] {
                if !node_ids.contains(endpoint.as_str()) {
                    errors.push(ValidationError::DanglingEdge {
                        edge_id: edge.id.clone(),
                        missing_node: endpoint.clone(),
                    });
                }
            }
        }

        errors
    }

    /// Drop every edge whose `from_node` or `to_node` references a non-existent
    /// node, returning the ids of the edges removed. Used on the non-destructive
    /// reload path: rather than silently rendering nothing for a dangling edge
    /// (which looks like a bug), the UI removes it from the in-memory board and
    /// warns. The on-disk file is untouched until the next save.
    pub fn drop_dangling_edges(&mut self) -> Vec<String> {
        let node_ids: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.id.as_str()).collect();
        let mut dropped = Vec::new();
        self.edges.retain(|edge| {
            let ok = node_ids.contains(edge.from_node.as_str())
                && node_ids.contains(edge.to_node.as_str());
            if !ok {
                dropped.push(edge.id.clone());
            }
            ok
        });
        dropped
    }
    /// Fill in zero `width`/`height` on freshly-loaded nodes using text-based
    /// auto-sizing. Agents (and hand-edited `board.json` files) may omit the
    /// dimensions entirely; `#[serde(default)]` deserializes those to `0.0`,
    /// and this pass replaces only the missing axis so any explicit dimension
    /// is preserved. Idempotent: once both axes are non-zero it's a no-op.
    pub fn apply_auto_size(&mut self) {
        for node in &mut self.nodes {
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    pub text: String,
    #[serde(default)]
    pub node_type: NodeType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
}

impl Node {
    pub fn new(id: String, x: f64, y: f64, text: String) -> Self {
        Self {
            id,
            x,
            y,
            width: 200.0,
            height: 100.0,
            text,
            node_type: NodeType::Text,
            color: None,
            tags: Vec::new(),
            status: None,
            group: None,
            priority: None,
        }
    }

    pub fn contains_point(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }

    /// World-space center of the node's bounding rectangle. Used by navigation
    /// (recenter-on-search-match) and any code that needs to aim the camera at a
    /// node rather than its top-left corner.
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn auto_size(text: &str) -> (f64, f64) {
        let char_count = text.len().min(30);
        let width = ((char_count * 9 + 40) as f64).clamp(150.0, 400.0);

        let newline_count = text.chars().filter(|c| *c == '\n').count();
        let wrap_lines = if char_count > 0 {
            (text.len() as f64 / 30.0).ceil() as usize
        } else {
            1
        };
        let lines = newline_count.max(wrap_lines).max(1);
        let height = ((lines * 18 + 50) as f64).clamp(60.0, 500.0);

        (width, height)
    }

    pub fn resize_handle_at(&self, px: f64, py: f64, handle_size: f64) -> Option<ResizeHandle> {
        let half = handle_size / 2.0;

        // Top-left corner
        let tl_x = self.x;
        let tl_y = self.y;
        if px >= tl_x - half && px <= tl_x + half && py >= tl_y - half && py <= tl_y + half {
            return Some(ResizeHandle::TopLeft);
        }

        // Top-right corner
        let tr_x = self.x + self.width;
        let tr_y = self.y;
        if px >= tr_x - half && px <= tr_x + half && py >= tr_y - half && py <= tr_y + half {
            return Some(ResizeHandle::TopRight);
        }

        // Bottom-left corner
        let bl_x = self.x;
        let bl_y = self.y + self.height;
        if px >= bl_x - half && px <= bl_x + half && py >= bl_y - half && py <= bl_y + half {
            return Some(ResizeHandle::BottomLeft);
        }

        // Bottom-right corner
        let br_x = self.x + self.width;
        let br_y = self.y + self.height;
        if px >= br_x - half && px <= br_x + half && py >= br_y - half && py <= br_y + half {
            return Some(ResizeHandle::BottomRight);
        }

        None
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Edge {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Camera {
    pub fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }

    pub fn screen_to_world(&self, screen_x: f64, screen_y: f64) -> (f64, f64) {
        // Guard against a non-finite or non-positive zoom (e.g. a hand-edited
        // board.json, a derived `Default`, or accumulated float error) which would
        // otherwise divide by zero and produce NaN/inf world coordinates.
        let zoom = if self.zoom.is_finite() && self.zoom > 0.0 {
            self.zoom
        } else {
            1.0
        };
        let world_x = (screen_x / zoom) + self.x;
        let world_y = (screen_y / zoom) + self.y;
        (world_x, world_y)
    }

    pub fn world_to_screen(&self, world_x: f64, world_y: f64) -> (f64, f64) {
        let screen_x = (world_x - self.x) * self.zoom;
        let screen_y = (world_y - self.y) * self.zoom;
        (screen_x, screen_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod camera_tests {
        use super::*;

        #[test]
        fn new_has_default_values() {
            let cam = Camera::new();
            assert_eq!(cam.x, 0.0);
            assert_eq!(cam.y, 0.0);
            assert_eq!(cam.zoom, 1.0);
        }

        #[test]
        fn default_matches_new() {
            let cam = Camera::default();
            assert_eq!(cam.x, 0.0);
            assert_eq!(cam.y, 0.0);
            assert_eq!(cam.zoom, 1.0);
        }

        #[test]
        fn screen_to_world_guards_zero_zoom() {
            // A zoom of 0 would divide by zero; the guard falls back to 1.0.
            let cam = Camera { x: 10.0, y: 20.0, zoom: 0.0 };
            let (wx, wy) = cam.screen_to_world(100.0, 200.0);
            assert!(wx.is_finite() && wy.is_finite());
            assert_eq!(wx, 110.0);
            assert_eq!(wy, 220.0);
        }

        #[test]
        fn screen_to_world_guards_nan_and_negative_zoom() {
            for bad in [f64::NAN, f64::INFINITY, -2.0] {
                let cam = Camera { x: 0.0, y: 0.0, zoom: bad };
                let (wx, wy) = cam.screen_to_world(50.0, 50.0);
                assert!(wx.is_finite() && wy.is_finite(), "zoom {bad} produced non-finite");
                assert_eq!(wx, 50.0);
                assert_eq!(wy, 50.0);
            }
        }

        #[test]
        fn screen_to_world_identity_at_origin() {
            let cam = Camera::new();
            let (wx, wy) = cam.screen_to_world(100.0, 200.0);
            assert_eq!(wx, 100.0);
            assert_eq!(wy, 200.0);
        }

        #[test]
        fn world_to_screen_identity_at_origin() {
            let cam = Camera::new();
            let (sx, sy) = cam.world_to_screen(100.0, 200.0);
            assert_eq!(sx, 100.0);
            assert_eq!(sy, 200.0);
        }

        #[test]
        fn screen_to_world_with_pan() {
            let cam = Camera { x: 50.0, y: 100.0, zoom: 1.0 };
            let (wx, wy) = cam.screen_to_world(100.0, 200.0);
            assert_eq!(wx, 150.0);
            assert_eq!(wy, 300.0);
        }

        #[test]
        fn world_to_screen_with_pan() {
            let cam = Camera { x: 50.0, y: 100.0, zoom: 1.0 };
            let (sx, sy) = cam.world_to_screen(150.0, 300.0);
            assert_eq!(sx, 100.0);
            assert_eq!(sy, 200.0);
        }

        #[test]
        fn screen_to_world_with_zoom() {
            let cam = Camera { x: 0.0, y: 0.0, zoom: 2.0 };
            let (wx, wy) = cam.screen_to_world(200.0, 400.0);
            assert_eq!(wx, 100.0);
            assert_eq!(wy, 200.0);
        }

        #[test]
        fn world_to_screen_with_zoom() {
            let cam = Camera { x: 0.0, y: 0.0, zoom: 2.0 };
            let (sx, sy) = cam.world_to_screen(100.0, 200.0);
            assert_eq!(sx, 200.0);
            assert_eq!(sy, 400.0);
        }

        #[test]
        fn round_trip_screen_world_screen() {
            let cam = Camera { x: 123.0, y: 456.0, zoom: 1.5 };
            let (wx, wy) = cam.screen_to_world(300.0, 400.0);
            let (sx, sy) = cam.world_to_screen(wx, wy);
            assert!((sx - 300.0).abs() < 1e-10);
            assert!((sy - 400.0).abs() < 1e-10);
        }

        #[test]
        fn round_trip_world_screen_world() {
            let cam = Camera { x: 123.0, y: 456.0, zoom: 1.5 };
            let (sx, sy) = cam.world_to_screen(500.0, 600.0);
            let (wx, wy) = cam.screen_to_world(sx, sy);
            assert!((wx - 500.0).abs() < 1e-10);
            assert!((wy - 600.0).abs() < 1e-10);
        }
    }

    mod node_tests {
        use super::*;

        #[test]
        fn new_has_default_dimensions() {
            let node = Node::new("test-id".to_string(), 10.0, 20.0, "Hello".to_string());
            assert_eq!(node.id, "test-id");
            assert_eq!(node.x, 10.0);
            assert_eq!(node.y, 20.0);
            assert_eq!(node.width, 200.0);
            assert_eq!(node.height, 100.0);
            assert_eq!(node.text, "Hello");
            assert_eq!(node.node_type, NodeType::Text);
            assert_eq!(node.color, None);
            assert!(node.tags.is_empty());
            assert_eq!(node.status, None);
            assert_eq!(node.group, None);
            assert_eq!(node.priority, None);
        }

        #[test]
        fn center_is_rect_midpoint() {
            let node = Node::new("n".to_string(), 100.0, 200.0, "".to_string());
            // Default 200x100 -> center at (100+100, 200+50)
            assert_eq!(node.center(), (200.0, 250.0));
        }

        #[test]
        fn center_handles_negative_coordinates() {
            let node = Node {
                id: "n".to_string(),
                x: -300.0,
                y: -100.0,
                width: 80.0,
                height: 40.0,
                text: "".to_string(),
                node_type: NodeType::Text,
                color: None,
                tags: Vec::new(),
                status: None,
                group: None,
                priority: None,
            };
            assert_eq!(node.center(), (-260.0, -80.0));
        }

        #[test]
        fn contains_point_inside() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            assert!(node.contains_point(150.0, 150.0));
            assert!(node.contains_point(200.0, 150.0));
        }

        #[test]
        fn contains_point_on_boundary() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            // Top-left corner
            assert!(node.contains_point(100.0, 100.0));
            // Top-right corner
            assert!(node.contains_point(300.0, 100.0));
            // Bottom-left corner
            assert!(node.contains_point(100.0, 200.0));
            // Bottom-right corner
            assert!(node.contains_point(300.0, 200.0));
        }

        #[test]
        fn contains_point_outside() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            // Left of node
            assert!(!node.contains_point(99.0, 150.0));
            // Right of node
            assert!(!node.contains_point(301.0, 150.0));
            // Above node
            assert!(!node.contains_point(150.0, 99.0));
            // Below node
            assert!(!node.contains_point(150.0, 201.0));
        }

        #[test]
        fn resize_handle_at_top_left() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            let handle_size = 8.0;
            // Exactly at corner
            assert_eq!(node.resize_handle_at(100.0, 100.0, handle_size), Some(ResizeHandle::TopLeft));
            // Within handle range
            assert_eq!(node.resize_handle_at(98.0, 98.0, handle_size), Some(ResizeHandle::TopLeft));
            assert_eq!(node.resize_handle_at(103.0, 103.0, handle_size), Some(ResizeHandle::TopLeft));
            // Outside handle range
            assert_eq!(node.resize_handle_at(110.0, 100.0, handle_size), None);
        }

        #[test]
        fn resize_handle_at_top_right() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            let handle_size = 8.0;
            // Exactly at corner (100 + 200 = 300)
            assert_eq!(node.resize_handle_at(300.0, 100.0, handle_size), Some(ResizeHandle::TopRight));
            // Within handle range
            assert_eq!(node.resize_handle_at(298.0, 98.0, handle_size), Some(ResizeHandle::TopRight));
        }

        #[test]
        fn resize_handle_at_bottom_left() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            let handle_size = 8.0;
            // Exactly at corner (100 + 100 = 200)
            assert_eq!(node.resize_handle_at(100.0, 200.0, handle_size), Some(ResizeHandle::BottomLeft));
            // Within handle range
            assert_eq!(node.resize_handle_at(102.0, 202.0, handle_size), Some(ResizeHandle::BottomLeft));
        }

        #[test]
        fn resize_handle_at_bottom_right() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            let handle_size = 8.0;
            // Exactly at corner
            assert_eq!(node.resize_handle_at(300.0, 200.0, handle_size), Some(ResizeHandle::BottomRight));
            // Within handle range
            assert_eq!(node.resize_handle_at(301.0, 201.0, handle_size), Some(ResizeHandle::BottomRight));
        }

        #[test]
        fn resize_handle_at_center_returns_none() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());
            let handle_size = 8.0;
            // Center of node
            assert_eq!(node.resize_handle_at(200.0, 150.0, handle_size), None);
            // Edge but not corner
            assert_eq!(node.resize_handle_at(200.0, 100.0, handle_size), None);
        }

        #[test]
        fn auto_size_short_text() {
            let (w, h) = Node::auto_size("Hello");
            assert!(w >= 150.0);
            assert!(h >= 60.0);
        }

        #[test]
        fn auto_size_long_text() {
            let text = "This is a long piece of text that should make the node wider than the minimum";
            let (w, h) = Node::auto_size(text);
            assert!(w >= 150.0 && w <= 400.0);
            assert!(h >= 60.0);
        }

        #[test]
        fn auto_size_multiline_text() {
            let text = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
            let (_w, h) = Node::auto_size(text);
            assert!(h > 60.0);
        }

        #[test]
        fn auto_size_empty_text() {
            let (w, h) = Node::auto_size("");
            assert_eq!(w, 150.0); // min clamp
            assert_eq!(h, 68.0); // 1 line * 18 + 50 = 68
        }
    }

    mod board_tests {
        use super::*;

        #[test]
        fn default_board_is_empty() {
            let board = Board::default();
            assert!(board.nodes.is_empty());
            assert!(board.edges.is_empty());
        }

        #[test]
        fn serde_round_trip() {
            let board = Board {
                version: None,
                nodes: vec![
                    Node::new("n1".to_string(), 0.0, 0.0, "First".to_string()),
                    Node {
                        id: "n2".to_string(),
                        x: 250.0,
                        y: 0.0,
                        width: 200.0,
                        height: 100.0,
                        text: "Second".to_string(),
                        node_type: NodeType::Idea,
                        color: None,
                        tags: Vec::new(),
                        status: None,
                        group: None,
                        priority: None,
                    },
                ],
                edges: vec![Edge {
                    id: "e1".to_string(),
                    from_node: "n1".to_string(),
                    to_node: "n2".to_string(),
                    label: None,
                }],
            };

            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();

            assert_eq!(board, deserialized);
        }

        #[test]
        fn deserialize_with_missing_node_type_uses_default() {
            let json = r#"{
                "nodes": [{
                    "id": "n1",
                    "x": 0,
                    "y": 0,
                    "width": 200,
                    "height": 100,
                    "text": "No type"
                }],
                "edges": []
            }"#;

            let board: Board = serde_json::from_str(json).unwrap();
            assert_eq!(board.nodes[0].node_type, NodeType::Text);
        }

        #[test]
        fn deserialize_node_without_width_height() {
            let json = r#"{
                "nodes": [{
                    "id": "n1",
                    "x": 100, "y": 200,
                    "text": "Agent-created node", "node_type": "idea"
                }],
                "edges": []
            }"#;
            let board: Board = serde_json::from_str(json).unwrap();
            assert_eq!(board.nodes[0].width, 0.0);
            assert_eq!(board.nodes[0].height, 0.0);
        }

        #[test]
        fn apply_auto_size_fills_zero_dimensions() {
            let json = r#"{
                "nodes": [{
                    "id": "n1",
                    "x": 100, "y": 200,
                    "text": "Agent-created node", "node_type": "idea"
                }],
                "edges": []
            }"#;
            let mut board: Board = serde_json::from_str(json).unwrap();
            assert_eq!(board.nodes[0].width, 0.0);
            assert_eq!(board.nodes[0].height, 0.0);

            board.apply_auto_size();

            let (expected_w, expected_h) = Node::auto_size("Agent-created node");
            assert_eq!(board.nodes[0].width, expected_w);
            assert_eq!(board.nodes[0].height, expected_h);
        }

        #[test]
        fn apply_auto_size_preserves_explicit_dimensions() {
            let mut board = Board {
                version: None,
                nodes: vec![Node {
                    id: "n1".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 321.0,
                    height: 123.0,
                    text: "Sized".to_string(),
                    node_type: NodeType::Text,
                    color: None,
                    tags: Vec::new(),
                    status: None,
                    group: None,
                    priority: None,
                }],
                edges: vec![],
            };

            board.apply_auto_size();

            // Both axes were non-zero, so the pass is a no-op.
            assert_eq!(board.nodes[0].width, 321.0);
            assert_eq!(board.nodes[0].height, 123.0);
        }

        #[test]
        fn apply_auto_size_fills_only_missing_axis() {
            // Width supplied, height omitted (0.0): only height is computed.
            let mut board = Board {
                version: None,
                nodes: vec![Node {
                    id: "n1".to_string(),
                    x: 0.0,
                    y: 0.0,
                    width: 275.0,
                    height: 0.0,
                    text: "Half".to_string(),
                    node_type: NodeType::Text,
                    color: None,
                    tags: Vec::new(),
                    status: None,
                    group: None,
                    priority: None,
                }],
                edges: vec![],
            };

            board.apply_auto_size();

            let (_w, expected_h) = Node::auto_size("Half");
            assert_eq!(board.nodes[0].width, 275.0, "explicit width must be kept");
            assert_eq!(board.nodes[0].height, expected_h, "missing height auto-sized");
        }

        #[test]
        fn deserialize_old_json_without_metadata_fields() {
            let json = r#"{
                "nodes": [{
                    "id": "n1",
                    "x": 0, "y": 0, "width": 200, "height": 100,
                    "text": "Old node", "node_type": "idea"
                }],
                "edges": []
            }"#;
            let board: Board = serde_json::from_str(json).unwrap();
            let node = &board.nodes[0];
            assert_eq!(node.color, None);
            assert!(node.tags.is_empty());
            assert_eq!(node.status, None);
            assert_eq!(node.group, None);
            assert_eq!(node.priority, None);
        }

        #[test]
        fn serde_round_trip_with_metadata() {
            let node = Node {
                id: "m1".to_string(),
                x: 0.0, y: 0.0, width: 200.0, height: 100.0,
                text: "Meta node".to_string(),
                node_type: NodeType::Idea,
                color: Some("#ff6600".to_string()),
                tags: vec!["urgent".to_string(), "pricing".to_string()],
                status: Some("in-progress".to_string()),
                group: Some("cluster-a".to_string()),
                priority: Some(2),
            };
            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(node, deserialized);
        }

        #[test]
        fn skip_serializing_empty_metadata() {
            let node = Node::new("n1".to_string(), 0.0, 0.0, "Plain".to_string());
            let json = serde_json::to_string(&node).unwrap();
            assert!(!json.contains("color"));
            assert!(!json.contains("tags"));
            assert!(!json.contains("status"));
            assert!(!json.contains("group"));
            assert!(!json.contains("priority"));
        }

        #[test]
        fn serialize_includes_set_metadata() {
            let mut node = Node::new("n1".to_string(), 0.0, 0.0, "Tagged".to_string());
            node.color = Some("#00ff00".to_string());
            node.tags = vec!["test".to_string()];
            node.priority = Some(3);
            let json = serde_json::to_string(&node).unwrap();
            assert!(json.contains("\"color\":\"#00ff00\""));
            assert!(json.contains("\"tags\":[\"test\"]"));
            assert!(json.contains("\"priority\":3"));
            assert!(!json.contains("\"status\""));
            assert!(!json.contains("\"group\""));
        }

        #[test]
        fn serialize_produces_valid_json() {
            let board = Board {
                version: None,
                nodes: vec![Node {
                    id: "test".to_string(),
                    x: 100.0,
                    y: 200.0,
                    width: 200.0,
                    height: 100.0,
                    text: "Hello \"world\"".to_string(),
                    node_type: NodeType::Text,
                    color: None,
                    tags: vec![],
                    status: None,
                    group: None,
                    priority: None,
                }],
                edges: vec![],
            };

            let json = serde_json::to_string_pretty(&board).unwrap();
            assert!(json.contains("\"id\": \"test\""));
            assert!(json.contains("\"x\": 100.0"));
            assert!(json.contains("Hello \\\"world\\\""));
        }
    }

    mod validation_tests {
        use super::*;

        fn node(id: &str) -> Node {
            Node::new(id.to_string(), 0.0, 0.0, "n".to_string())
        }

        fn edge(id: &str, from: &str, to: &str) -> Edge {
            Edge {
                id: id.to_string(),
                from_node: from.to_string(),
                to_node: to.to_string(),
                label: None,
            }
        }

        #[test]
        fn clean_board_returns_no_errors() {
            let board = Board {
                version: None,
                nodes: vec![node("a"), node("b")],
                edges: vec![edge("e1", "a", "b")],
            };
            assert!(board.validate().is_empty(), "{:?}", board.validate());
        }

        #[test]
        fn empty_board_is_clean() {
            assert!(Board::default().validate().is_empty());
        }

        #[test]
        fn detects_duplicate_node_id() {
            let board = Board {
                version: None,
                nodes: vec![node("dup"), node("dup")],
                edges: vec![],
            };
            let errs = board.validate();
            assert!(errs.contains(&ValidationError::DuplicateNodeId("dup".to_string())));
        }

        #[test]
        fn detects_duplicate_edge_id() {
            let board = Board {
                version: None,
                nodes: vec![node("a"), node("b")],
                edges: vec![edge("e", "a", "b"), edge("e", "b", "a")],
            };
            let errs = board.validate();
            assert!(errs.contains(&ValidationError::DuplicateEdgeId("e".to_string())));
        }

        #[test]
        fn detects_dangling_from_node() {
            let board = Board {
                version: None,
                nodes: vec![node("a")],
                edges: vec![edge("e1", "ghost", "a")],
            };
            let errs = board.validate();
            assert!(errs.contains(&ValidationError::DanglingEdge {
                edge_id: "e1".to_string(),
                missing_node: "ghost".to_string(),
            }));
        }

        #[test]
        fn detects_dangling_to_node() {
            let board = Board {
                version: None,
                nodes: vec![node("a")],
                edges: vec![edge("e1", "a", "ghost")],
            };
            let errs = board.validate();
            assert!(errs.contains(&ValidationError::DanglingEdge {
                edge_id: "e1".to_string(),
                missing_node: "ghost".to_string(),
            }));
        }

        #[test]
        fn detects_non_finite_coords() {
            let mut n = node("a");
            n.x = f64::NAN;
            n.height = f64::INFINITY;
            let board = Board {
                version: None,
                nodes: vec![n],
                edges: vec![],
            };
            let errs = board.validate();
            assert!(errs.contains(&ValidationError::NonFiniteCoord {
                node_id: "a".to_string(),
                field: "x".to_string(),
            }));
            assert!(errs.contains(&ValidationError::NonFiniteCoord {
                node_id: "a".to_string(),
                field: "height".to_string(),
            }));
        }

        #[test]
        fn detects_priority_out_of_range() {
            let mut low = node("a");
            low.priority = Some(0);
            let mut high = node("b");
            high.priority = Some(9);
            let board = Board {
                version: None,
                nodes: vec![low, high],
                edges: vec![],
            };
            let errs = board.validate();
            assert!(errs.contains(&ValidationError::PriorityOutOfRange {
                node_id: "a".to_string(),
                priority: 0,
            }));
            assert!(errs.contains(&ValidationError::PriorityOutOfRange {
                node_id: "b".to_string(),
                priority: 9,
            }));
        }

        #[test]
        fn valid_priorities_pass() {
            for p in 1..=5u8 {
                let mut n = node("a");
                n.priority = Some(p);
                let board = Board {
                    version: None,
                    nodes: vec![n],
                    edges: vec![],
                };
                assert!(board.validate().is_empty(), "priority {p} should be valid");
            }
        }

        #[test]
        fn future_version_warns_but_is_not_fatal_for_other_checks() {
            let board = Board {
                version: Some(CURRENT_BOARD_VERSION + 1),
                nodes: vec![node("a"), node("b")],
                edges: vec![edge("e1", "a", "b")],
            };
            let errs = board.validate();
            assert_eq!(
                errs,
                vec![ValidationError::UnknownVersion(CURRENT_BOARD_VERSION + 1)]
            );
        }

        #[test]
        fn current_and_absent_version_do_not_warn() {
            let with = Board {
                version: Some(CURRENT_BOARD_VERSION),
                nodes: vec![],
                edges: vec![],
            };
            assert!(with.validate().is_empty());
            assert!(Board::default().validate().is_empty());
        }

        #[test]
        fn schema_version_defaults_to_current_when_absent() {
            assert_eq!(Board::default().schema_version(), CURRENT_BOARD_VERSION);
            let b = Board {
                version: Some(7),
                nodes: vec![],
                edges: vec![],
            };
            assert_eq!(b.schema_version(), 7);
        }

        #[test]
        fn drop_dangling_edges_removes_only_dangling() {
            let mut board = Board {
                version: None,
                nodes: vec![node("a"), node("b")],
                edges: vec![
                    edge("good", "a", "b"),
                    edge("bad1", "a", "ghost"),
                    edge("bad2", "ghost", "b"),
                ],
            };
            let dropped = board.drop_dangling_edges();
            assert_eq!(dropped, vec!["bad1".to_string(), "bad2".to_string()]);
            assert_eq!(board.edges.len(), 1);
            assert_eq!(board.edges[0].id, "good");
            // After dropping, the board validates clean.
            assert!(board.validate().is_empty());
        }

        #[test]
        fn drop_dangling_edges_noop_on_clean_board() {
            let mut board = Board {
                version: None,
                nodes: vec![node("a"), node("b")],
                edges: vec![edge("e1", "a", "b")],
            };
            assert!(board.drop_dangling_edges().is_empty());
            assert_eq!(board.edges.len(), 1);
        }

        #[test]
        fn version_absent_is_not_serialized() {
            // A board with no explicit version must not gain a `version` key on
            // serialize — old files round-trip byte-stably.
            let board = Board::default();
            let json = serde_json::to_string(&board).unwrap();
            assert!(!json.contains("version"));
        }

        #[test]
        fn explicit_version_round_trips() {
            let board = Board {
                version: Some(1),
                nodes: vec![],
                edges: vec![],
            };
            let json = serde_json::to_string(&board).unwrap();
            assert!(json.contains("\"version\":1"));
            let parsed: Board = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.version, Some(1));
        }

        #[test]
        fn deserialize_old_board_without_version() {
            let json = r#"{"nodes":[],"edges":[]}"#;
            let board: Board = serde_json::from_str(json).unwrap();
            assert_eq!(board.version, None);
            assert_eq!(board.schema_version(), CURRENT_BOARD_VERSION);
        }

        #[test]
        fn validation_error_display_messages() {
            assert_eq!(
                ValidationError::DuplicateNodeId("x".to_string()).to_string(),
                "duplicate node id: x"
            );
            assert_eq!(
                ValidationError::DanglingEdge {
                    edge_id: "e1".to_string(),
                    missing_node: "ghost".to_string(),
                }
                .to_string(),
                "dangling edge e1: references missing node ghost"
            );
        }
    }

    mod edge_tests {
        use super::*;

        #[test]
        fn serde_round_trip() {
            let edge = Edge {
                id: "e1".to_string(),
                from_node: "a".to_string(),
                to_node: "b".to_string(),
                label: None,
            };

            let json = serde_json::to_string(&edge).unwrap();
            let deserialized: Edge = serde_json::from_str(&json).unwrap();

            assert_eq!(edge, deserialized);
        }

        #[test]
        fn serde_round_trip_with_label() {
            let edge = Edge {
                id: "e1".to_string(),
                from_node: "a".to_string(),
                to_node: "b".to_string(),
                label: Some("depends on".to_string()),
            };
            let json = serde_json::to_string(&edge).unwrap();
            assert!(json.contains("\"label\":\"depends on\""));
            let deserialized: Edge = serde_json::from_str(&json).unwrap();
            assert_eq!(edge, deserialized);
        }

        #[test]
        fn skip_serializing_none_label() {
            let edge = Edge {
                id: "e1".to_string(),
                from_node: "a".to_string(),
                to_node: "b".to_string(),
                label: None,
            };
            let json = serde_json::to_string(&edge).unwrap();
            assert!(!json.contains("label"));
        }

        #[test]
        fn deserialize_old_edge_without_label() {
            let json = r#"{"id":"e1","from_node":"a","to_node":"b"}"#;
            let edge: Edge = serde_json::from_str(json).unwrap();
            assert_eq!(edge.label, None);
        }
    }

    mod link_preview_tests {
        use super::*;

        #[test]
        fn default_has_empty_url() {
            let preview = LinkPreview::default();
            assert_eq!(preview.url, "");
            assert!(preview.title.is_none());
            assert!(preview.description.is_none());
            assert!(preview.image.is_none());
            assert!(preview.site_name.is_none());
        }

        #[test]
        fn serde_round_trip_with_all_fields() {
            let preview = LinkPreview {
                url: "https://example.com".to_string(),
                title: Some("Example".to_string()),
                description: Some("A test".to_string()),
                image: Some("https://example.com/img.png".to_string()),
                site_name: Some("Example Site".to_string()),
            };

            let json = serde_json::to_string(&preview).unwrap();
            let deserialized: LinkPreview = serde_json::from_str(&json).unwrap();

            assert_eq!(preview, deserialized);
        }
    }

    mod edge_cases {
        use super::*;

        #[test]
        fn camera_with_very_small_zoom() {
            let cam = Camera { x: 0.0, y: 0.0, zoom: 0.1 };
            let (wx, wy) = cam.screen_to_world(100.0, 100.0);
            assert_eq!(wx, 1000.0);
            assert_eq!(wy, 1000.0);

            let (sx, sy) = cam.world_to_screen(1000.0, 1000.0);
            assert_eq!(sx, 100.0);
            assert_eq!(sy, 100.0);
        }

        #[test]
        fn camera_with_large_zoom() {
            let cam = Camera { x: 0.0, y: 0.0, zoom: 5.0 };
            let (wx, wy) = cam.screen_to_world(500.0, 500.0);
            assert_eq!(wx, 100.0);
            assert_eq!(wy, 100.0);
        }

        #[test]
        fn camera_with_negative_position() {
            let cam = Camera { x: -100.0, y: -200.0, zoom: 1.0 };
            let (wx, wy) = cam.screen_to_world(0.0, 0.0);
            assert_eq!(wx, -100.0);
            assert_eq!(wy, -200.0);
        }

        #[test]
        fn node_at_negative_coordinates() {
            let node = Node::new("n".to_string(), -500.0, -300.0, "".to_string());
            assert!(node.contains_point(-400.0, -250.0));
            assert!(!node.contains_point(-501.0, -250.0));
        }

        #[test]
        fn node_with_custom_dimensions() {
            let node = Node {
                id: "n".to_string(),
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 25.0,
                text: "tiny".to_string(),
                node_type: NodeType::Text,
                color: None,
                tags: Vec::new(),
                status: None,
                group: None,
                priority: None,
            };
            assert!(node.contains_point(25.0, 12.0));
            assert!(node.contains_point(50.0, 25.0));
            assert!(!node.contains_point(51.0, 12.0));
        }

        #[test]
        fn node_with_empty_text() {
            let node = Node::new("n".to_string(), 0.0, 0.0, "".to_string());
            assert_eq!(node.text, "");

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.text, "");
        }

        #[test]
        fn node_with_multiline_text() {
            let text = "Line 1\nLine 2\nLine 3";
            let node = Node::new("n".to_string(), 0.0, 0.0, text.to_string());

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.text, text);
        }

        #[test]
        fn node_with_unicode_text() {
            let text = "Hello 世界 🌍 émojis";
            let node = Node::new("n".to_string(), 0.0, 0.0, text.to_string());

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.text, text);
        }

        #[test]
        fn node_with_special_characters_in_text() {
            let text = r#"Quotes: "test" 'single' and \backslash\ and tabs	here"#;
            let node = Node {
                id: "special".to_string(),
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
                text: text.to_string(),
                node_type: NodeType::Text,
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.text, text);
        }

        #[test]
        fn all_node_types() {
            let types = [
                NodeType::Text,
                NodeType::Idea,
                NodeType::Note,
                NodeType::Image,
                NodeType::Md,
                NodeType::Link,
            ];
            for node_type in types {
                let node = Node {
                    id: format!("node-{}", node_type.as_str()),
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                    text: "content".to_string(),
                    node_type,
                    color: None,
                    tags: vec![],
                    status: None,
                    group: None,
                    priority: None,
                };

                let json = serde_json::to_string(&node).unwrap();
                assert!(json.contains(&format!("\"node_type\":\"{}\"", node_type.as_str())));
                let deserialized: Node = serde_json::from_str(&json).unwrap();
                assert_eq!(deserialized.node_type, node_type);
            }
        }

        #[test]
        fn node_with_float_precision() {
            let node = Node {
                id: "precise".to_string(),
                x: 123.456789,
                y: 987.654321,
                width: 200.123,
                height: 100.789,
                text: "precise".to_string(),
                node_type: NodeType::Text,
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let json = serde_json::to_string(&node).unwrap();
            let deserialized: Node = serde_json::from_str(&json).unwrap();
            assert!((deserialized.x - 123.456789).abs() < 1e-6);
            assert!((deserialized.y - 987.654321).abs() < 1e-6);
        }
    }

    mod stress_tests {
        use super::*;

        #[test]
        fn board_with_many_nodes() {
            let nodes: Vec<Node> = (0..1000)
                .map(|i| {
                    let row = i / 10;
                    let col = i % 10;
                    Node::new(
                        format!("node-{}", i),
                        col as f64 * 250.0,
                        row as f64 * 150.0,
                        format!("Node content {}", i),
                    )
                })
                .collect();

            let board = Board {
                version: None,
                nodes,
                edges: vec![],
            };

            assert_eq!(board.nodes.len(), 1000);

            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.nodes.len(), 1000);
            assert_eq!(deserialized.nodes[999].id, "node-999");
        }

        #[test]
        fn board_with_many_edges() {
            let nodes: Vec<Node> = (0..100)
                .map(|i| Node::new(format!("n{}", i), i as f64 * 250.0, 0.0, format!("Node {}", i)))
                .collect();

            let edges: Vec<Edge> = (0..99)
                .map(|i| Edge {
                    id: format!("e{}", i),
                    from_node: format!("n{}", i),
                    to_node: format!("n{}", i + 1),
                    label: None,
                })
                .collect();

            let board = Board { version: None, nodes, edges };

            assert_eq!(board.edges.len(), 99);

            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.edges.len(), 99);
        }

        #[test]
        fn board_with_fully_connected_nodes() {
            let n = 20;
            let nodes: Vec<Node> = (0..n)
                .map(|i| Node::new(format!("n{}", i), i as f64 * 250.0, 0.0, format!("Node {}", i)))
                .collect();

            let mut edges = Vec::new();
            let mut edge_id = 0;
            for i in 0..n {
                for j in (i + 1)..n {
                    edges.push(Edge {
                        id: format!("e{}", edge_id),
                        from_node: format!("n{}", i),
                        to_node: format!("n{}", j),
                        label: None,
                    });
                    edge_id += 1;
                }
            }

            let board = Board { version: None, nodes, edges };

            let expected_edges = n * (n - 1) / 2;
            assert_eq!(board.edges.len(), expected_edges);

            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.edges.len(), expected_edges);
        }

        #[test]
        fn node_with_large_text() {
            let large_text: String = (0..10000).map(|_| 'x').collect();
            let node = Node {
                id: "large".to_string(),
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
                text: large_text.clone(),
                node_type: NodeType::Text,
                color: None,
                tags: vec![],
                status: None,
                group: None,
                priority: None,
            };

            let board = Board { version: None, nodes: vec![node], edges: vec![] };
            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();

            assert_eq!(deserialized.nodes[0].text.len(), 10000);
        }

        #[test]
        fn node_contains_point_many_checks() {
            let node = Node::new("n".to_string(), 100.0, 100.0, "".to_string());

            for i in 0..1000 {
                let x = 100.0 + (i as f64 % 200.0);
                let y = 100.0 + ((i / 200) as f64 % 100.0);
                let inside = node.contains_point(x, y);
                let expected = x >= 100.0 && x <= 300.0 && y >= 100.0 && y <= 200.0;
                assert_eq!(inside, expected, "Failed at ({}, {})", x, y);
            }
        }
    }

    mod node_type_tests {
        use super::*;

        #[test]
        fn default_is_text() {
            assert_eq!(NodeType::default(), NodeType::Text);
        }

        #[test]
        fn serializes_lowercase() {
            assert_eq!(serde_json::to_string(&NodeType::Idea).unwrap(), "\"idea\"");
            assert_eq!(serde_json::to_string(&NodeType::Md).unwrap(), "\"md\"");
            assert_eq!(serde_json::to_string(&NodeType::Link).unwrap(), "\"link\"");
        }

        #[test]
        fn deserializes_known_variants() {
            for s in ["text", "idea", "note", "image", "md", "link"] {
                let nt: NodeType = serde_json::from_str(&format!("\"{s}\"")).unwrap();
                assert_eq!(nt.as_str(), s);
            }
        }

        #[test]
        fn unknown_variant_is_forward_compatible() {
            // A future/unrecognized node_type must not fail deserialization; it maps
            // to Unknown so the node round-trips instead of being dropped.
            let nt: NodeType = serde_json::from_str("\"hologram\"").unwrap();
            assert_eq!(nt, NodeType::Unknown);

            let json = r#"{
                "nodes": [{
                    "id": "n1", "x": 0, "y": 0, "text": "future",
                    "node_type": "hologram"
                }],
                "edges": []
            }"#;
            let board: Board = serde_json::from_str(json).unwrap();
            assert_eq!(board.nodes[0].node_type, NodeType::Unknown);
        }

        #[test]
        fn cycle_progression() {
            assert_eq!(NodeType::Text.cycle(), NodeType::Idea);
            assert_eq!(NodeType::Idea.cycle(), NodeType::Note);
            assert_eq!(NodeType::Note.cycle(), NodeType::Image);
            assert_eq!(NodeType::Image.cycle(), NodeType::Md);
            assert_eq!(NodeType::Md.cycle(), NodeType::Link);
            assert_eq!(NodeType::Link.cycle(), NodeType::Text);
            assert_eq!(NodeType::Unknown.cycle(), NodeType::Text);
        }

        #[test]
        fn from_str_round_trips_as_str() {
            use std::str::FromStr;
            for nt in [
                NodeType::Text,
                NodeType::Idea,
                NodeType::Note,
                NodeType::Image,
                NodeType::Md,
                NodeType::Link,
            ] {
                assert_eq!(NodeType::from_str(nt.as_str()).unwrap(), nt);
            }
            assert_eq!(NodeType::from_str("nope").unwrap(), NodeType::Unknown);
        }
    }

    mod truncate_tests {
        use super::*;

        #[test]
        fn short_name_unchanged() {
            assert_eq!(truncate_filename("photo.png"), "photo.png");
        }

        #[test]
        fn long_ascii_name_truncated() {
            let name = "a_very_long_filename_indeed.png";
            let out = truncate_filename(name);
            assert_eq!(out, "a_very_long_filen...");
            assert_eq!(out.chars().count(), 20); // 17 + "..."
        }

        #[test]
        fn multibyte_name_does_not_panic() {
            // 16 ascii + emoji at char 17 boundary + tail: a byte slice [..17] would
            // land mid-codepoint and panic. char-safe truncation must not.
            let name = "abcdefghijklmnop😀extra.png";
            let out = truncate_filename(name);
            assert!(out.ends_with("..."));
            // First 17 chars then ellipsis: the emoji is char index 16 (0-based).
            assert_eq!(out, "abcdefghijklmnop😀...");
        }

        #[test]
        fn all_multibyte_name_does_not_panic() {
            let name = "世界世界世界世界世界世界世界世界世界世界世界.png";
            let out = truncate_filename(name);
            assert!(out.ends_with("..."));
            assert_eq!(out.chars().count(), 20);
        }
    }
}
