// This module contains implementation of the data model and graph-related peripherals.

use anyhow::anyhow;
use blake3::Hash as BlakeHash;
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
use std::path::{Path, PathBuf};

/// A blob is any document (e.g. PDF, image, hyperlink, etc. or even a pinboard!)
// NOTE: Cloning an trait object is impossible, that's why we didn't implement in that style
#[derive(Serialize, Deserialize, Clone)]
pub enum BlobType {
    PinboardGraph,
    File,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Blob {
    ty: BlobType,
    path: PathBuf,
    hash: BlakeHash,
}

impl Blob {
    pub async fn new(ty: BlobType, path: PathBuf) -> anyhow::Result<Self> {
        let content = tokio::fs::read(&path).await?;
        let hash = blake3::hash(&content);
        Ok(Self { ty, path, hash })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn ty(&self) -> &BlobType {
        &self.ty
    }

    fn walk(dir: &Path, hash: &BlakeHash) -> anyhow::Result<Option<PathBuf>> {
        let mut count = 0;
        let mut res = None;
        if dir.is_dir() {
            // Count number of files matching our hash
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                // If the path is not hidden
                // If errored, we assume it's hidden to play safe
                if !hf::is_hidden(&path).unwrap_or(true) {
                    if path.is_dir() {
                        if let Some(matched) = Self::walk(&path, hash)? {
                            res = Some(matched);
                            count += 1;
                        }
                    } else {
                        if blake3::hash(&std::fs::read(&path)?) == *hash {
                            res = Some(path);
                            count += 1;
                        }
                    }
                }
            }
        }
        if count > 1 {
            return Err(anyhow!(
                "multiple files matching targeted hash {} exists, aborting auto-repairing",
                hash
            ));
        }
        Ok(res)
    }

    /// Update the blob info
    /// If the path exists, then update the hash
    /// If the path is no longer accessible, then try find the _unique_ _unhidden_ file matching the current hash in
    /// the provided root
    /// If cannot find one file matching the hash, then error
    ///
    /// NOTE: root must be a folder
    /// This should be spawned as blocking
    pub fn update(&mut self, root: &Path) -> anyhow::Result<()> {
        match self.path.try_exists() {
            Ok(true) => {
                // File exists, update the hash
                self.hash = blake3::hash(&std::fs::read(&self.path)?);
            }
            Ok(false) => {
                // File doesn't exist or is not accessible, search from the path
                if let Some(path) = Self::walk(root, &self.hash)? {
                    self.path = path;
                }
            }
            Err(_) => {
                // Do nothing because it might be that we just have no permission to list the file
                // or something
            }
        }
        Ok(())
    }

    pub fn color(&self) -> Option<Color32> {
        self.ty.color()
    }
}

impl BlobType {
    pub fn color(&self) -> Option<Color32> {
        match self {
            BlobType::PinboardGraph => Some(Color32::LIGHT_BLUE),
            BlobType::File => None,
        }
    }
}

/// Relation between nodes
#[derive(Serialize, Deserialize, Clone)]
pub enum Relation {
    /// Contradicting or confusing
    Conflict,
    /// Partial progress towards understanding
    Progress,
    /// Non-trivial and interesting relation
    Insight,
    /// Easy to identify or probably trivial relation
    Related,
}

impl Relation {
    pub fn color(&self) -> Option<Color32> {
        match self {
            Self::Conflict => Some(Color32::LIGHT_RED),
            Self::Progress => Some(Color32::YELLOW),
            Self::Insight => Some(Color32::LIGHT_GREEN),
            // Color should be determined by foregrapund default
            Self::Related => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Relation::Conflict => "Conflict",
            Relation::Progress => "Progress",
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

pub type PinboardGraph = Graph<Option<Blob>, Conn, Undirected, DefaultIx, MyNodeShape, MyEdgeShape>;

pub type PinboardGraphView<'a> =
    GraphView<'a, Option<Blob>, Conn, Undirected, DefaultIx, MyNodeShape, MyEdgeShape>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyNodeShape {
    super_shape: DefaultNodeShape,
}

impl<E: Clone, Ty: EdgeType, Ix: IndexType> DisplayNode<Option<Blob>, E, Ty, Ix> for MyNodeShape {
    fn closest_boundary_point(&self, dir: egui::Vec2) -> egui::Pos2 {
        <DefaultNodeShape as DisplayNode<Blob, E, Ty, Ix>>::closest_boundary_point(
            &self.super_shape,
            dir,
        )
    }

    fn shapes(&mut self, ctx: &DrawContext) -> Vec<egui::Shape> {
        <DefaultNodeShape as DisplayNode<Blob, E, Ty, Ix>>::shapes(&mut self.super_shape, ctx)
    }

    fn is_inside(&self, pos: egui::Pos2) -> bool {
        <DefaultNodeShape as DisplayNode<Blob, E, Ty, Ix>>::is_inside(&self.super_shape, pos)
    }
}

impl From<NodeProps<Option<Blob>>> for MyNodeShape {
    fn from(node_props: NodeProps<Option<Blob>>) -> Self {
        let color = node_props.payload.as_ref().map(|b| b.color()).flatten();
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
        super_shape.color = color;
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

    fn is_inside(
        &self,
        start: &egui_graphs::Node<N, Conn, Ty, Ix, D>,
        end: &egui_graphs::Node<N, Conn, Ty, Ix, D>,
        pos: egui::Pos2,
    ) -> bool {
        self.super_shape.is_inside(start, end, pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updating_blob_multi_match() {
        let hash = blake3::hash(&std::fs::read(&Path::new("./tests/misc/foo.txt")).unwrap());
        assert!(Blob::walk(Path::new("./tests/misc/multi_match"), &hash).is_err());
    }

    #[test]
    fn blob_hidden_match() {
        let hash = blake3::hash(&std::fs::read(&Path::new("./tests/misc/foo.txt")).unwrap());
        // We cannot do assert_eq because of anyhow::Error doesn't implement PartialEq
        assert!(Blob::walk(Path::new("./tests/misc/hidden_match"), &hash)
            .map(|o| o.is_none())
            .unwrap());
    }

    #[test]
    fn blob_match() {
        let hash = blake3::hash(&std::fs::read(&Path::new("./tests/misc/foo.txt")).unwrap());
        // We cannot do assert_eq because of anyhow::Error doesn't implement PartialEq
        assert!(Blob::walk(Path::new("./tests/misc/match"), &hash)
            .map(|o| o.is_some())
            .unwrap());
    }
}
