//! The input and output of graph layout.

use crate::ir::Rect;

/// Which way ranks advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[default]
    TB,
    BT,
    LR,
    RL,
}

impl Direction {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim() {
            "TB" | "TD" | "v" | "tb" | "td" => Self::TB,
            "BT" | "^" | "bt" => Self::BT,
            "LR" | ">" | "lr" => Self::LR,
            "RL" | "<" | "rl" => Self::RL,
            _ => return None,
        })
    }

    pub fn horizontal(self) -> bool {
        matches!(self, Self::LR | Self::RL)
    }

    /// A canonical (ranks run down) size or vector as it appears on screen.
    pub(crate) fn real_size(self, s: [f32; 2]) -> [f32; 2] {
        if self.horizontal() { [s[1], s[0]] } else { s }
    }

    /// An on-screen size in canonical terms.
    pub(crate) fn canonical_size(self, s: [f32; 2]) -> [f32; 2] {
        self.real_size(s)
    }

    /// A canonical point inside a `w × h` canonical box, on screen.
    pub(crate) fn to_real(self, p: [f32; 2], h: f32) -> [f32; 2] {
        match self {
            Self::TB => p,
            Self::BT => [p[0], h - p[1]],
            Self::LR => [p[1], p[0]],
            Self::RL => [h - p[1], p[0]],
        }
    }
}

/// A node to place, at its on-screen size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeSpec {
    pub size: [f32; 2],
    pub cluster: Option<usize>,
    /// The fraction of a side edges may attach along (rectangles use most
    /// of it, diamonds and circles keep to the middle).
    pub port_span: f32,
}

impl NodeSpec {
    pub fn new(size: [f32; 2]) -> Self {
        Self {
            size,
            cluster: None,
            port_span: 0.7,
        }
    }
}

/// A group drawn as a box around its members.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterSpec {
    pub parent: Option<usize>,
    /// Its own direction. A group with edges leaving it follows its
    /// parent's, as Mermaid does.
    pub direction: Option<Direction>,
    /// The title's on-screen size; zero for none.
    pub title: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Node(usize),
    Cluster(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSpec {
    pub from: End,
    pub to: End,
    /// The label's on-screen size.
    pub label: Option<[f32; 2]>,
    /// Ranks the edge spans at least.
    pub min_len: u32,
    /// How strongly the edge pulls its ends together.
    pub weight: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub direction: Direction,
    pub nodes: Vec<NodeSpec>,
    pub clusters: Vec<ClusterSpec>,
    pub edges: Vec<EdgeSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    pub node_sep: f32,
    pub rank_sep: f32,
    pub edge_sep: f32,
    pub cluster_pad: f32,
    /// Minimum gap between parallel horizontal edge runs.
    pub track_sep: f32,
    /// Gap between neighbouring edge ends on one node side.
    pub port_gap: f32,
    /// How far a self-loop reaches out from its node.
    pub self_loop: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    /// An orthogonal polyline from the source's outline to the target's.
    Normal,
    /// A loop on one side of its node.
    SelfLoop,
    /// An edge between a group and its own member: not drawn.
    Dropped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub points: Vec<[f32; 2]>,
    /// Centre of the label, when the edge has one.
    pub label: Option<[f32; 2]>,
    pub kind: RouteKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphLayout {
    pub nodes: Vec<Rect>,
    pub clusters: Vec<Rect>,
    /// Where each cluster's title goes.
    pub titles: Vec<Rect>,
    /// The direction each cluster was laid out in.
    pub cluster_directions: Vec<Direction>,
    pub edges: Vec<Route>,
    pub bounds: Rect,
}
