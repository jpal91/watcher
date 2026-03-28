use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

use anyhow::Result;
use glob::glob;
use log::{error, info, trace};
use notify::{Event, EventHandler, RecommendedWatcher, RecursiveMode, Watcher};
use uuid::Uuid;

use crate::config::{EventFlags, WatchCommands, WatchItem, YamlChoice};

pub struct WatchFiles {
    watch_map: HashMap<Uuid, ActiveWatcher>,
    sx: Sender<Uuid>,
    rx: Receiver<Uuid>,
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
            if let Ok(id) = self.rx.recv_timeout(Duration::from_secs(1)) {
                // Definitely exists
                let watch = self.watch_map.get_mut(&id).unwrap();
                let now = Instant::now();

                watch.last_send = Some(now);

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
                }
            }
        }
    }

    fn start_one(&mut self, item: WatchItem) -> Result<RecommendedWatcher> {
        trace!("Starting '{}'", item.name);

        let paths = get_all_paths(&item)?;
        let watcher = ActiveWatcher::new(&item);
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

pub struct ActiveWatcher {
    pub name: String,
    pub cmd: String,
    last_send: Option<Instant>,
    debounce: u64,
}

impl ActiveWatcher {
    fn new(item: &WatchItem) -> Self {
        let cmd = item.run.to_string();
        let name = item.name.clone();

        Self {
            cmd,
            name,
            last_send: None,
            debounce: item.debounce,
        }
    }
}

pub struct WatchEventHandler {
    pub id: Uuid,
    pub flags: EventFlags,
    pub sx: Sender<Uuid>,
}

impl WatchEventHandler {
    pub fn new(id: Uuid, flags: EventFlags, sx: Sender<Uuid>) -> Self {
        Self { id, flags, sx }
    }
}

impl EventHandler for WatchEventHandler {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        match event {
            Ok(Event { kind, .. }) => {
                let flag = EventFlags::from(kind);

                if self.flags.intersects(flag) {
                    self.sx.send(self.id).unwrap();
                }
            }
            Err(e) => error!("{}", e),
        }
    }
}

fn get_all_paths(item: &WatchItem) -> Result<Vec<PathBuf>> {
    let mut paths = HashSet::new();

    match &item.watch {
        YamlChoice::Single(s) => get_single_path(s, &mut paths)?,
        YamlChoice::Arr(arr) => get_multi_paths(arr, &mut paths)?,
    }

    if let Some(ign) = &item.ignore {
        let mut ignored = HashSet::new();

        match ign {
            YamlChoice::Single(s) => get_single_path(s, &mut ignored)?,
            YamlChoice::Arr(arr) => get_multi_paths(arr, &mut ignored)?,
        }

        Ok(paths.difference(&ignored).cloned().collect())
    } else {
        Ok(paths.into_iter().collect())
    }
}

fn get_multi_paths(items: &[String], paths: &mut HashSet<PathBuf>) -> Result<()> {
    for glob_path in items.iter() {
        get_single_path(glob_path, paths)?;
    }

    Ok(())
}

fn get_single_path(item: &str, paths: &mut HashSet<PathBuf>) -> Result<()> {
    for path in glob(item)? {
        let path = path?;
        paths.insert(path);
    }

    Ok(())
}

fn run_command(watcher: &ActiveWatcher) -> Result<()> {
    let output = Command::new("sh").args(["-c", &watcher.cmd]).output()?;

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;

    trace!(
        "\nNAME: '{}'\nSTDOUT:\n{}\nSTDERR:\n{}",
        &watcher.name, stdout, stderr
    );

    Ok(())
}
