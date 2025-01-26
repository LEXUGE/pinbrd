use anyhow::anyhow;
use clap::Parser;
use eframe::{run_native, App, CreationContext, NativeOptions};
use egui::{Context, TopBottomPanel};
use graph::{BlobType, PinboardGraph};
use petgraph::stable_graph::StableGraph;
use pinboard::*;
use poll_promise::Promise;
use rfd::FileDialog;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

mod graph;
mod pinboard;

pub struct PinlabApp {
    // Each pinboard is identified with an UUID, no matter it's saved or not. When saving, the uuid
    // will be stored into the pinboard file.
    // NOTE: The bool represents if the pinboard window is open
    pinboards: HashMap<Uuid, (PinboardBuffer, bool)>,

    boards_to_open: Vec<Option<Promise<anyhow::Result<PinboardBuffer>>>>,

    nvim_ext: Vec<String>,
    nvim_srv: Option<String>,
}

impl PinlabApp {
    fn new(cc: &CreationContext<'_>, args: Args) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        Self {
            pinboards: HashMap::new(),
            boards_to_open: Vec::default(),
            nvim_srv: args.nvim_srv,
            nvim_ext: args
                .nvim_ext
                .unwrap_or(vec!["md".into(), "markdown".into()]),
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

    async fn open_pinboard() -> anyhow::Result<PinboardBuffer> {
        if let Some(path) = FileDialog::new()
            // https://github.com/PolyMeilex/rfd/issues/235
            .set_directory(Path::new(".").canonicalize()?)
            .add_filter("Pinboard", &["pinbrd"])
            .pick_file()
        {
            return Ok(Self::open_pinboard_from_path(&path).await?);
        }
        Err(anyhow!("user canceled opening"))
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
                            .push(Some(Promise::spawn_async(Self::open_pinboard())));
                        ui.close_menu();
                    }
                });
            });
        });
    }
}

fn handle_promise<T: Send + 'static, R>(
    p: &mut Option<Promise<T>>,
    f: impl FnOnce(&T) -> R,
) -> Option<R> {
    // workaround to the borrow checker
    let mut flag = false;
    let res = p
        .as_ref()
        .map(|promise| {
            promise.ready().map(|t| {
                flag = true;
                f(t)
            })
        })
        .flatten();
    if flag {
        *p = None;
    }
    return res;
}

impl App for PinlabApp {
    fn update(&mut self, ctx: &Context, _: &mut eframe::Frame) {
        self.show_menu_bar(ctx);

        for (p, open) in self.pinboards.values_mut() {
            if let Some(b) = p.show(ctx, open) {
                async fn _h(path: PathBuf) -> anyhow::Result<PinboardBuffer> {
                    PinlabApp::open_pinboard_from_path(&path).await
                }
                match b.ty() {
                    BlobType::File => {
                        match if let Some(srv) = &self.nvim_srv {
                            // If matches any of the extension we want to launch in neovim
                            if Some(true)
                                == b.path()
                                    .extension()
                                    .map(|s| s.to_str())
                                    .flatten()
                                    .map(|ext| self.nvim_ext.iter().any(|e| e.as_str() == ext))
                            {
                                std::process::Command::new("nvim")
                                    .arg("--server")
                                    .arg(srv)
                                    .arg("--remote")
                                    .arg(b.path())
                                    .spawn()
                                    .map(|_| ())
                            } else {
                                // if not matched, open in default as well
                                open::that(b.path())
                            }
                        } else {
                            open::that(b.path())
                        } {
                            // print out error if any
                            Err(e) => eprintln!("{}", e),
                            _ => {}
                        }
                    }
                    BlobType::PinboardGraph => self
                        .boards_to_open
                        .push(Some(Promise::spawn_async(_h(b.path().to_path_buf())))),
                }
            }
        }

        // Handle board opening
        // WARN: we need to do some terrible workaround...
        let mut indices_to_remove = Vec::with_capacity(self.boards_to_open.len());
        for (i, p) in self.boards_to_open.iter_mut().enumerate() {
            if let Some(promise) = p {
                match promise.ready() {
                    Some(Ok(_)) => indices_to_remove.push(i),
                    Some(Err(e)) => {
                        eprintln!("{}", e);
                        *p = None;
                    }
                    None => {}
                }
            }
        }

        // We have already removed these indices so we wouldn't need to replace them with None
        for i in indices_to_remove {
            let buf = self
                .boards_to_open
                .remove(i)
                .unwrap()
                .try_take()
                .unwrap_or_else(|_| panic!("this shouldn't happened!"))
                .unwrap();
            if let Some(p) = self.pinboards.get_mut(&buf.pinboard.get_uuid()) {
                p.1 = true;
            } else {
                self.pinboards.insert(*buf.pinboard.get_uuid(), (buf, true));
            }
        }
        self.boards_to_open.retain(Option::is_some);
    }

    // fn save(&mut self, storage: &mut dyn Storage) {
    //     // eframe::set_value(storage, "pinlab_state", &self.g);
    // }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// neovim server address. If not provided, then all files will be opened via default apps.
    #[arg(short, long)]
    nvim_srv: Option<String>,

    /// types of files to launch in neovim remotely
    #[arg(short, long)]
    nvim_ext: Option<Vec<String>>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    run_native(
        "Pinlab",
        NativeOptions::default(),
        Box::new(|cc| Ok(Box::new(PinlabApp::new(cc, args)))),
    )
    .unwrap();
}
