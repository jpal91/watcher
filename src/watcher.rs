use std::{collections::HashSet, path::PathBuf, process::Command, thread, time::Duration};

use anyhow::Result;
use glob::glob;
use notify::{Event, EventHandler, EventKind, RecursiveMode, Watcher};

use crate::config::{WatchCommands, WatchItem, YamlChoice};

pub struct WatchFiles;

impl WatchFiles {
    pub fn start(items: WatchCommands) -> Result<()> {
        let mut watchers = vec![];

        for item in items {
            let paths = match get_all_paths(&item) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{}", e);
                    continue;
                }
            };

            match ActiveWatcher::new(item) {
                Ok(w) => {
                    let mut watcher = match notify::recommended_watcher(w) {
                        Ok(w) => w,
                        Err(e) => {
                            eprintln!("{}", e);
                            continue;
                        }
                    };

                    for path in paths {
                        let _ = watcher.watch(&path, RecursiveMode::NonRecursive);
                    }

                    watchers.push(watcher);
                }
                Err(e) => eprintln!("{}", e),
            };
        }

        loop {
            println!("Tick");
            thread::sleep(Duration::from_secs(1));
        }
    }
}

pub struct ActiveWatcher {
    pub name: String,
    pub cmd: String,
}

impl ActiveWatcher {
    pub fn new(item: WatchItem) -> Result<Self> {
        let cmd = item.run.to_string();

        Ok(Self {
            name: item.name,
            cmd,
        })
    }
}

impl EventHandler for ActiveWatcher {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        match event {
            Ok(Event {
                kind: EventKind::Modify(_),
                ..
            }) => {
                let out = Command::new("sh").args(["-c", &self.cmd]).output();

                match out {
                    Ok(o) => {
                        let msg = match String::from_utf8(o.stdout) {
                            Ok(m) => m,
                            Err(e) => return eprintln!("{}", e),
                        };
                        println!("{msg}");
                    }
                    Err(e) => eprintln!("{}", e),
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("{}", e),
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
