use std::{
    collections::HashMap,
    error::Error,
    fs::{create_dir_all, File},
    io,
    path::{Path, PathBuf},
};

use procfs::{kernel_config, ConfigSetting, CpuInfo, Meminfo};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

use crate::Experiment;
use crate::ExperimentConfiguration;

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    Other(#[from] Box<dyn Error>),
}

pub struct RunConfig {
    pub results_dir: PathBuf,
}

pub async fn run<E: Experiment>(experiment: &mut E, config: &RunConfig) -> Result<(), RunError> {
    let exp_path = create_experiment_dir(&config.results_dir)?;
    debug!(dir=%exp_path.display(), "Running experiment");

    run_single(experiment, &exp_path).await?;
    Ok(())
}

async fn run_single<E: Experiment>(
    experiment: &mut E,
    experiment_dir: &Path,
) -> Result<(), RunError> {
    collect_environment_data(experiment_dir);

    let configurations = experiment.configurations();
    let total_configurations = configurations.len();

    // for each configuration, build the directories they would make
    // if the directories exist then skip this dir
    let mut configurations_to_run = Vec::new();
    for configuration in configurations {
        let config_path = build_config_dir(experiment_dir, &configuration)?;
        if config_path.exists() {
            debug!(?config_path, "Config directory exists, skipping config");
        }
        configurations_to_run.push(configuration);
    }

    let skipped_configs = total_configurations - configurations_to_run.len();
    debug!(
        pre_completed = skipped_configs,
        remaining = configurations_to_run.len(),
        "Finished skipping pre-completed configurations, running remaining"
    );

    for (i, config) in configurations_to_run.iter().enumerate() {
        let config_dir = create_config_dir(experiment_dir, config)?;
        let mut config_file = File::create(&config_dir.join("configuration.json"))?;
        config.ser_pretty(&mut config_file)?;
        experiment.pre_run(config).await;
        debug!(
            "Running configuration {}/{}",
            i + 1,
            configurations_to_run.len(),
        );
        experiment.run(config, config_dir).await;
        experiment.post_run(config).await;
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
    cpu_model_name: String,
    cpu_vendor_id: String,
    cpu_cores: usize,
    mem_info: Meminfo,
    kernel_config: HashMap<String, ConfigSetting>,
}

fn collect_environment_data(path: &Path) {
    let utsname = nix::sys::utsname::uname().unwrap();
    let cpuinfo = CpuInfo::new().unwrap();
    let meminfo = Meminfo::new().unwrap();
    let env = Environment {
        hostname: utsname.nodename().to_string_lossy().to_string(),
        os: utsname.sysname().to_string_lossy().to_string(),
        release: utsname.release().to_string_lossy().to_string(),
        version: utsname.version().to_string_lossy().to_string(),
        architecture: utsname.machine().to_string_lossy().to_string(),
        cpu_model_name: cpuinfo.model_name(0).unwrap().to_owned(),
        cpu_vendor_id: cpuinfo.vendor_id(0).unwrap().to_owned(),
        cpu_cores: cpuinfo.num_cores(),
        mem_info: meminfo,
        kernel_config: kernel_config().unwrap_or_default(),
    };
    let env_file = File::create(path.join("environment.json")).unwrap();
    serde_json::to_writer_pretty(env_file, &env).unwrap();
}

fn create_experiment_dir(results_dir: &Path) -> Result<PathBuf, io::Error> {
    let exp_path = results_dir.to_owned();
    debug!(path = ?exp_path, "Creating experiments directory");
    create_dir_all(&exp_path)?;
    Ok(exp_path)
}

fn build_config_dir<C: ExperimentConfiguration>(
    parent: &Path,
    configuration: &C,
) -> Result<PathBuf, Box<dyn Error>> {
    let config_hash = configuration.hash()?;
    let config_path = parent.join(config_hash);
    Ok(config_path)
}

fn create_config_dir<C: ExperimentConfiguration>(
    parent: &Path,
    configuration: &C,
) -> Result<PathBuf, RunError> {
    let config_path = build_config_dir(parent, configuration)?;
    debug!(path = ?config_path, "Checking for config directory");
    create_dir_all(&config_path)?;
    Ok(config_path)
}
