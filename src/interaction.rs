//! DOM-free interaction/reducer layer.
//!
//! All board mutations are expressed as a [`BoardAction`] and applied by the pure
//! [`reduce`] function, which returns the next [`Board`] plus a list of
//! [`SideEffect`]s the caller must perform (asset deletion, persistence). Keeping
//! the mutation logic free of `web_sys`/Leptos lets it be unit-tested under native
//! `cargo test` (no WASM, no DOM), and gives undo/redo a single, well-defined place
//! where a history snapshot is taken.
//!
//! The view layer (`app.rs`, components) builds an action from DOM input, calls a
//! thin `apply` wrapper that snapshots history once and runs `reduce`, then sets the
//! board signal and dispatches the returned side effects.

use crate::state::{Board, Edge, Node, NodeType};
use std::str::FromStr;

/// How node type cycling progresses when the user presses `T`, expressed over the
/// string form for callers that still work with raw `node_type` strings.
///
/// Delegates to [`NodeType::cycle`] so the progression has a single source of truth;
/// unrecognized inputs cycle to `"text"`.
pub fn cycle_node_type(current: &str) -> String {
    NodeType::from_str(current)
        .unwrap_or(NodeType::Unknown)
        .cycle()
        .as_str()
        .to_string()
}

/// A side effect the caller must perform after a [`reduce`] call.
///
/// The reducer itself is pure and never touches disk or the network; it only
/// *describes* what should happen. `app.rs` interprets these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideEffect {
    /// Delete a local asset file from disk (e.g. a pasted image removed by a node
    /// deletion). Only emitted for paths the reducer judged to be local assets.
    DeleteAsset(String),
    /// Persist the board. Emitted by every mutating action so the caller can route
    /// it through the centralized debounced save sink.
    RequestSave,
}

