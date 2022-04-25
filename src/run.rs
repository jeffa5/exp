use std::{
    collections::HashMap,
    fs::{create_dir_all, File},
    io,
    path::{Path, PathBuf},
};

use procfs::{kernel_config, ConfigSetting, CpuInfo, Meminfo};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

use crate::{Experiment, ExperimentConfiguration};

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

pub struct RunConfig {
    pub results_dir: PathBuf,
}

pub async fn run<E: Experiment>(experiment: &E, config: &RunConfig) -> Result<(), RunError> {
    let exp_path = create_experiment_dir(&config.results_dir)?;
    println!("Running experiment with dir {}", exp_path.display());

    run_single(experiment, &exp_path).await?;
    Ok(())
}

async fn run_single<E: Experiment>(experiment: &E, experiment_dir: &Path) -> Result<(), RunError> {
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
        for j in 0..repeats {
            println!(
                "Running configuration {}/{}, repeat {}/{}",
                i + 1,
                configurations.len(),
                j + 1,
                repeats
            );
            let repeat_dir = create_repeat_dir(&config_dir, j as usize, width)?;
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
    cpu_model_name: String,
    cpu_vendor_id: String,
    cpu_cores: usize,
    mem_info: Meminfo,
    kernel_config: HashMap<String, ConfigSetting>,
}

fn collect_environment_data(path: &Path) {
    let utsname = nix::sys::utsname::uname();
    let cpuinfo = CpuInfo::new().unwrap();
    let meminfo = Meminfo::new().unwrap();
    let env = Environment {
        hostname: utsname.nodename().to_owned(),
        os: utsname.sysname().to_owned(),
        release: utsname.release().to_owned(),
        version: utsname.version().to_owned(),
        architecture: utsname.machine().to_owned(),
        cpu_model_name: cpuinfo.model_name(0).unwrap().to_owned(),
        cpu_vendor_id: cpuinfo.vendor_id(0).unwrap().to_owned(),
        cpu_cores: cpuinfo.num_cores(),
        mem_info: meminfo,
        kernel_config: kernel_config().unwrap(),
    };
    let env_file = File::create(path.join("environment.json")).unwrap();
    serde_json::to_writer_pretty(env_file, &env).unwrap();
}

fn create_experiment_dir(results_dir: &Path) -> Result<PathBuf, io::Error> {
    let exp_path = results_dir.join(chrono::Utc::now().to_rfc3339());
    debug!(path = ?exp_path, "Creating experiments directory");
    create_dir_all(&exp_path)?;
    Ok(exp_path)
}

fn create_config_dir(parent: &Path, i: usize, width: usize) -> Result<PathBuf, io::Error> {
    let config_path = parent.join(format!("configuration-{:0>width$}", i + 1, width = width));
    debug!(path = ?config_path, "Creating config directory");
    create_dir_all(&config_path)?;
    Ok(config_path)
}

fn create_repeat_dir(parent: &Path, i: usize, width: usize) -> Result<PathBuf, io::Error> {
    let repeat_path = parent.join(format!("repeat-{:0>width$}", i + 1, width = width));
    debug!(path = ?repeat_path, "Creating repeat directory");
    create_dir_all(&repeat_path)?;
    Ok(repeat_path)
}
