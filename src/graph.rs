// This module contains implementation of the data model and graph-related peripherals.

use egui_graphs::{Graph, GraphView};
use petgraph::Undirected;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A blob is any document (e.g. PDF, image, hyperlink, etc. or even a pinboard!)
// NOTE: Cloning an trait object is impossible, that's why we didn't implement in that style
#[derive(Serialize, Deserialize, Clone)]
pub enum Blob {
    PinboardGraph(PathBuf),
    URI(String),
    File(PathBuf),
}

/// Relation between nodes
#[derive(Serialize, Deserialize, Clone)]
pub enum Relation {
    Conflict,
    Insight,
    Related,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Conn {
    pub comment: Option<Blob>,
    pub relation: Relation,
}

// TODO: Replace default node and edge shape with my own display
pub type PinboardGraph = Graph<Blob, Conn, Undirected>;

pub type PinboardGraphView<'a> = GraphView<'a, Blob, Conn, Undirected>;
