//! Round-trip + persistence tests for the pure board IO core
//! (`load_board_at` / `write_board_atomic`). Run against real temp dirs so the
//! on-disk format is exercised end to end, not just in-memory serde.

use infinite_brainstorm_lib::{load_board_at, write_board_atomic, Board, Edge, Node};

fn sample_node(id: &str, text: &str) -> Node {
    Node {
        id: id.to_string(),
        x: 10.0,
        y: 20.0,
        width: 200.0,
        height: 100.0,
        text: text.to_string(),
        node_type: "text".to_string(),
        color: None,
        tags: vec![],
        status: None,
        group: None,
        priority: None,
    }
}

fn decorated_board() -> Board {
    Board {
        nodes: vec![
            sample_node("n1", "Hello"),
            Node {
                id: "n2".to_string(),
                x: 250.0,
                y: 0.0,
                width: 240.0,
                height: 120.0,
                text: "Decorated".to_string(),
                node_type: "idea".to_string(),
                color: Some("#ff6600".to_string()),
                tags: vec!["urgent".to_string(), "pricing".to_string()],
                status: Some("in-progress".to_string()),
                group: Some("cluster-a".to_string()),
                priority: Some(2),
            },
        ],
        edges: vec![Edge {
            id: "e1".to_string(),
            from_node: "n1".to_string(),
            to_node: "n2".to_string(),
            label: Some("depends on".to_string()),
        }],
    }
}

#[test]
fn write_then_load_round_trips_to_equal_board() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.json");

    let original = decorated_board();
    write_board_atomic(&path, &original).expect("atomic write should succeed");

    let loaded = load_board_at(&path).expect("load should succeed");
    assert_eq!(
        original, loaded,
        "loaded board must equal the written board"
    );
}

#[test]
fn save_produces_identical_redeserialize() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.json");

    let board = decorated_board();
    write_board_atomic(&path, &board).unwrap();

    // The persisted JSON deserializes back to an identical board.
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: Board = serde_json::from_str(&content).unwrap();
    assert_eq!(board, parsed);

    // And a second save of the same board yields byte-identical contents.
    let first_bytes = std::fs::read(&path).unwrap();
    write_board_atomic(&path, &board).unwrap();
    let second_bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        first_bytes, second_bytes,
        "re-saving an unchanged board must be byte-stable"
    );
}

#[test]
fn load_missing_file_returns_empty_board() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("does_not_exist.json");

    let board = load_board_at(&path).expect("missing file should be Ok(empty board)");
    assert_eq!(board, Board::default());
    assert!(board.nodes.is_empty());
    assert!(board.edges.is_empty());
    // Loading must NOT create the file.
    assert!(!path.exists(), "load must not create the board file");
}

#[test]
fn load_malformed_json_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.json");
    std::fs::write(&path, "{ this is not valid json ]").unwrap();

    let result = load_board_at(&path);
    assert!(
        result.is_err(),
        "malformed JSON must return Err, not an empty board"
    );
}

#[test]
fn load_truncated_json_returns_err() {
    // A board that was cut off mid-write must surface as an error rather than
    // silently collapsing to an empty board (regression guard for F73).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.json");
    std::fs::write(&path, "{\"nodes\": [{\"id\": \"n1\", \"x\": 0").unwrap();

    assert!(load_board_at(&path).is_err());
}

/// The committed golden fixture exercises every optional metadata field plus an
/// edge label. We assert two things:
/// 1. It round-trips through serde to an equal `Board`.
/// 2. `write_board_atomic` re-serializes it *byte-identically* to the committed
///    file — so the on-disk format the app writes stays pinned and reviewable.
#[test]
fn golden_fixture_round_trips_byte_identically() {
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden_board.json");

    let golden_text = std::fs::read_to_string(&golden_path).expect("golden fixture must exist");

    // 1. Round-trip equality.
    let board: Board = serde_json::from_str(&golden_text).expect("golden fixture must parse");
    assert_eq!(board.nodes.len(), 2);
    assert_eq!(board.edges.len(), 1);
    let decorated = &board.nodes[1];
    assert_eq!(decorated.color.as_deref(), Some("#ff6600"));
    assert_eq!(
        decorated.tags,
        vec!["urgent".to_string(), "pricing".to_string()]
    );
    assert_eq!(decorated.status.as_deref(), Some("in-progress"));
    assert_eq!(decorated.group.as_deref(), Some("cluster-a"));
    assert_eq!(decorated.priority, Some(2));
    assert_eq!(board.edges[0].label.as_deref(), Some("depends on"));

    let reserialized: Board = serde_json::from_str(&golden_text).unwrap();
    assert_eq!(board, reserialized);

    // 2. Byte-identical write. `write_board_atomic` uses `to_string_pretty`
    //    (2-space indent), matching the fixture's formatting. We compare against
    //    the fixture text with a trailing newline trimmed (the writer emits no
    //    trailing newline).
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("board.json");
    write_board_atomic(&out, &board).unwrap();
    let written = std::fs::read_to_string(&out).unwrap();

    assert_eq!(
        written,
        golden_text.trim_end_matches('\n'),
        "writer output must match the committed golden fixture byte-for-byte"
    );
}
