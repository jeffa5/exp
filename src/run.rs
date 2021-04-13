use std::{
    fs::{create_dir_all, File},
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

use crate::{Experiment, ExperimentConfiguration};

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

pub async fn run<E: Experiment>(experiments: &[E], config: &RunConfig) -> Result<(), RunError> {
    let exp_path = create_experiments_dir(&config.output_dir)?;
    println!(
        "Running {} experiments with experiment dir {}",
        experiments.len(),
        exp_path.display()
    );

    for e in experiments {
        run_single(e, &exp_path).await?
    }
    Ok(())
}

async fn run_single<E: Experiment>(experiment: &E, dir: &Path) -> Result<(), RunError> {
    let experiment_dir = create_experiment_dir(dir, experiment.name())?;
    collect_environment_data(&experiment_dir);

    let configurations = experiment.configurations();
    let width = configurations.len().to_string().len();
    for (i, config) in configurations.iter().enumerate() {
        let config_dir = create_config_dir(&experiment_dir, i, width)?;
        let config_file = File::create(&config_dir.join("configuration.json"))?;
        serde_json::to_writer_pretty(config_file, &config)?;
        experiment.pre_run(&config).await;
        let repeats = config.repeats();
        let width = repeats.to_string().len();
        for i in 0..repeats {
            let repeat_dir = create_repeat_dir(&config_dir, i as usize, width)?;
            experiment.run(config, repeat_dir).await;
        }
        experiment.post_run(&config).await;
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Environment {
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
    serde_json::to_writer_pretty(env_file, &env).unwrap();
}

fn create_experiments_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let exp_path = parent
        .join("experiments")
        .join(chrono::Utc::now().to_rfc3339());
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
