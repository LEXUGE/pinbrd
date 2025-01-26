use crate::{
    graph::{Blob, BlobType, Conn, PinboardGraph, PinboardGraphView, Relation},
    handle_promise,
};
use anyhow::{anyhow, Result};
use crossbeam::channel::{unbounded, Receiver, Sender};
use egui::{Button, Context, Id, Key, KeyboardShortcut, Modal, Modifiers, Pos2, Ui, Window};
use egui_graphs::{events::Event, Metadata, SettingsInteraction, SettingsNavigation};
use petgraph::{graph::NodeIndex, prelude::EdgeIndex, stable_graph::StableGraph};
use poll_promise::Promise;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// A single pinboard
#[derive(Clone, Serialize, Deserialize)]
pub struct Pinboard {
    // UUID identifying the pinboard,
    // this must be the same as the key in HashMap storing the pinboard.
    uuid: Uuid,
    pub title: String,
    // underlying graph
    pub graph: PinboardGraph,
}

impl Pinboard {
    pub fn get_uuid(&self) -> &Uuid {
        &self.uuid
    }
    pub fn new(title: String, graph: PinboardGraph) -> Self {
        Self {
            title,
            graph,
            uuid: Uuid::new_v4(),
        }
    }
}

impl Default for Pinboard {
    fn default() -> Self {
        Self {
            uuid: Uuid::default(),
            title: String::new(),
            graph: PinboardGraph::from(&StableGraph::default()),
        }
    }
}

enum Either {
    Edge(EdgeIndex),
    Node(NodeIndex),
}

// A single pinboard buffer, handles the opening etc
pub struct PinboardBuffer {
    pub pinboard: Pinboard,
    // path of the pinboard file
    path: Option<PathBuf>,
    // containing unsaved changes
    unsaved: bool,

    // For widget events
    event_publisher: Sender<Event>,
    event_receiver: Receiver<Event>,

    // UI related states
    show_rename_modal: bool,

    // Promises
    save_file_promise: Option<Promise<Result<PathBuf>>>,
    update_blob_promise: Option<Promise<(Either, Result<Blob>)>>,
    update_blob_and_open_promise: Option<Promise<(Either, Result<Blob>)>>,
}

impl Default for PinboardBuffer {
    fn default() -> Self {
        let (event_publisher, event_receiver) = unbounded();
        Self {
            pinboard: Pinboard::default(),
            path: None,
            event_publisher,
            event_receiver,
            show_rename_modal: false,
            save_file_promise: None,
            update_blob_promise: None,
            update_blob_and_open_promise: None,
            unsaved: false,
        }
    }
}

impl PinboardBuffer {
    pub fn new(pinboard: Pinboard, path: Option<PathBuf>, unsaved: bool) -> Self {
        PinboardBuffer {
            path,
            pinboard,
            unsaved,
            ..Default::default()
        }
    }
    async fn save_as(pinboard: Pinboard) -> anyhow::Result<PathBuf> {
        if let Some(path) = FileDialog::new()
            // https://github.com/PolyMeilex/rfd/issues/235
            .set_directory(Path::new(".").canonicalize()?)
            .add_filter("Pinboard", &["pinbrd"])
            .save_file()
        {
            Self::save_to_path(pinboard, path).await
        } else {
            Err(anyhow::anyhow!(
                "user didn't select path to save pinboard {}",
                pinboard.title
            ))
        }
    }

    async fn save_to_path(pinboard: Pinboard, path: PathBuf) -> anyhow::Result<PathBuf> {
        let content = serde_json::to_string(&pinboard)?;
        tokio::fs::write(&path, &content).await?;
        Ok(path)
    }

    fn save(&mut self) {
        let path = self.path.clone();
        let pinboard = self.pinboard.clone();
        self.save_file_promise = Some(Promise::spawn_async(async {
            if let Some(path) = path {
                Self::save_to_path(pinboard, path).await
            } else {
                Self::save_as(pinboard).await
            }
        }));
    }

