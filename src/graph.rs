// This module contains implementation of the data model and graph-related peripherals.

use egui::Color32;
use egui_graphs::{
    DefaultEdgeShape, DefaultNodeShape, DisplayEdge, DisplayNode, DrawContext, EdgeProps, Graph,
    GraphView, Node, NodeProps,
};
use petgraph::{
    csr::{DefaultIx, IndexType},
    EdgeType, Undirected,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A blob is any document (e.g. PDF, image, hyperlink, etc. or even a pinboard!)
// NOTE: Cloning an trait object is impossible, that's why we didn't implement in that style
#[derive(Serialize, Deserialize, Clone)]
pub enum Blob {
    PinboardGraph(PathBuf),
    File(PathBuf),
}

impl Blob {
    pub fn color(&self) -> Option<Color32> {
        match self {
            Blob::PinboardGraph(_) => Some(Color32::LIGHT_BLUE),
            Blob::File(_) => None,
        }
    }
}

/// Relation between nodes
#[derive(Serialize, Deserialize, Clone)]
pub enum Relation {
    Conflict,
    Insight,
    Related,
}

impl Relation {
    pub fn color(&self) -> Option<Color32> {
        match self {
            Self::Conflict => Some(Color32::LIGHT_RED),
            Self::Insight => Some(Color32::LIGHT_GREEN),
            // Color should be determined by foregrapund default
            Self::Related => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Relation::Conflict => "Conflict",
            Relation::Insight => "Insight",
            Relation::Related => "Related",
        }
        .to_string()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Conn {
    pub comment: Option<Blob>,
    pub relation: Relation,
}

pub type PinboardGraph = Graph<Blob, Conn, Undirected, DefaultIx, MyNodeShape, MyEdgeShape>;

pub type PinboardGraphView<'a> =
    GraphView<'a, Blob, Conn, Undirected, DefaultIx, MyNodeShape, MyEdgeShape>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyNodeShape {
    super_shape: DefaultNodeShape,
}

impl<E: Clone, Ty: EdgeType, Ix: IndexType> DisplayNode<Blob, E, Ty, Ix> for MyNodeShape {
    fn closest_boundary_point(&self, dir: egui::Vec2) -> egui::Pos2 {
        <DefaultNodeShape as DisplayNode<Blob, E, Ty, Ix>>::closest_boundary_point(
            &self.super_shape,
            dir,
        )
    }

    fn shapes(&mut self, ctx: &DrawContext) -> Vec<egui::Shape> {
        <DefaultNodeShape as DisplayNode<Blob, E, Ty, Ix>>::shapes(&mut self.super_shape, ctx)
    }

    fn update(&mut self, state: &NodeProps<Blob>) {
        <DefaultNodeShape as DisplayNode<Blob, E, Ty, Ix>>::update(&mut self.super_shape, state);
        self.super_shape.color = state.payload.color();
    }

    fn is_inside(&self, pos: egui::Pos2) -> bool {
        <DefaultNodeShape as DisplayNode<Blob, E, Ty, Ix>>::is_inside(&self.super_shape, pos)
    }
}

impl From<NodeProps<Blob>> for MyNodeShape {
    fn from(node_props: NodeProps<Blob>) -> Self {
        let color = node_props.payload.color();
        let mut super_shape = DefaultNodeShape::from(node_props);
        super_shape.color = color;
        Self { super_shape }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyEdgeShape {
    super_shape: DefaultEdgeShape,
}

impl From<EdgeProps<Conn>> for MyEdgeShape {
    fn from(edge: EdgeProps<Conn>) -> Self {
        let color = edge.payload.relation.color();
        let mut super_shape = DefaultEdgeShape::from(edge);
        super_shape.color = dbg!(color);
        Self { super_shape }
    }
}

impl<N: Clone, Ty: EdgeType, Ix: IndexType, D: DisplayNode<N, Conn, Ty, Ix>>
    DisplayEdge<N, Conn, Ty, Ix, D> for MyEdgeShape
{
    fn shapes(
        &mut self,
        start: &Node<N, Conn, Ty, Ix, D>,
        end: &Node<N, Conn, Ty, Ix, D>,
        ctx: &DrawContext,
    ) -> Vec<egui::Shape> {
        self.super_shape.shapes(start, end, ctx)
    }

    fn update(&mut self, state: &EdgeProps<Conn>) {
        <DefaultEdgeShape as DisplayEdge<N, Conn, Ty, Ix, D>>::update(&mut self.super_shape, state);
        self.super_shape.color = state.payload.relation.color();
    }

    fn is_inside(
        &self,
        start: &egui_graphs::Node<N, Conn, Ty, Ix, D>,
        end: &egui_graphs::Node<N, Conn, Ty, Ix, D>,
        pos: egui::Pos2,
    ) -> bool {
        self.super_shape.is_inside(start, end, pos)
    }
}
