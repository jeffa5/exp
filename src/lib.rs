use std::path::Path;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::error::Error;

mod analyse;
pub mod docker_runner;
mod run;

pub use analyse::{analyse, repeat_dirs, AnalyseConfig, AnalyseError};
pub use run::{run, Environment, RunConfig, RunError};

pub type ExpResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub trait ExperimentConfiguration: Serialize + DeserializeOwned {
    /// Calculate the hash of the serialized version of this config.
    fn hash(&self) -> ExpResult<String> {
        let mut v = Vec::new();
        self.ser(&mut v)?;
        let config_hash = blake3::hash(&v).to_hex();
        Ok(config_hash.to_string())
    }

    fn ser<W: std::io::Write>(&self, w: W) -> ExpResult<()> {
        serde_json::to_writer(w, self)?;
        Ok(())
    }

    fn ser_pretty<W: std::io::Write>(&self, w: W) -> ExpResult<()> {
        serde_json::to_writer_pretty(w, self)?;
        Ok(())
    }

    fn deser<R: std::io::Read>(r: R) -> ExpResult<Self> {
        let conf = serde_json::from_reader(r)?;
        Ok(conf)
    }
}

#[async_trait]
pub trait Experiment {
    type Configuration: ExperimentConfiguration;

    fn configurations(&mut self) -> Vec<Self::Configuration>;

    async fn pre_run(&mut self, configuration: &Self::Configuration) -> ExpResult<()>;
    async fn run(
        &mut self,
        configuration: &Self::Configuration,
        repeat_dir: &Path,
    ) -> ExpResult<()>;
    async fn post_run(&mut self, configuration: &Self::Configuration) -> ExpResult<()>;

    fn analyse(
        &mut self,
        experiment_dir: &Path,
        environment: Environment,
        configurations: Vec<(Self::Configuration, PathBuf)>,
    );
}