    fn show_rename_dialog(&mut self, ui: &Ui) {
        if self.show_rename_modal {
            Modal::new(ui.next_auto_id()).show(ui.ctx(), |ui| {
                ui.label("Enter the new name:");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.pinboard.title);

                    if ui.button("Done").clicked() {
                        self.show_rename_modal = false;
                        self.unsaved = true;
                    }
                })
            });
        }
    }

    fn handle_events(&mut self) {
        for e in self.event_receiver.try_iter() {
            match e {
                Event::EdgeDoubleClick(payload) => {
                    let root = giro::git_root(self.path.as_ref().unwrap())
                        .unwrap_or(None)
                        .unwrap_or(Path::new(".").to_path_buf());
                    let edge_id = EdgeIndex::new(payload.id);

                    if let Some(mut blob) = self
                        .pinboard
                        .graph
                        .edge(edge_id)
                        .map(|e| e.payload().comment.clone())
                        .flatten()
                    {
                        self.update_blob_and_open_promise =
                            Some(Promise::spawn_blocking(move || -> _ {
                                match blob.update(&root) {
                                    Ok(()) => (Either::Edge(edge_id), Ok(blob)),
                                    Err(e) => (Either::Edge(edge_id), Err(e)),
                                }
                            }));
                        return;
                    }
                }
                Event::NodeDoubleClick(payload) => {
                    let root = giro::git_root(self.path.as_ref().unwrap())
                        .unwrap_or(None)
                        .unwrap_or(Path::new(".").to_path_buf());
                    let node_id = NodeIndex::new(payload.id);

                    if let Some(mut blob) = self
                        .pinboard
                        .graph
                        .node(node_id)
                        .map(|n| n.payload().clone())
                        .flatten()
                    {
                        self.update_blob_and_open_promise =
                            Some(Promise::spawn_blocking(move || -> _ {
                                match blob.update(&root) {
                                    Ok(()) => (Either::Node(node_id), Ok(blob)),
                                    Err(e) => (Either::Node(node_id), Err(e)),
                                }
                            }));
                        return;
                    }
                }
                Event::NodeMove(_) => self.unsaved = true,
                _ => {}
            }
        }
    }

    async fn add_blob() -> Result<Blob> {
        let path = FileDialog::new()
            // https://github.com/PolyMeilex/rfd/issues/235
            .set_directory(Path::new(".").canonicalize()?)
            .pick_file()
            .ok_or(anyhow!("user didn't select file"))?;

        match path.extension().map(|s| s.to_str()).flatten() {
            Some("pinbrd") => Blob::new(BlobType::PinboardGraph, path.to_path_buf()).await,
            _ => Blob::new(BlobType::File, path.to_path_buf()).await,
        }
    }

    fn add_node(&mut self, pos: Option<Pos2>, metadata: &Metadata) {
        let id = if let Some(pos) = pos {
            self.pinboard
                .graph
                .add_node_with_location(None, metadata.screen_to_canvas_pos(pos))
        } else {
            self.pinboard.graph.add_node(None)
        };
        self.update_blob_promise = Some(Promise::spawn_async(async move {
            (Either::Node(id), Self::add_blob().await)
        }));
    }

    // Display the UI and optionally return the Blob to preview
    pub fn show(&mut self, ctx: &Context, open: &mut bool) -> Option<Blob> {
        let mut metadata = Metadata::default();
        let id = Id::new(self.pinboard.uuid);
        // keyboard shortcuts
        let save_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::S);
        let rename_shortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F2);
        let add_node_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::N);
        let title = format!(
            "{}{}",
            self.pinboard.title.as_str(),
            if self.unsaved { "*" } else { "" }
        );

        Window::new(title)
            // Set UUID as Id to avoid collision
            .id(id)
            .open(open)
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui
                            .add(
                                Button::new("Save")
                                    .shortcut_text(ctx.format_shortcut(&save_shortcut)),
                            )
                            .clicked()
                        {
                            self.save();
                            ui.close_menu();
                        }
                        if ui
                            .add(
                                Button::new("Rename")
                                    .shortcut_text(ctx.format_shortcut(&rename_shortcut)),
                            )
                            .clicked()
                        {
                            self.show_rename_modal = true;
                            ui.close_menu();
                        }
                    });
                    if ui.button("Reset View").clicked() {
                        PinboardGraphView::reset_metadata(id, ui);
                    }
                });
                ui.separator();
                let resp = ui.add(
                    // We cannot save graphview because it borrows the underlying graph. And we
                    // cannot do self-referential struct...
                    &mut PinboardGraphView::new(&mut self.pinboard.graph, id)
                        .with_interactions(
                            &SettingsInteraction::new()
                                .with_dragging_enabled(true)
                                .with_node_clicking_enabled(true)
                                .with_node_selection_enabled(true)
                                .with_node_selection_multi_enabled(true)
                                .with_edge_selection_enabled(true)
                                .with_edge_selection_multi_enabled(true),
                        )
                        .with_navigations(
                            &SettingsNavigation::new()
                                .with_zoom_and_pan_enabled(true)
                                .with_fit_to_screen_enabled(false),
                        )
                        .with_events(&self.event_publisher),
                );

                // Technically you could also directly use context.data_mut, but we wouldn't bother
                // to write it like that.
                // NOTE: It's important to make sure metadata is updated before we process cursor
                // information
                metadata = Metadata::load(ui, id);

                if resp.hovered() {
                    // Process keyboard shortcuts
                    if ui.input_mut(|i| i.consume_shortcut(&save_shortcut)) {
                        self.save();
                    }
                    if ui.input_mut(|i| i.consume_shortcut(&rename_shortcut)) {
                        self.show_rename_modal = true;
                    }
                    if ui.input_mut(|i| i.consume_shortcut(&add_node_shortcut)) {
                        let pos = ui.input(|i| i.pointer.hover_pos());
                        self.add_node(pos, &metadata);
                    }
                }

                resp.context_menu(|ui| {
                    // Position when user interacted in the context menu, this value should be
                    // saved for the use of node addition later, either passing through closure.
                    let pos = ui.input(|i| i.pointer.interact_pos());
                    // TODO: These should spun up a property sidepanel and ask user to put their
                    // stuff there
                    if ui.button("Add node").clicked() {
                        self.add_node(pos, &metadata);
                        ui.close_menu();
                    }

                    // Display context menu based on what we have selected
                    if self.pinboard.graph.selected_nodes().len() > 0 {
                        ui.separator();
                        if ui.button("Delete selected node(s)").clicked() {
                            for n in Vec::from(self.pinboard.graph.selected_nodes()) {
                                self.pinboard.graph.remove_node(n);
                            }
                            self.unsaved = true;
                            ui.close_menu();
                        }
                    }

                    // If we have two nodes selected, offer an option to connect them by edge
                    if self.pinboard.graph.selected_nodes().len() == 2 {
                        let a = self.pinboard.graph.selected_nodes()[0];
                        let b = self.pinboard.graph.selected_nodes()[1];
                        if self.pinboard.graph.g().find_edge(a, b).is_none() {
                            ui.menu_button("Connect with", |ui| {
                                let selected = Vec::from(self.pinboard.graph.selected_nodes());
                                let mut relation = Relation::Related;
                                let mut clicked = false;
                                if ui.button("Related").clicked() {
                                    relation = Relation::Related;
                                    clicked = true;
                                    ui.close_menu();
                                }
                                if ui.button("Insight").clicked() {
                                    relation = Relation::Insight;
                                    clicked = true;
                                    ui.close_menu();
                                }
                                if ui.button("Progress").clicked() {
                                    relation = Relation::Progress;
                                    clicked = true;
                                    ui.close_menu();
                                }
                                if ui.button("Conflict").clicked() {
                                    relation = Relation::Conflict;
                                    clicked = true;
                                    ui.close_menu();
                                }
                                if clicked {
                                    let label = relation.label();
                                    self.pinboard.graph.add_edge_with_label(
                                        selected[0],
                                        selected[1],
                                        Conn {
                                            comment: None,
                                            relation,
                                        },
                                        label,
                                    );
                                    self.unsaved = true;
                                }
                            });
                        }
                    }

                    if self.pinboard.graph.selected_edges().len() == 1 {
                        let id = self.pinboard.graph.selected_edges()[0];
                        if ui.button("Add to the Edge").clicked() {
                            self.update_blob_promise = Some(Promise::spawn_async(async move {
                                (Either::Edge(id), Self::add_blob().await)
                            }));
                            ui.close_menu();
                        }

                        ui.menu_button("Change Relation", |ui| {
                            let mut relation = Relation::Related;
                            let mut clicked = false;
                            if ui.button("Related").clicked() {
                                relation = Relation::Related;
                                clicked = true;
                                ui.close_menu();
                            }
                            if ui.button("Insight").clicked() {
                                relation = Relation::Insight;
                                clicked = true;
                                ui.close_menu();
                            }
                            if ui.button("Progress").clicked() {
                                relation = Relation::Progress;
                                clicked = true;
                                ui.close_menu();
                            }
                            if ui.button("Conflict").clicked() {
                                relation = Relation::Conflict;
                                clicked = true;
                                ui.close_menu();
                            }
                            if clicked {
                                let edge = self.pinboard.graph.edge_mut(id).unwrap();
                                edge.set_label(relation.label());
                                edge.payload_mut().relation = relation;
                                self.unsaved = true;
                            }
                        });
                    }

                    if self.pinboard.graph.selected_edges().len() > 0 {
                        if ui.button("Delete selected edge(s)").clicked() {
                            for e in Vec::from(self.pinboard.graph.selected_edges()) {
                                self.pinboard.graph.remove_edge(e);
                            }
                            self.unsaved = true;
                            ui.close_menu();
                        }
                    }
                });

                self.show_rename_dialog(ui);
            });

        self.handle_events();

        // Handle Promises
        handle_promise(&mut self.save_file_promise, |r| match r {
            Ok(p) => {
                self.path = Some(p.to_path_buf());
                self.unsaved = false;
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        });

        handle_promise(&mut self.update_blob_promise, |(either, b)| match b {
            Ok(blob) => {
                Self::handle_update_blob_to_node(
                    &mut self.unsaved,
                    &mut self.pinboard.graph,
                    either,
                    blob,
                );
            }
            Err(e) => {
                eprintln!("cannot open blob: {}", e);
            }
        });

        handle_promise(
            &mut self.update_blob_and_open_promise,
            |(either, b)| match b {
                Ok(blob) => {
                    Self::handle_update_blob_to_node(
                        &mut self.unsaved,
                        &mut self.pinboard.graph,
                        either,
                        blob,
                    );
                    Some(blob.clone())
                }
                Err(e) => {
                    eprintln!("cannot open blob: {}", e);
                    None
                }
            },
        )
        .flatten()
    }

    // Borrow checker is too dumb to infer across function call that we are mutably borrowing
    // different part of a struct
    fn handle_update_blob_to_node(
        unsaved: &mut bool,
        graph: &mut PinboardGraph,
        either: &Either,
        blob: &Blob,
    ) {
        let filename = blob
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        match either {
            Either::Edge(id) => {
                graph.edge_mut(*id).map(|e| {
                    e.payload_mut().comment = Some(blob.clone());
                    e.set_label(filename);
                });
            }
            Either::Node(id) => {
                graph.node_mut(*id).map(|n| {
                    *n.payload_mut() = Some(blob.clone());
                    n.set_label(filename);
                });
            }
        };
        *unsaved = true;
    }
}
