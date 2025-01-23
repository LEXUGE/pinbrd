use eframe::{run_native, App, CreationContext, NativeOptions};
use egui::{Context, TopBottomPanel};
use graph::{Blob, PinboardGraph};
use lazy_async_promise::{
    BoxedSendError, DirectCacheAccess, ImmediateValuePromise, ImmediateValueState,
};
use petgraph::stable_graph::StableGraph;
use previewer::MarkdownPreviwer;
use rfd::FileDialog;
use std::{collections::HashMap, path::PathBuf};
use tokio_shutdown::Shutdown;
use uuid::Uuid;

mod graph;
mod pinboard;
mod previewer;

use pinboard::*;

pub struct PinlabApp {
    // Each pinboard is identified with an UUID, no matter it's saved or not. When saving, the uuid
    // will be stored into the pinboard file.
    // NOTE: The bool represents if the pinboard window is open
    pinboards: HashMap<Uuid, (PinboardBuffer, bool)>,
    previewer: MarkdownPreviwer,

    boards_to_open: Vec<Option<ImmediateValuePromise<Option<PinboardBuffer>>>>,
}

impl PinlabApp {
    fn new(cc: &CreationContext<'_>, _shutdown: Shutdown) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        Self {
            pinboards: HashMap::new(),
            previewer: MarkdownPreviwer::new(),
            boards_to_open: Vec::default(),
        }
    }

    fn new_pinboard(&mut self) {
        let pinboard = PinboardBuffer::new(
            Pinboard::new(
                "Untitled".to_string(),
                PinboardGraph::from(&StableGraph::default()),
            ),
            None,
            true,
        );
        self.pinboards
            .insert(*pinboard.pinboard.get_uuid(), (pinboard, true));
    }

    async fn open_pinboard() -> anyhow::Result<Option<PinboardBuffer>> {
        if let Some(path) = FileDialog::new()
            .add_filter("Pinboard", &["pinbrd"])
            .pick_file()
        {
            return Ok(Some(Self::open_pinboard_from_path(&path).await?));
        }
        Ok(None)
    }

    async fn open_pinboard_from_path(path: &PathBuf) -> anyhow::Result<PinboardBuffer> {
        let pinboard =
            serde_json::from_str::<Pinboard>(tokio::fs::read_to_string(&path).await?.as_str())?;
        Ok(PinboardBuffer::new(
            pinboard,
            Some(path.to_path_buf()),
            false,
        ))
    }

    fn show_menu_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Pinboards", |ui| {
                    if ui.button("New").clicked() {
                        self.new_pinboard();
                        ui.close_menu();
                    }

                    if ui.button("Open...").clicked() {
                        self.boards_to_open
                            .push(Some(new_promise(Self::open_pinboard())));
                        ui.close_menu();
                    }
                });
            });
        });
    }
}

fn handle_promise<T: Send + 'static, R>(
    promise: &mut Option<ImmediateValuePromise<T>>,
    handler: impl FnOnce(Result<T, BoxedSendError>) -> R,
) -> Option<R> {
    if let Some(state) = promise {
        let res = state.poll_state_mut().take_result().map(handler);
        match state.poll_state() {
            ImmediateValueState::Updating => {}
            _ => *promise = None,
        }
        res
    } else {
        None
    }
}

// Helper function
fn new_promise<
    U: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
>(
    updater: U,
) -> ImmediateValuePromise<T> {
    ImmediateValuePromise::new(async { updater.await.map_err(|e| BoxedSendError(Box::from(e))) })
}

impl App for PinlabApp {
    fn update(&mut self, ctx: &Context, _: &mut eframe::Frame) {
        self.show_menu_bar(ctx);

        for (p, open) in self.pinboards.values_mut() {
            if let Some(b) = p.show(ctx, open) {
                async fn _h(path: PathBuf) -> anyhow::Result<Option<PinboardBuffer>> {
                    return Ok(Some(PinlabApp::open_pinboard_from_path(&path).await?));
                }
                match b {
                    Blob::URI(uri) => open::that(uri).unwrap(),
                    Blob::File(path) => {
                        if Some("md") == path.extension().map(|s| s.to_str()).flatten() {
                            self.previewer.append(path);
                        } else {
                            open::that(path).unwrap()
                        }
                    }
                    Blob::PinboardGraph(path) => {
                        self.boards_to_open.push(Some(new_promise(_h(path))))
                    }
                }
            }
        }

        // Handle board opening
        for p in &mut self.boards_to_open {
            handle_promise(p, |r| match r {
                Ok(Some(buf)) => {
                    if let Some(p) = self.pinboards.get_mut(&buf.pinboard.get_uuid()) {
                        p.1 = true;
                    } else {
                        self.pinboards.insert(*buf.pinboard.get_uuid(), (buf, true));
                    }
                }
                Err(_e) => eprintln!("Failed to open pinboard"),
                _ => {}
            });
        }
        self.boards_to_open.retain(Option::is_some);

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| self.previewer.show(ui));
        });
    }

    // fn save(&mut self, storage: &mut dyn Storage) {
    //     // eframe::set_value(storage, "pinlab_state", &self.g);
    // }
}

#[tokio::main]
async fn main() {
    let shutdown = Shutdown::new().unwrap();
    run_native(
        "Pinlab",
        NativeOptions::default(),
        Box::new(|cc| Ok(Box::new(PinlabApp::new(cc, shutdown)))),
    )
    .unwrap();
}
