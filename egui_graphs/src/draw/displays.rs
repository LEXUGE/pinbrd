use egui::{Pos2, Shape, Vec2};
use petgraph::{stable_graph::IndexType, EdgeType};

use crate::{draw::drawer::DrawContext, elements::EdgeProps, Node, NodeProps};

pub trait DisplayNode<N, E, Ty, Ix>: Clone + From<NodeProps<N>>
where
    N: Clone,
    E: Clone,
    Ty: EdgeType,
    Ix: IndexType,
{
    /// Returns the closest point on the shape boundary in the direction of dir.
    ///
    /// * `dir` - direction pointing from the shape center to the required boundary point.
    ///
    /// Could be used to snap the edge ends to the node.
    fn closest_boundary_point(&self, dir: Vec2) -> Pos2;

    /// Draws shapes of the node. If the node is interacted these shapes will be used for drawing on foreground layer, otherwise on background layer.
    /// Has mutable reference to itself for possibility to change internal state for the visualizations where this is important.
    ///
    /// * `ctx` - contains [`egui::Context`] and graph metadata.
    ///
    /// Use `ctx.meta` to properly scale and translate the shape.
    /// Use `ctx.painter` to have low level access to egui painting process.
    fn shapes(&mut self, ctx: &DrawContext) -> Vec<Shape>;

    /// Checks if the provided `pos` is inside the shape.
    ///
    /// * `pos` - position is in the canvas coordinates.
    ///
    /// Could be used to bind mouse events to the custom drawn nodes.
    fn is_inside(&self, pos: Pos2) -> bool;
}

pub trait DisplayEdge<N, E, Ty, Ix, D>: Clone + From<EdgeProps<E>>
where
    N: Clone,
    E: Clone,
    Ty: EdgeType,
    Ix: IndexType,
    D: DisplayNode<N, E, Ty, Ix>,
{
    /// Draws shapes of the edge. Uses [`DisplayNode`] implementation from node endpoints to get start and end coordinates using [`closest_boundary_point`](DisplayNode::closest_boundary_point).
    /// If the node is interacted these shapes will be used for drawing on foreground layer, otherwise on background layer.
    /// Has mutable reference to itself for possibility to change internal state for the visualizations where this is important.
    ///
    /// * `ctx` - contains [`egui::Context`] and graph metadata.
    /// * `start` and `end` - start and end points of the edge.
    ///
    /// Use `ctx.meta` to properly scale and translate the shape.
    /// Use `ctx.painter` to have low level access to egui painting process.
    fn shapes(
        &mut self,
        start: &Node<N, E, Ty, Ix, D>,
        end: &Node<N, E, Ty, Ix, D>,
        ctx: &DrawContext,
    ) -> Vec<Shape>;

    /// Checks if the provided `pos` is inside the shape.
    ///
    /// * `start` - start node of the edge.
    /// * `end`   - end node of the edge.
    /// * `pos`   - position is in the canvas coordinates.
    ///
    /// Could be used to bind mouse events to the custom drawn nodes.
    fn is_inside(
        &self,
        start: &Node<N, E, Ty, Ix, D>,
        end: &Node<N, E, Ty, Ix, D>,
        pos: Pos2,
    ) -> bool;
}
