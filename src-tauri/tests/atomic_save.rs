use infinite_brainstorm_lib::{write_board_atomic, Board, Edge, Node, NodeType};

fn sample_node(id: &str, text: &str) -> Node {
    Node {
        id: id.to_string(),
        x: 10.0,
        y: 20.0,
        width: 200.0,
        height: 100.0,
        text: text.to_string(),
        node_type: NodeType::Text,
        color: None,
        tags: vec![],
        status: None,
        group: None,
        priority: None,
    }
}

fn sample_board() -> Board {
    Board {
        version: None,
        nodes: vec![sample_node("n1", "Hello"), sample_node("n2", "World")],
        edges: vec![Edge {
            id: "e1".to_string(),
            from_node: "n1".to_string(),
            to_node: "n2".to_string(),
            label: Some("connects".to_string()),
        }],
    }
}

#[test]
fn writes_final_file_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.json");

    let board = sample_board();
    write_board_atomic(&path, &board).expect("atomic write should succeed");

    // Final file exists and the temp file was renamed away (no leftover).
    assert!(path.exists(), "board.json should exist after save");
    let tmp = dir.path().join("board.json.tmp");
    assert!(
        !tmp.exists(),
        "temp file should be renamed into place, not left behind"
    );

    // Contents round-trip back to an equivalent board.
    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: Board = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed.nodes.len(), 2);
    assert_eq!(parsed.edges.len(), 1);
    assert_eq!(parsed.nodes[0].id, "n1");
    assert_eq!(parsed.nodes[1].text, "World");
    assert_eq!(parsed.edges[0].label.as_deref(), Some("connects"));
}

#[test]
fn second_save_backs_up_prior_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("board.json");
    let bak = dir.path().join("board.json.bak");

    // First save: no prior file, so no backup is produced.
    let first = sample_board();
    write_board_atomic(&path, &first).unwrap();
    assert!(!bak.exists(), "no backup should exist after the first save");

    // Second save: the prior on-disk contents are copied to .bak before rename.
    let mut second = sample_board();
    second.nodes.push(sample_node("n3", "Third"));
    write_board_atomic(&path, &second).unwrap();

    assert!(
        bak.exists(),
        "backup of prior contents should exist after a second save"
    );

    // The .bak holds the FIRST board; the live file holds the SECOND board.
    let bak_board: Board = serde_json::from_str(&std::fs::read_to_string(&bak).unwrap()).unwrap();
    let live_board: Board = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        bak_board.nodes.len(),
        2,
        "backup should hold the prior (2-node) board"
    );
    assert_eq!(
        live_board.nodes.len(),
        3,
        "live file should hold the new (3-node) board"
    );
}

#[test]
fn creates_missing_parent_directory() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b").join("board.json");

    write_board_atomic(&nested, &sample_board()).expect("should create parent dirs and write");
    assert!(nested.exists());
}
