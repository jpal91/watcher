use std::fmt::Display;

use bitflags::bitflags;
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

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchItem {
    pub name: String,
    pub watch: YamlChoice,
    pub run: YamlChoice,
    pub ignore: Option<YamlChoice>,
    #[serde(default)]
    pub events: EventFlags,
    #[serde(
        default = "default_debounce",
        deserialize_with = "deserialize_debounce"
    )]
    pub debounce: u64,
}

pub type WatchCommands = Vec<WatchItem>;

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
