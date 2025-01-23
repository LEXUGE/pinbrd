use crate::{
    graph::{Blob, Conn, PinboardGraph, PinboardGraphView, Relation},
    handle_promise, new_promise,
};
use crossbeam::channel::{unbounded, Receiver, Sender};
use egui::{Button, Context, Id, Key, KeyboardShortcut, Modal, Modifiers, Pos2, Ui, Window};
use egui_graphs::{events::Event, Metadata, SettingsInteraction, SettingsNavigation};
use lazy_async_promise::ImmediateValuePromise;
use petgraph::{graph::NodeIndex, stable_graph::StableGraph};
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
    show_add_url_node_modal: bool,
    temp: String,
    context_menu_cursor_pos: Option<Pos2>,

    // Promises
    save_file_promise: Option<ImmediateValuePromise<Option<PathBuf>>>,
    add_file_node_promise: Option<ImmediateValuePromise<(Option<Pos2>, Option<PathBuf>)>>,
    add_pinboard_node_promise: Option<ImmediateValuePromise<(Option<Pos2>, Option<PathBuf>)>>,
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
            show_add_url_node_modal: false,
            temp: String::default(),
            context_menu_cursor_pos: None,
            save_file_promise: None,
            add_file_node_promise: None,
            add_pinboard_node_promise: None,
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
    async fn save_as(pinboard: Pinboard) -> anyhow::Result<Option<PathBuf>> {
        if let Some(path) = FileDialog::new()
            .add_filter("Pinboard", &["pinbrd"])
            .save_file()
        {
            Self::save_to_path(pinboard, path).await
        } else {
            Ok(None)
        }
    }

    async fn save_to_path(pinboard: Pinboard, path: PathBuf) -> anyhow::Result<Option<PathBuf>> {
        let content = serde_json::to_string(&pinboard)?;
        tokio::fs::write(&path, &content).await?;
        Ok(Some(path))
    }

    fn save(&mut self) {
        let path = self.path.clone();
        let pinboard = self.pinboard.clone();
        self.save_file_promise = Some(new_promise(async {
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
                    }
                })
            });
        }
    }

    fn show_add_url_node_dialog(&mut self, ui: &Ui, metadata: &Metadata) {
        if self.show_add_url_node_modal {
            Modal::new(ui.next_auto_id()).show(ui.ctx(), |ui| {
                let esc = ui.input_mut(|i| {
                    i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Escape))
                });
                ui.label("Enter URL:");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.temp);

                    if ui.button("Cancel").clicked() || esc {
                        self.temp = String::default();
                        self.show_add_url_node_modal = false;
                    }
                    if ui.button("Done").clicked() {
                        let n = Blob::URI(self.temp.clone());
                        if let Some(pos) = self.context_menu_cursor_pos {
                            self.pinboard.graph.add_node_with_location(
                                n,
                                metadata.screen_to_canvas_pos(dbg!(pos)),
                            );
                        } else {
                            self.pinboard.graph.add_node(n);
                        }
                        self.unsaved = true;
                        self.temp = String::default();
                        self.show_add_url_node_modal = false;
                    }
                })
            });
        }
    }

    fn handle_events(&mut self) -> Option<Blob> {
        let mut res = None;
        for e in self.event_receiver.try_iter() {
            match e {
                Event::NodeDoubleClick(payload) => {
                    let node_id = NodeIndex::new(payload.id);

                    res = self
                        .pinboard
                        .graph
                        .node(node_id)
                        .map(|n| n.payload().clone());
                }
                Event::NodeMove(_) => self.unsaved = true,
                _ => {}
            }
        }
        res
    }

    // Display the UI and optionally return the Blob to preview
    pub fn show(&mut self, ctx: &Context, open: &mut bool) -> Option<Blob> {
        let mut metadata = Metadata::default();
        let id = Id::new(self.pinboard.uuid);
        let save_shortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::S);
        let rename_shortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F2);
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
                // Process keyboard shortcuts
                if ui.input_mut(|i| i.consume_shortcut(&save_shortcut)) {
                    self.save()
                }
                if ui.input_mut(|i| i.consume_shortcut(&rename_shortcut)) {
                    self.show_rename_modal = true;
                }

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
                ui.add(
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
                )
                .context_menu(|ui| {
                    // Position when user interacted in the context menu, this value should be
                    // saved for the use of node addition later, either passing through closure.
                    let pos = ui.input(|i| i.pointer.interact_pos());
                    // TODO: These should spun up a property sidepanel and ask user to put their
                    // stuff there
                    ui.menu_button("Add node", |ui| {
                        if ui.button("File").clicked() {
                            self.add_file_node_promise = Some(new_promise(async move {
                                Ok((pos, FileDialog::new().pick_file()))
                            }));
                            ui.close_menu();
                        }

                        if ui.button("URL").clicked() {
                            self.show_add_url_node_modal = true;
                            self.context_menu_cursor_pos = pos;
                            ui.close_menu();
                        }

                        if ui.button("Pinboard").clicked() {
                            self.add_pinboard_node_promise = Some(new_promise(async move {
                                Ok((
                                    pos,
                                    FileDialog::new()
                                        .add_filter("Pinboard", &["pinbrd"])
                                        .pick_file(),
                                ))
                            }));
                            ui.close_menu();
                        }
                    });

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
                    // TODO: We should allow at most one edge between two nodes
                    if self.pinboard.graph.selected_nodes().len() == 2 {
                        ui.menu_button("Connect with", |ui| {
                            let selected = Vec::from(self.pinboard.graph.selected_nodes());
                            let mut relation = Relation::Insight;
                            let mut clicked = false;
                            if ui.button("Insight").clicked() {
                                clicked = true;
                                ui.close_menu();
                            }
                            if ui.button("Conflict").clicked() {
                                relation = Relation::Conflict;
                                clicked = true;
                                ui.close_menu();
                            }
                            if clicked {
                                self.pinboard.graph.add_edge(
                                    selected[0],
                                    selected[1],
                                    Conn {
                                        comment: None,
                                        relation,
                                    },
                                );
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

                // Technically you could also directly use context.data_mut, but we wouldn't bother
                // to write it like that.
                metadata = Metadata::load(ui, id);

                self.show_rename_dialog(ui);
                self.show_add_url_node_dialog(ui, &metadata);
            });

        // Handle Promises
        handle_promise(&mut self.save_file_promise, |r| match r {
            Ok(Some(path)) => {
                self.path = Some(path);
                self.unsaved = false;
            }
            Ok(None) => {}
            Err(_e) => eprintln!("Failed to save pinboard"),
        });

        handle_promise(&mut self.add_file_node_promise, |r| {
            let (press_pos, p) = r.unwrap_or((None, None));
            if let Some(path) = p {
                let n = Blob::File(path.to_str().unwrap().into());
                let filename = path.file_name().unwrap().to_str().unwrap().to_string();
                if let Some(pos) = press_pos {
                    self.pinboard.graph.add_node_with_label_and_location(
                        n,
                        filename,
                        metadata.screen_to_canvas_pos(pos),
                    );
                } else {
                    self.pinboard.graph.add_node_with_label(n, filename);
                }
                self.unsaved = true;
            }
        });

        handle_promise(&mut self.add_pinboard_node_promise, |r| {
            let (press_pos, p) = r.unwrap_or((None, None));
            if let Some(path) = p {
                let n = Blob::PinboardGraph(path.to_str().unwrap().into());
                let filename = path.file_name().unwrap().to_str().unwrap().to_string();
                if let Some(pos) = press_pos {
                    self.pinboard.graph.add_node_with_label_and_location(
                        n,
                        filename,
                        metadata.screen_to_canvas_pos(pos),
                    );
                } else {
                    self.pinboard.graph.add_node_with_label(n, filename);
                }
                self.unsaved = true;
            }
        });

        self.handle_events()
    }
}
