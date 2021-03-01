use std::{
    fs::{create_dir_all, File},
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

use crate::{ExperimentConfiguration, RunnableExperiment};

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

pub struct RunConfig {
    pub output_dir: PathBuf,
}

pub async fn run<'a, E: RunnableExperiment<'a>>(
    experiments: &[E],
    config: &RunConfig,
) -> Result<(), RunError> {
    let exp_path = create_experiments_dir(&config.output_dir)?;
    for e in experiments {
        run_single(e, &exp_path).await?
    }
    Ok(())
}

async fn run_single<'a, E: RunnableExperiment<'a>>(
    experiment: &E,
    dir: &Path,
) -> Result<(), RunError> {
    let experiment_dir = create_experiment_dir(dir, experiment.name())?;
    collect_environment_data(&experiment_dir);

    let configurations = experiment.run_configurations();
    let width = configurations.len().to_string().len();
    for (i, config) in configurations.iter().enumerate() {
        let config_dir = create_config_dir(&experiment_dir, i, width)?;
        let config_file = File::create(&config_dir.join("configuration.json"))?;
        serde_json::to_writer(config_file, &config)?;
        experiment.pre_run(&config).await;
        let repeats = config.repeats();
        let width = repeats.to_string().len();
        for i in 0..repeats {
            let repeat_dir = create_repeat_dir(&config_dir, i as usize, width)?;
            let logs_dir = create_logs_dir(&repeat_dir)?;
            let metrics_dir = create_metrics_dir(&repeat_dir)?;
            let data_dir = create_data_dir(&repeat_dir)?;
            experiment.run(&config, repeat_dir).await;
        }
        experiment.post_run(&config).await;
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Environment {
    hostname: String,
    os: String,
    release: String,
    version: String,
    architecture: String,
}

fn collect_environment_data(path: &Path) {
    let utsname = nix::sys::utsname::uname();
    let env = Environment {
        hostname: utsname.nodename().to_owned(),
        os: utsname.sysname().to_owned(),
        release: utsname.release().to_owned(),
        version: utsname.version().to_owned(),
        architecture: utsname.machine().to_owned(),
    };
    let env_file = File::create(path.join("environment.json")).unwrap();
    serde_json::to_writer(env_file, &env).unwrap();
}

fn create_experiments_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let exp_path = parent
        .join("experiments")
        .join(chrono::Local::now().to_rfc3339());
    info!(path = ?exp_path, "Creating experiments directory");
    create_dir_all(&exp_path)?;
    Ok(exp_path)
}

fn create_experiment_dir(parent: &Path, name: &str) -> Result<PathBuf, io::Error> {
    let exp_path = parent.join(name);
    info!(path = ?exp_path, "Creating experiment directory");
    create_dir_all(&exp_path)?;
    Ok(exp_path)
}

fn create_config_dir(parent: &Path, i: usize, width: usize) -> Result<PathBuf, io::Error> {
    let config_path = parent.join(format!("configuration-{:0>width$}", i + 1, width = width));
    info!(path = ?config_path, "Creating config directory");
    create_dir_all(&config_path)?;
    Ok(config_path)
}

fn create_repeat_dir(parent: &Path, i: usize, width: usize) -> Result<PathBuf, io::Error> {
    let repeat_path = parent.join(format!("repeat-{:0>width$}", i + 1, width = width));
    info!(path = ?repeat_path, "Creating repeat directory");
    create_dir_all(&repeat_path)?;
    Ok(repeat_path)
}

fn create_logs_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let logs_path = parent.join("logs");
    info!(path = ?logs_path, "Creating logs directory");
    create_dir_all(&logs_path)?;
    Ok(logs_path)
}

fn create_metrics_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let metrics_path = parent.join("metrics");
    info!(path = ?metrics_path, "Creating metrics directory");
    create_dir_all(&metrics_path)?;
    Ok(metrics_path)
}

fn create_data_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let data_path = parent.join("data");
    info!(path = ?data_path, "Creating data directory");
    create_dir_all(&data_path)?;
    Ok(data_path)
}
