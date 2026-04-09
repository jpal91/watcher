use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

use anyhow::Result;
use ignore::gitignore::Gitignore;
use log::{error, info, trace};
use notify::{Event, EventHandler, RecommendedWatcher, RecursiveMode, Watcher};
use uuid::Uuid;

use crate::config::{EventFlags, WatchCommands, WatchItem, is_ignored};

struct WatchEvent {
    id: Uuid,
    paths: Vec<PathBuf>,
}

pub struct WatchFiles {
    watch_map: HashMap<Uuid, WatchCommandRunner>,
    sx: Sender<WatchEvent>,
    rx: Receiver<WatchEvent>,
}

impl WatchFiles {
    pub fn start(&mut self, items: WatchCommands) -> Result<()> {
        let mut watchers = vec![];

        for item in items {
            let notifier = match self.start_one(item) {
                Ok(n) => n,
                Err(e) => {
                    error!("{}", e);
                    continue;
                }
            };

            watchers.push(notifier);
        }

        loop {
            if let Ok(WatchEvent { id, paths }) = self.rx.recv_timeout(Duration::from_secs(1)) {
                // Definitely exists
                let watch = self.watch_map.get_mut(&id).unwrap();
                let now = Instant::now();

                watch.last_send = Some(now);

                for path in paths {
                    if let Some(ref g_ignore) = watch.git_ignore
                        && is_ignored(&path, g_ignore)
                    {
                        continue;
                    }

                    watch.files.insert(path);
                }

                trace!("Event for '{}'", watch.name);

                continue;
            }

            for watcher in self.watch_map.values_mut() {
                let now = Instant::now();

                if let Some(last) = watcher.last_send
                    && now >= last + Duration::from_millis(watcher.debounce)
                {
                    info!("Command ran for '{}'", watcher.name);

                    if let Err(e) = run_command(watcher) {
                        error!("{}", e);
                    }

                    watcher.last_send = None;
                    watcher.files.clear();
                }
            }
        }
    }

    fn start_one(&mut self, item: WatchItem) -> Result<RecommendedWatcher> {
        trace!("Starting '{}'", item.name);

        let (paths, git_ignore) = item.get_all_paths()?;

        trace!(
            "Paths - {:?}, gitignore: {}",
            paths,
            git_ignore
                .as_ref()
                .map(|g| g.num_ignores())
                .unwrap_or_default()
        );

        let watcher = WatchCommandRunner::new(&item, git_ignore);
        let id = Uuid::new_v4();

        let handler = WatchEventHandler::new(id, item.events, self.sx.clone());

        self.watch_map.insert(id, watcher);

        let mut notifier = notify::recommended_watcher(handler)?;

        for path in paths {
            notifier.watch(&path, RecursiveMode::NonRecursive)?;
        }

        Ok(notifier)
    }
}

impl Default for WatchFiles {
    fn default() -> Self {
        let (sx, rx) = mpsc::channel();

        Self {
            watch_map: HashMap::new(),
            sx,
            rx,
        }
    }
}

#[derive(Default)]
struct WatchCommandRunner {
    name: String,
    cmd: String,
    files: HashSet<PathBuf>,
    git_ignore: Option<Gitignore>,
    debounce: u64,
    last_send: Option<Instant>,
}

impl WatchCommandRunner {
    fn new(item: &WatchItem, git_ignore: Option<Gitignore>) -> Self {
        Self {
            name: item.name.clone(),
            cmd: item.run.to_string(),
            debounce: item.debounce,
            git_ignore,
            ..Default::default()
        }
    }
}

struct WatchEventHandler {
    id: Uuid,
    pub flags: EventFlags,
    pub sx: Sender<WatchEvent>,
}

impl WatchEventHandler {
    fn new(id: Uuid, flags: EventFlags, sx: Sender<WatchEvent>) -> Self {
        Self { id, flags, sx }
    }
}

impl EventHandler for WatchEventHandler {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        match event {
            Ok(Event { kind, paths, .. }) => {
                let flag = EventFlags::from(kind);

                if self.flags.intersects(flag) && !paths.is_empty() {
                    self.sx.send(WatchEvent { id: self.id, paths }).unwrap();
                }
            }
            Err(e) => error!("{}", e),
        }
    }
}

fn run_command(watcher: &WatchCommandRunner) -> Result<()> {
    let files = watcher
        .files
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    let output = Command::new("sh")
        .args(["-c", &watcher.cmd])
        .env("PATHS", &files)
        .output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;

    trace!(
        "\nNAME: '{}'\nPATHS: {}\nSTDOUT:\n{}\nSTDERR:\n{}",
        &watcher.name, files, stdout, stderr
    );

    Ok(())
}
