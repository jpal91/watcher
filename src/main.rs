#![allow(unused)]
use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow};
use flexi_logger::{
    Cleanup, Criterion, FileSpec, Logger, LoggerHandle, Naming, filter::LogLineFilter,
};
use log::{info, warn};
use watcher_lib::{
    config::{EventFlags, WatchCommands, WatchItem, YamlChoice},
    watcher::WatchFiles,
};

fn main() -> Result<()> {
    let watcher_file_path = watcher_file()?;
    let _logger = logging_setup()?;

    let content = fs::read_to_string(watcher_file_path)?;
    let items: WatchCommands = serde_yaml::from_str(&content)?;

    if items.is_empty() {
        warn!("Watcher configuration is empty, no actions will be performed");
        return Ok(());
    }

    info!("Starting watcher");
    WatchFiles::default().start(items)?;

    Ok(())
}

fn watcher_file() -> Result<PathBuf> {
    let config_dir = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    } else {
        dirs::config_local_dir()
            .map(|dir| dir.join("watcher"))
            .ok_or_else(|| anyhow!("Could not find config directory"))?
    };

    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }

    let watcher_file = config_dir.join("watcher_config.yml");

    if !watcher_file.exists() {
        fs::write(&watcher_file, "")?;
    }

    Ok(watcher_file)
}

struct LogFilter;

impl LogLineFilter for LogFilter {
    fn write(
        &self,
        now: &mut flexi_logger::DeferredNow,
        record: &log::Record,
        log_line_writer: &dyn flexi_logger::filter::LogLineWriter,
    ) -> std::io::Result<()> {
        if let Some(path) = record.module_path()
            && path.starts_with("watcher")
        {
            log_line_writer.write(now, record)?;
        }

        Ok(())
    }
}

fn logging_setup() -> Result<LoggerHandle> {
    let data_dir = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    } else {
        dirs::data_local_dir()
            .map(|dir| dir.join("watcher"))
            .ok_or_else(|| anyhow!("Could not find local data dir"))?
    };

    if !data_dir.exists() {
        fs::create_dir_all(&data_dir)?;
    }

    let mut logger = Logger::try_with_env_or_str("trace")?
        .log_to_file(
            FileSpec::default()
                .directory(data_dir)
                .basename("watcher_log")
                .suffix("log"),
        )
        .filter(Box::new(LogFilter))
        .rotate(
            Criterion::Size(1_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(1),
        )
        .format_for_files(flexi_logger::detailed_format)
        .append();

    if cfg!(debug_assertions) {
        logger = logger
            .duplicate_to_stderr(flexi_logger::Duplicate::Trace)
            .adaptive_format_for_stderr(flexi_logger::AdaptiveFormat::Detailed);
    }

    Ok(logger.start()?)
}
