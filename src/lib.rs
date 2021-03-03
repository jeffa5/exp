use std::path::PathBuf;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

mod analyse;
pub mod docker_runner;
mod run;

pub use analyse::{analyse, AnalyseConfig, AnalyseError};
pub use run::{run, RunConfig, RunError};

pub trait ExperimentConfiguration: Serialize + DeserializeOwned {
    fn repeats(&self) -> u32;
}

#[async_trait]
pub trait Experiment {
    type RunConfiguration: ExperimentConfiguration;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration>;
    fn name(&self) -> &str;

    async fn pre_run(&self, configuration: &Self::RunConfiguration);
    async fn run(&self, configuration: &Self::RunConfiguration, repeat_dir: PathBuf);
    async fn post_run(&self, configuration: &Self::RunConfiguration);

    fn analyse(
        &self,
        experiment_dir: PathBuf,
        date: chrono::DateTime<chrono::Local>,
        configurations: &[Self::RunConfiguration],
    );
}