/// A single, atomic board mutation.
///
/// Each variant carries plain data (ids, coordinates, text) rather than DOM
/// events, so the whole enum is `Send`-able plain data and trivially testable.
#[derive(Debug, Clone, PartialEq)]
pub enum BoardAction {
    /// Move a set of nodes to absolute positions. `(id, x, y)` triples.
    MoveNodes(Vec<(String, f64, f64)>),
    /// Resize one node to an absolute geometry.
    ResizeNode {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// Create a directed edge between two existing nodes.
    CreateEdge {
        id: String,
        from_node: String,
        to_node: String,
    },
    /// Insert a fully-formed node (the caller pre-builds it with a fresh id).
    CreateNode(Node),
    /// Delete the given node ids and any edge touching them. The selected edge id
    /// (if any) is deleted as well. Asset paths flagged here become
    /// [`SideEffect::DeleteAsset`].
    DeleteSelected {
        node_ids: Vec<String>,
        edge_id: Option<String>,
    },
    /// Cycle the `node_type` of the given nodes one step forward.
    CycleType(Vec<String>),
    /// Paste a batch of pre-rewritten nodes and edges (ids already fresh).
    PasteNodes { nodes: Vec<Node>, edges: Vec<Edge> },
    /// Replace a node's text (plain text / markdown inline editor commit).
    EditText { id: String, text: String },
    /// Replace a node's text from the markdown modal editor. Behaviourally identical
    /// to [`BoardAction::EditText`] but kept distinct so undo entries and any future
    /// instrumentation can tell the two editors apart.
    EditMarkdown { id: String, text: String },
}

/// Does this path look like a deletable local asset (a pasted image under
/// `/assets/`)? Mirrors the previous inline check in the keyboard handler.
fn is_local_asset(path: &str) -> bool {
    path.contains("/assets/")
}

/// Apply `action` to `board`, returning the next board and the side effects the
/// caller must perform.
///
/// Pure: no I/O, no DOM, no global state. Given the same inputs it always returns
/// the same outputs, which is what makes the unit tests below meaningful.
pub fn reduce(mut board: Board, action: BoardAction) -> (Board, Vec<SideEffect>) {
    match action {
        BoardAction::MoveNodes(moves) => {
            for (id, x, y) in moves {
                if let Some(node) = board.nodes.iter_mut().find(|n| n.id == id) {
                    node.x = x;
                    node.y = y;
                }
            }
            (board, vec![SideEffect::RequestSave])
        }
        BoardAction::ResizeNode {
            id,
            x,
            y,
            width,
            height,
        } => {
            if let Some(node) = board.nodes.iter_mut().find(|n| n.id == id) {
                node.x = x;
                node.y = y;
                node.width = width;
                node.height = height;
            }
            (board, vec![SideEffect::RequestSave])
        }
        BoardAction::CreateEdge {
            id,
            from_node,
            to_node,
        } => {
            board.edges.push(Edge {
                id,
                from_node,
                to_node,
                label: None,
            });
            (board, vec![SideEffect::RequestSave])
        }
        BoardAction::CreateNode(node) => {
            board.nodes.push(node);
            (board, vec![SideEffect::RequestSave])
        }
        BoardAction::DeleteSelected { node_ids, edge_id } => {
            let mut effects = Vec::new();
            if let Some(edge_id) = edge_id {
                board.edges.retain(|e| e.id != edge_id);
            }
            if !node_ids.is_empty() {
                for node in &board.nodes {
                    if node_ids.contains(&node.id)
                        && node.node_type == NodeType::Image
                        && is_local_asset(&node.text)
                    {
                        effects.push(SideEffect::DeleteAsset(node.text.clone()));
                    }
                }
                board.nodes.retain(|n| !node_ids.contains(&n.id));
                board
                    .edges
                    .retain(|e| !node_ids.contains(&e.from_node) && !node_ids.contains(&e.to_node));
            }
            effects.push(SideEffect::RequestSave);
            (board, effects)
        }
        BoardAction::CycleType(ids) => {
            for node in &mut board.nodes {
                if ids.contains(&node.id) {
                    node.node_type = node.node_type.cycle();
                }
            }
            (board, vec![SideEffect::RequestSave])
        }
        BoardAction::PasteNodes { nodes, edges } => {
            board.nodes.extend(nodes);
            board.edges.extend(edges);
            (board, vec![SideEffect::RequestSave])
        }
        BoardAction::EditText { id, text } | BoardAction::EditMarkdown { id, text } => {
            if let Some(node) = board.nodes.iter_mut().find(|n| n.id == id) {
                node.text = text;
            }
            (board, vec![SideEffect::RequestSave])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, x: f64, y: f64) -> Node {
        Node::new(id.to_string(), x, y, "n".to_string())
    }

    fn board_with(nodes: Vec<Node>, edges: Vec<Edge>) -> Board {
        Board { version: None, nodes, edges }
    }

    #[test]
    fn cycle_node_type_progression() {
        assert_eq!(cycle_node_type("text"), "idea");
        assert_eq!(cycle_node_type("idea"), "note");
        assert_eq!(cycle_node_type("note"), "image");
        assert_eq!(cycle_node_type("image"), "md");
        assert_eq!(cycle_node_type("md"), "link");
        assert_eq!(cycle_node_type("link"), "text");
        assert_eq!(cycle_node_type("anything-else"), "text");
    }

    #[test]
    fn move_nodes_sets_absolute_positions() {
        let board = board_with(vec![node("a", 0.0, 0.0), node("b", 10.0, 10.0)], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::MoveNodes(vec![("a".into(), 50.0, 60.0)]),
        );
        let a = out.nodes.iter().find(|n| n.id == "a").unwrap();
        assert_eq!((a.x, a.y), (50.0, 60.0));
        // Untouched node stays put.
        let b = out.nodes.iter().find(|n| n.id == "b").unwrap();
        assert_eq!((b.x, b.y), (10.0, 10.0));
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn move_nodes_ignores_unknown_id() {
        let board = board_with(vec![node("a", 0.0, 0.0)], vec![]);
        let (out, _) = reduce(
            board,
            BoardAction::MoveNodes(vec![("ghost".into(), 1.0, 1.0)]),
        );
        // Nothing changed, no panic.
        assert_eq!(out.nodes.len(), 1);
        assert_eq!((out.nodes[0].x, out.nodes[0].y), (0.0, 0.0));
    }

    #[test]
    fn resize_node_sets_geometry() {
        let board = board_with(vec![node("a", 0.0, 0.0)], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::ResizeNode {
                id: "a".into(),
                x: 5.0,
                y: 6.0,
                width: 123.0,
                height: 45.0,
            },
        );
        let a = &out.nodes[0];
        assert_eq!((a.x, a.y, a.width, a.height), (5.0, 6.0, 123.0, 45.0));
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn create_edge_appends() {
        let board = board_with(vec![node("a", 0.0, 0.0), node("b", 0.0, 0.0)], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::CreateEdge {
                id: "e1".into(),
                from_node: "a".into(),
                to_node: "b".into(),
            },
        );
        assert_eq!(out.edges.len(), 1);
        assert_eq!(out.edges[0].from_node, "a");
        assert_eq!(out.edges[0].to_node, "b");
        assert_eq!(out.edges[0].label, None);
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn create_node_appends() {
        let board = board_with(vec![], vec![]);
        let (out, fx) = reduce(board, BoardAction::CreateNode(node("new", 1.0, 2.0)));
        assert_eq!(out.nodes.len(), 1);
        assert_eq!(out.nodes[0].id, "new");
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn delete_selected_removes_nodes_and_incident_edges() {
        let board = board_with(
            vec![node("a", 0.0, 0.0), node("b", 0.0, 0.0), node("c", 0.0, 0.0)],
            vec![
                Edge {
                    id: "ab".into(),
                    from_node: "a".into(),
                    to_node: "b".into(),
                    label: None,
                },
                Edge {
                    id: "bc".into(),
                    from_node: "b".into(),
                    to_node: "c".into(),
                    label: None,
                },
            ],
        );
        let (out, fx) = reduce(
            board,
            BoardAction::DeleteSelected {
                node_ids: vec!["a".into()],
                edge_id: None,
            },
        );
        // Node a gone, edge ab (incident to a) gone, edge bc survives.
        assert!(out.nodes.iter().all(|n| n.id != "a"));
        assert_eq!(out.nodes.len(), 2);
        assert!(out.edges.iter().any(|e| e.id == "bc"));
        assert!(out.edges.iter().all(|e| e.id != "ab"));
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn delete_selected_only_edge() {
        let board = board_with(
            vec![node("a", 0.0, 0.0), node("b", 0.0, 0.0)],
            vec![Edge {
                id: "ab".into(),
                from_node: "a".into(),
                to_node: "b".into(),
                label: None,
            }],
        );
        let (out, fx) = reduce(
            board,
            BoardAction::DeleteSelected {
                node_ids: vec![],
                edge_id: Some("ab".into()),
            },
        );
        assert!(out.edges.is_empty());
        assert_eq!(out.nodes.len(), 2);
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn delete_selected_emits_asset_deletion_for_local_images() {
        let mut img = node("img", 0.0, 0.0);
        img.node_type = NodeType::Image;
        img.text = "/Users/me/proj/assets/pic.png".to_string();
        let mut remote = node("remote", 0.0, 0.0);
        remote.node_type = NodeType::Image;
        remote.text = "https://example.com/pic.png".to_string();
        let board = board_with(vec![img, remote], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::DeleteSelected {
                node_ids: vec!["img".into(), "remote".into()],
                edge_id: None,
            },
        );
        assert!(out.nodes.is_empty());
        // Only the local /assets/ image yields a DeleteAsset; remote does not.
        assert_eq!(
            fx,
            vec![
                SideEffect::DeleteAsset("/Users/me/proj/assets/pic.png".to_string()),
                SideEffect::RequestSave,
            ]
        );
    }

    #[test]
    fn delete_selected_non_image_node_emits_no_asset() {
        let mut text_node = node("t", 0.0, 0.0);
        text_node.text = "/assets/not-an-image-path".to_string();
        let board = board_with(vec![text_node], vec![]);
        let (_out, fx) = reduce(
            board,
            BoardAction::DeleteSelected {
                node_ids: vec!["t".into()],
                edge_id: None,
            },
        );
        // node_type is "text" so no DeleteAsset, just save.
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn cycle_type_advances_only_selected() {
        let mut a = node("a", 0.0, 0.0);
        a.node_type = NodeType::Text;
        let mut b = node("b", 0.0, 0.0);
        b.node_type = NodeType::Idea;
        let board = board_with(vec![a, b], vec![]);
        let (out, fx) = reduce(board, BoardAction::CycleType(vec!["a".into()]));
        assert_eq!(out.nodes.iter().find(|n| n.id == "a").unwrap().node_type, NodeType::Idea);
        // b unselected, unchanged.
        assert_eq!(out.nodes.iter().find(|n| n.id == "b").unwrap().node_type, NodeType::Idea);
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn paste_nodes_extends_board() {
        let board = board_with(vec![node("a", 0.0, 0.0)], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::PasteNodes {
                nodes: vec![node("p1", 0.0, 0.0), node("p2", 0.0, 0.0)],
                edges: vec![Edge {
                    id: "pe".into(),
                    from_node: "p1".into(),
                    to_node: "p2".into(),
                    label: None,
                }],
            },
        );
        assert_eq!(out.nodes.len(), 3);
        assert_eq!(out.edges.len(), 1);
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn edit_text_replaces_node_text() {
        let board = board_with(vec![node("a", 0.0, 0.0)], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::EditText {
                id: "a".into(),
                text: "hello world".into(),
            },
        );
        assert_eq!(out.nodes[0].text, "hello world");
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn edit_markdown_replaces_node_text() {
        let board = board_with(vec![node("a", 0.0, 0.0)], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::EditMarkdown {
                id: "a".into(),
                text: "# heading".into(),
            },
        );
        assert_eq!(out.nodes[0].text, "# heading");
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn edit_text_unknown_id_is_noop() {
        let board = board_with(vec![node("a", 0.0, 0.0)], vec![]);
        let (out, fx) = reduce(
            board,
            BoardAction::EditText {
                id: "ghost".into(),
                text: "x".into(),
            },
        );
        assert_eq!(out.nodes[0].text, "n");
        assert_eq!(fx, vec![SideEffect::RequestSave]);
    }

    #[test]
    fn reduce_does_not_mutate_other_fields() {
        // EditText must not disturb geometry/metadata.
        let mut a = node("a", 7.0, 8.0);
        a.width = 222.0;
        a.height = 99.0;
        a.tags = vec!["keep".into()];
        let board = board_with(vec![a], vec![]);
        let (out, _) = reduce(
            board,
            BoardAction::EditText {
                id: "a".into(),
                text: "new".into(),
            },
        );
        let a = &out.nodes[0];
        assert_eq!((a.x, a.y, a.width, a.height), (7.0, 8.0, 222.0, 99.0));
        assert_eq!(a.tags, vec!["keep".to_string()]);
        assert_eq!(a.text, "new");
    }
}
