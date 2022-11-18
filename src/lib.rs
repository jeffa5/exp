use std::path::PathBuf;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

mod analyse;
pub mod docker_runner;
mod run;

pub use analyse::{analyse, repeat_dirs, AnalyseConfig, AnalyseError};
pub use run::{run, Environment, RunConfig, RunError};

pub trait ExperimentConfiguration: Serialize + DeserializeOwned {}

#[async_trait]
pub trait Experiment {
    type Configuration: ExperimentConfiguration;

    fn configurations(&mut self) -> Vec<Self::Configuration>;

    async fn pre_run(&mut self, configuration: &Self::Configuration);
    async fn run(&mut self, configuration: &Self::Configuration, repeat_dir: PathBuf);
    async fn post_run(&mut self, configuration: &Self::Configuration);

    fn analyse(
        &mut self,
        experiment_dir: PathBuf,
        environment: Environment,
        configurations: Vec<(Self::Configuration, PathBuf)>,
    );
}
