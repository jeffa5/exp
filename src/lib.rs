use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

mod analyse;
pub mod docker_runner;
mod run;

pub use run::{run, RunConfig};

pub trait ExperimentConfiguration<'a>: Serialize + Deserialize<'a> {
    fn repeats(&self) -> u32;
}

pub trait Experiment<'a>: RunnableExperiment<'a> + AnalysableExperiment {}

impl<'a, T: RunnableExperiment<'a> + AnalysableExperiment> Experiment<'a> for T {}

#[async_trait]
pub trait RunnableExperiment<'a>: NamedExperiment {
    type RunConfiguration: ExperimentConfiguration<'a>;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration>;

    async fn pre_run(&self, configuration: &Self::RunConfiguration);
    async fn run(&self, configuration: &Self::RunConfiguration, repeat_dir: PathBuf);
    async fn post_run(&self, configuration: &Self::RunConfiguration);
}

pub trait AnalysableExperiment: NamedExperiment {
    fn pre_analyse(&self);
    fn analyse(&self);
    fn post_analyse(&self);
}

pub trait NamedExperiment {
    fn name(&self) -> &str;
}
