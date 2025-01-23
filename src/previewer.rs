use crossbeam::channel::{bounded, Receiver, Sender};
use egui::Ui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use lazy_async_promise::ImmediateValuePromise;
use notify::{EventHandler, PollWatcher, RecursiveMode, Watcher};
use std::{path::PathBuf, time::Duration};

use crate::{handle_promise, new_promise};

struct MarkdownBuffer {
    cache: CommonMarkCache,
    title: String,
    content: String,
    recv: Receiver<String>,
    _watcher: PollWatcher,
}

struct MyWatcher {
    path: PathBuf,
    send: Sender<String>,
}

impl EventHandler for MyWatcher {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        if event.is_ok() {
            println!("{:?}", event);
            if let Ok(content) = std::fs::read_to_string(&self.path) {
                println!("Good");
                self.send.send(content).unwrap();
            }
        }
    }
}

impl MarkdownBuffer {
    pub async fn new(path: PathBuf) -> anyhow::Result<Self> {
        let content = tokio::fs::read_to_string(&path).await?;
        let title = path.file_name().unwrap().to_str().unwrap().to_string();
        let (send, recv) = bounded(1);
        let mut watcher = notify::PollWatcher::new(
            MyWatcher {
                path: path.clone(),
                send,
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(500)),
        )?;
        watcher.watch(&path, RecursiveMode::NonRecursive)?;
        Ok(Self {
            cache: CommonMarkCache::default(),
            title,
            content,
            recv,
            _watcher: watcher,
        })
    }

    pub fn show(&mut self, ui: &mut Ui) {
        if let Ok(content) = self.recv.try_recv() {
            self.content = content;
        }
        egui::CollapsingHeader::new(self.title.as_str())
            .default_open(true)
            .show(ui, |ui| {
                CommonMarkViewer::new().show(ui, &mut self.cache, self.content.as_str());
            });
        ui.separator();
    }
}

pub struct MarkdownPreviwer {
    buffers: Vec<MarkdownBuffer>,
    append_promises: Vec<Option<ImmediateValuePromise<MarkdownBuffer>>>,
}

impl MarkdownPreviwer {
    pub fn new() -> Self {
        Self {
            buffers: Vec::new(),
            append_promises: Vec::new(),
        }
    }
    pub fn append(&mut self, path: PathBuf) {
        self.append_promises
            .push(Some(new_promise(MarkdownBuffer::new(path))))
    }

    pub fn handle_promises(&mut self) {
        for p in &mut self.append_promises {
            handle_promise(p, |r| {
                if let Ok(m) = r {
                    self.buffers.push(m);
                }
            });
        }
        self.append_promises.retain(Option::is_some);
    }

    pub fn show(&mut self, ui: &mut Ui) {
        for buf in &mut self.buffers {
            buf.show(ui);
        }

        self.handle_promises();
    }
}
