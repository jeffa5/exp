use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod analyse;
mod run;

pub use run::{run, RunConfig};

pub trait ExperimentConfiguration<'a>: Serialize + Deserialize<'a> {
    fn repeats(&self) -> u32;
}

pub trait Experiment<'a>: RunnableExperiment<'a> + AnalysableExperiment {}

impl<'a, T: RunnableExperiment<'a> + AnalysableExperiment> Experiment<'a> for T {}

pub trait RunnableExperiment<'a>: NamedExperiment {
    type RunConfiguration: ExperimentConfiguration<'a>;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration>;

    fn pre_run(&self, configuration: &Self::RunConfiguration);
    fn run(&self, configuration: &Self::RunConfiguration, data_dir: PathBuf);
    fn post_run(&self, configuration: &Self::RunConfiguration);
}

pub trait AnalysableExperiment: NamedExperiment {
    fn pre_analyse(&self);
    fn analyse(&self);
    fn post_analyse(&self);
}

pub trait NamedExperiment {
    fn name(&self) -> &str;
}
