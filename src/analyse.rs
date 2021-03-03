use std::path::{Path, PathBuf};

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

pub async fn analyse<'a, E: Experiment<'a>>(
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

async fn analyse_single<'a, E: Experiment<'a>>(
    experiment: &E,
    date: chrono::DateTime<Local>,
    dir: &Path,
) -> Result<(), AnalyseError> {
    if !dir.exists() {
        warn!("No directory for experiment '{}' exists", experiment.name());
        return Ok(());
    }
    experiment.analyse(dir.to_path_buf(), date);
    Ok(())
}
