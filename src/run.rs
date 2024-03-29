use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs::{create_dir_all, rename, File},
    io,
    path::{Path, PathBuf},
};

use procfs::{kernel_config, ConfigSetting, CpuInfo, Meminfo};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

use crate::ExpResult;
use crate::Experiment;
use crate::ExperimentConfiguration;

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    Other(#[from] Box<dyn Error + Send + Sync>),
}

pub struct RunConfig {
    pub results_dir: PathBuf,
}

pub async fn run<E: Experiment>(experiment: &mut E, config: &RunConfig) -> Result<(), RunError> {
    let exp_path = create_experiment_dir(&config.results_dir)?;
    info!(dir=%exp_path.display(), "Running experiment");

    run_single(experiment, &exp_path).await?;
    Ok(())
}

async fn run_single<E: Experiment>(
    experiment: &mut E,
    experiment_dir: &Path,
) -> Result<(), RunError> {
    collect_environment_data(experiment_dir);

    let configurations = experiment.configurations();

    // for each configuration, build the directories they would make
    // if the directories exist then skip this dir
    let mut seen_configuration_hashes = HashSet::new();
    let mut configurations_to_run = Vec::new();
    let mut duplicate_configurations = 0;
    let mut skipped_configurations = 0;
    for configuration in configurations {
        let config_hash = configuration.hash_serialized()?;
        if !seen_configuration_hashes.insert(config_hash) {
            duplicate_configurations += 1;
            continue;
        }
        let config_path = build_config_dir(experiment_dir, &configuration)?;
        if config_path.exists() {
            debug!(?config_path, "Config directory exists, skipping config");
            skipped_configurations += 1;
            continue;
        }
        configurations_to_run.push(configuration);
    }

    info!(
        skipped = skipped_configurations,
        duplicates = duplicate_configurations,
        remaining = configurations_to_run.len(),
        "Finished skipping pre-completed configurations, running remaining"
    );

    for (i, config) in configurations_to_run.iter().enumerate() {
        let config_dir = build_config_dir(experiment_dir, config)?;
        // set up dir for running in, in case of a failure
        let mut running_dir = config_dir.clone();
        running_dir.set_extension("running");

        debug!(path = ?running_dir, "Creating running dir");
        create_dir_all(&running_dir)?;

        info!(
            hash = %config.hash_serialized().unwrap(),
            "Running configuration {}/{}",
            i + 1,
            configurations_to_run.len(),
        );
        match run_configuration(&running_dir, experiment, config).await {
            Ok(()) => {
                // successfully run this experiment, move it to a finished dir
                rename(running_dir, config_dir)?;
            }
            Err(_) => {
                // unsuccessfully run this experiment, move it to an error dir
                let mut error_dir = config_dir.clone();
                error_dir.set_extension("failed");
                rename(running_dir, error_dir)?;
            }
        }
    }
    Ok(())
}

async fn run_configuration<E: Experiment>(
    dir: &Path,
    experiment: &mut E,
    config: &E::Configuration,
) -> ExpResult<()> {
    let mut config_file = File::create(dir.join("configuration.json"))?;
    config.ser_pretty(&mut config_file)?;
    experiment.pre_run(config).await?;
    experiment.run(config, dir).await?;
    experiment.post_run(config).await?;
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
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let config_hash = configuration.hash_serialized()?;
    let config_path = parent.join(config_hash);
    Ok(config_path)
}
