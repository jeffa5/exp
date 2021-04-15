use std::path::PathBuf;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

mod analyse;
pub mod docker_runner;
mod run;

pub use analyse::{analyse, repeat_dirs, AnalyseConfig, AnalyseError};
pub use run::{run, Environment, RunConfig, RunError};

pub trait ExperimentConfiguration: Serialize + DeserializeOwned {
    fn repeats(&self) -> u32;
    fn description(&self) -> &str;
}

#[async_trait]
pub trait Experiment {
    type Configuration: ExperimentConfiguration;

    fn configurations(&self) -> Vec<Self::Configuration>;
    fn name(&self) -> &str;

    async fn pre_run(&self, configuration: &Self::Configuration);
    async fn run(&self, configuration: &Self::Configuration, repeat_dir: PathBuf);
    async fn post_run(&self, configuration: &Self::Configuration);

    fn analyse(
        &self,
        experiment_dir: PathBuf,
        date: chrono::DateTime<chrono::Utc>,
        environment: Environment,
        configurations: Vec<(Self::Configuration, PathBuf)>,
    );
}
