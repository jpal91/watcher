use std::fmt::Display;

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
}

pub type WatchCommands = Vec<WatchItem>;
