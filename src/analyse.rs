use std::{
    fs::File,
    path::{Path, PathBuf},
};

use thiserror::Error;
use tracing::{warn, debug};

use crate::Experiment;

pub struct AnalyseConfig {
    pub results_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum AnalyseError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

pub async fn analyse<E: Experiment>(
    experiment: &mut E,
    config: &AnalyseConfig,
) -> Result<(), AnalyseError> {
    analyse_single(experiment, &config.results_dir).await?;
    Ok(())
}

async fn analyse_single<E: Experiment>(experiment: &mut E, dir: &Path) -> Result<(), AnalyseError> {
    if !dir.exists() {
        warn!("No directory for experiment exists");
        return Ok(());
    }
    let env_file = File::open(dir.join("environment.json"))?;
    let env = serde_json::from_reader(env_file)?;
    let mut configuration_dirs = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            configuration_dirs.push(path)
        }
    }
    configuration_dirs.sort();
    let mut configurations = Vec::new();
    for c in configuration_dirs {
        let config_file_path = c.join("configuration.json");
        debug!(?config_file_path, "Reading configuration");
        let config_file = File::open(config_file_path)?;
        let config: E::Configuration = serde_json::from_reader(config_file)?;
        configurations.push((config, c));
    }
    experiment.analyse(dir, env, configurations);
    Ok(())
}
