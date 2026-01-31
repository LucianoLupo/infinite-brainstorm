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
