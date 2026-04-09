use std::{
    collections::HashSet,
    fmt::Display,
    path::{Component, Path, PathBuf},
};

use anyhow::Result;
use bitflags::bitflags;
use glob::glob;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::EventKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum YamlChoice {
    Single(String),
    Arr(Vec<String>),
}

impl Display for YamlChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YamlChoice::Single(s) => write!(f, "{s}"),
            YamlChoice::Arr(arr) => {
                for line in arr.iter() {
                    writeln!(f, "{}", line)?;
                }

                Ok(())
            }
        }
    }
}

pub type WatchCommands = Vec<WatchItem>;

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchItem {
    pub name: String,
    pub watch: YamlChoice,
    pub run: YamlChoice,
    pub ignore: Option<YamlChoice>,
    pub base_path: Option<PathBuf>,
    #[serde(default)]
    pub events: EventFlags,
    #[serde(
        default = "default_debounce",
        deserialize_with = "deserialize_debounce"
    )]
    pub debounce: u64,
}

fn default_debounce() -> u64 {
    100
}

fn deserialize_debounce<'de, D>(de: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let out = f64::deserialize(de).unwrap_or(100.0);
    Ok((1000.0 * out) as u64)
}

impl WatchItem {
    pub fn get_watch_paths(&self) -> Vec<PathBuf> {
        self.get_full_paths(&self.watch)
    }

    pub fn get_ignore_paths(&self) -> Option<Vec<PathBuf>> {
        self.ignore.as_ref().map(|ign| self.get_full_paths(ign))
    }

    fn get_full_paths(&self, paths: &YamlChoice) -> Vec<PathBuf> {
        let base = match self.base_path {
            Some(ref base) => base,
            _ => &std::env::current_dir().unwrap(),
        };

        match paths {
            YamlChoice::Single(glob) => vec![build_full_path(base, glob)],
            YamlChoice::Arr(arr) => arr.iter().map(|glob| build_full_path(base, glob)).collect(),
        }
    }

    pub fn get_all_paths(&self) -> Result<(Vec<PathBuf>, Option<Gitignore>)> {
        let mut paths: HashSet<PathBuf> = HashSet::new();
        let mut git_ignore: Option<GitignoreBuilder> = None;

        let watch_paths = self.get_watch_paths();

        for path in watch_paths {
            get_globbed_path(path.to_string_lossy(), &mut paths, &mut git_ignore)?;
        }

        if let Some(ign) = self.get_ignore_paths() {
            let mut ignored = HashSet::new();

            for path in ign {
                get_globbed_path(path.to_string_lossy(), &mut ignored, &mut git_ignore)?;
            }

            paths = paths.difference(&ignored).cloned().collect();
        }

        if let Some(ignore) = git_ignore
            && let Ok(ignore) = ignore.build()
        {
            let filtered_paths = paths
                .into_iter()
                .filter(|path| !is_ignored(path, &ignore))
                .collect();
            Ok((filtered_paths, Some(ignore)))
        } else {
            Ok((paths.into_iter().collect(), None))
        }
    }
}

fn build_full_path(base: &Path, glob: &str) -> PathBuf {
    let glob = PathBuf::from(glob);
    let mut components = glob.components().peekable();

    while let Some(cmp) = components.peek() {
        match cmp {
            Component::RootDir | Component::Prefix(_) => {
                let _ = components.next();
            }
            _ => break,
        }
    }

    base.join(components.collect::<PathBuf>())
}

fn get_globbed_path<S: AsRef<str>>(
    item: S,
    paths: &mut HashSet<PathBuf>,
    ignored: &mut Option<GitignoreBuilder>,
) -> Result<()> {
    for path in glob(item.as_ref())? {
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
        } else if let Ok(p) = path.canonicalize() {
            paths.insert(p);
        }
    }

    Ok(())
}

#[inline]
pub fn is_ignored<P: AsRef<Path>>(path: P, g_ignore: &Gitignore) -> bool {
    let path = path.as_ref();
    g_ignore.matched(path, path.is_dir()).is_ignore()
}

bitflags! {
    #[derive(Serialize, Deserialize, Debug)]
    pub struct EventFlags: u32 {
        const CREATE = 1;
        const ACCESS = 2;
        const MODIFY = 4;
        const REMOVE = 8;
        const ALL = Self::CREATE.bits() | Self::ACCESS.bits() | Self::MODIFY.bits() | Self::REMOVE.bits();
    }
}

impl Default for EventFlags {
    fn default() -> Self {
        EventFlags::MODIFY
    }
}

impl From<EventKind> for EventFlags {
    fn from(value: EventKind) -> Self {
        match value {
            EventKind::Access(_) => EventFlags::ACCESS,
            EventKind::Modify(_) => EventFlags::MODIFY,
            EventKind::Create(_) => EventFlags::CREATE,
            EventKind::Remove(_) => EventFlags::REMOVE,
            _ => EventFlags::ALL,
        }
    }
}
