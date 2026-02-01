use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub text: String,
    #[serde(default = "default_node_type")]
    pub node_type: String,
}

fn default_node_type() -> String {
    "text".to_string()
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
            node_type: "text".to_string(),
        }
    }

    pub fn contains_point(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Edge {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
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
        let world_x = (screen_x / self.zoom) + self.x;
        let world_y = (screen_y / self.zoom) + self.y;
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
            assert_eq!(node.node_type, "text");
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
                nodes: vec![
                    Node::new("n1".to_string(), 0.0, 0.0, "First".to_string()),
                    Node {
                        id: "n2".to_string(),
                        x: 250.0,
                        y: 0.0,
                        width: 200.0,
                        height: 100.0,
                        text: "Second".to_string(),
                        node_type: "idea".to_string(),
                    },
                ],
                edges: vec![Edge {
                    id: "e1".to_string(),
                    from_node: "n1".to_string(),
                    to_node: "n2".to_string(),
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
            assert_eq!(board.nodes[0].node_type, "text");
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
            };

            let json = serde_json::to_string(&edge).unwrap();
            let deserialized: Edge = serde_json::from_str(&json).unwrap();

            assert_eq!(edge, deserialized);
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
                node_type: "text".to_string(),
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
                })
                .collect();

            let board = Board { nodes, edges };

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
                    });
                    edge_id += 1;
                }
            }

            let board = Board { nodes, edges };

            let expected_edges = n * (n - 1) / 2;
            assert_eq!(board.edges.len(), expected_edges);

            let json = serde_json::to_string(&board).unwrap();
            let deserialized: Board = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.edges.len(), expected_edges);
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
}
