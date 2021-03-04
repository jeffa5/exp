use std::{
    fs::File,
    path::{Path, PathBuf},
};

use chrono::Local;
use thiserror::Error;
use tracing::{info, warn};

use crate::Experiment;

pub struct AnalyseConfig {
    pub output_dir: PathBuf,
    pub date: Option<chrono::DateTime<Local>>,
}

#[derive(Debug, Error)]
pub enum AnalyseError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

pub async fn analyse<E: Experiment>(
    experiments: &[E],
    config: &AnalyseConfig,
) -> Result<(), AnalyseError> {
    let mut experiments_dir = config.output_dir.join("experiments");
    let date = if let Some(date) = config.date {
        experiments_dir.push(date.to_rfc3339());
        date
    } else {
        let mut dates = std::fs::read_dir(&experiments_dir)?
            .filter_map(|d| {
                d.ok().and_then(|d| {
                    chrono::DateTime::parse_from_rfc3339(&d.file_name().to_string_lossy()).ok()
                })
            })
            .collect::<Vec<_>>();
        dates.sort();
        let d = dates.last().unwrap();
        experiments_dir.push(d.to_rfc3339());
        d.with_timezone(&Local)
    };
    info!("Using date: {}", date);
    for e in experiments {
        analyse_single(e, date, &experiments_dir.join(e.name())).await?
    }
    Ok(())
}

async fn analyse_single<E: Experiment>(
    experiment: &E,
    date: chrono::DateTime<Local>,
    dir: &Path,
) -> Result<(), AnalyseError> {
    if !dir.exists() {
        warn!("No directory for experiment '{}' exists", experiment.name());
        return Ok(());
    }
    let mut configuration_dirs = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("configuration-")
        {
            configuration_dirs.push(path)
        }
    }
    configuration_dirs.sort();
    let mut configurations = Vec::new();
    for c in configuration_dirs {
        let config_file = File::open(c.join("configuration.json")).unwrap();
        let config: E::Configuration = serde_json::from_reader(config_file).unwrap();
        configurations.push(config);
    }
    experiment.analyse(dir.to_path_buf(), date, &configurations);
    Ok(())
}
