use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

use anyhow::Result;
use glob::glob;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use log::{error, info, trace};
use notify::{Event, EventHandler, RecommendedWatcher, RecursiveMode, Watcher};
use uuid::Uuid;

use crate::config::{EventFlags, WatchCommands, WatchItem, YamlChoice};

type WatchEvent = (Uuid, Vec<PathBuf>);

pub struct WatchFiles {
    watch_map: HashMap<Uuid, ActiveWatcher>,
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
            if let Ok((id, paths)) = self.rx.recv_timeout(Duration::from_secs(1)) {
                // Definitely exists
                let watch = self.watch_map.get_mut(&id).unwrap();
                let now = Instant::now();

                watch.last_send = Some(now);
                watch.files.extend(paths);

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

        if let Some(ref base) = item.base_path {
            std::env::set_current_dir(base)?;
        }

        let (paths, git_ignore) = get_all_paths(&item)?;
        let watcher = ActiveWatcher::new(&item);
        let id = Uuid::new_v4();

        trace!(
            "Paths - {:?}, gitignore: {}",
            paths,
            git_ignore
                .as_ref()
                .map(|g| g.num_ignores())
                .unwrap_or_default()
        );

        let handler = WatchEventHandler::new(id, item.events, self.sx.clone(), git_ignore);

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
    pub files: HashSet<PathBuf>,
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
            files: HashSet::new(),
            last_send: None,
            debounce: item.debounce,
        }
    }
}

pub struct WatchEventHandler {
    pub id: Uuid,
    pub flags: EventFlags,
    pub sx: Sender<WatchEvent>,
    pub git_ignore: Option<Gitignore>,
}

impl WatchEventHandler {
    pub fn new(
        id: Uuid,
        flags: EventFlags,
        sx: Sender<WatchEvent>,
        git_ignore: Option<Gitignore>,
    ) -> Self {
        Self {
            id,
            flags,
            sx,
            git_ignore,
        }
    }
}

impl EventHandler for WatchEventHandler {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        match event {
            Ok(Event { kind, paths, .. }) => {
                let flag = EventFlags::from(kind);

                let paths = if let Some(ref ignore) = self.git_ignore {
                    paths
                        .into_iter()
                        .filter(|p| !ignore.matched(p, p.is_dir()).is_ignore())
                        .collect()
                } else {
                    paths
                };

                if self.flags.intersects(flag) && !paths.is_empty() {
                    self.sx.send((self.id, paths)).unwrap();
                }
            }
            Err(e) => error!("{}", e),
        }
    }
}

fn get_all_paths(item: &WatchItem) -> Result<(Vec<PathBuf>, Option<Gitignore>)> {
    let mut paths = HashSet::new();
    let mut git_ignore: Option<GitignoreBuilder> = None;

    match &item.watch {
        YamlChoice::Single(s) => get_single_path(s, &mut paths, &mut git_ignore)?,
        YamlChoice::Arr(arr) => get_multi_paths(arr, &mut paths, &mut git_ignore)?,
    }

    let paths: Vec<PathBuf> = if let Some(ign) = &item.ignore {
        let mut ignored = HashSet::new();

        match ign {
            YamlChoice::Single(s) => get_single_path(s, &mut ignored, &mut git_ignore)?,
            YamlChoice::Arr(arr) => get_multi_paths(arr, &mut ignored, &mut git_ignore)?,
        }

        paths.difference(&ignored).cloned().collect()
    } else {
        paths.into_iter().collect()
    };

    if let Some(ignore) = git_ignore
        && let Ok(ignore) = ignore.build()
    {
        let filtered_paths = paths
            .into_iter()
            .filter(|path| !ignore.matched(path, path.is_dir()).is_ignore())
            .collect();
        Ok((filtered_paths, Some(ignore)))
    } else {
        Ok((paths, None))
    }
}

fn get_multi_paths(
    items: &[String],
    paths: &mut HashSet<PathBuf>,
    ignored: &mut Option<GitignoreBuilder>,
) -> Result<()> {
    for glob_path in items.iter() {
        get_single_path(glob_path, paths, ignored)?;
    }

    Ok(())
}

fn get_single_path(
    item: &str,
    paths: &mut HashSet<PathBuf>,
    ignored: &mut Option<GitignoreBuilder>,
) -> Result<()> {
    for path in glob(item)? {
        let path = path?;

        if let Some(f_name) = path.file_name().map(|s| s.to_string_lossy())
            && f_name == ".gitignore"
        {
            let abs_path = path.canonicalize().unwrap();

            let ignore =
                ignored.get_or_insert_with(|| GitignoreBuilder::new(path.parent().unwrap()));

            ignore.add(&abs_path);
        } else if path.to_string_lossy().contains(".git") {
            continue;
        } else {
            paths.insert(path);
        }
    }

    Ok(())
}

fn run_command(watcher: &ActiveWatcher) -> Result<()> {
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
