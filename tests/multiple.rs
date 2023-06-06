use std::{path::Path, path::PathBuf, time::Duration};

use async_trait::async_trait;
use exp::{
    docker_runner::ContainerConfig, Environment, ExpResult, Experiment, ExperimentConfiguration,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
struct ExpAConfig {}

impl ExperimentConfiguration for ExpAConfig {}

struct ExpA {
    configurations: Vec<ExpAConfig>,
}

#[async_trait]
impl Experiment for ExpA {
    type Configuration = ExpAConfig;

    fn configurations(&mut self) -> Vec<Self::Configuration> {
        self.configurations.clone()
    }
    async fn pre_run(&mut self, _: &Self::Configuration) -> ExpResult<()> {
        println!("prerun a");
        Ok(())
    }
    async fn run(&mut self, _: &Self::Configuration, conf_dir: &Path) -> ExpResult<()> {
        println!("run a {:?}", conf_dir);

        let mut runner = exp::docker_runner::Runner::new(conf_dir.to_path_buf()).await;

        runner
            .add_container(&ContainerConfig {
                name: "exp-test-1".to_owned(),
                image_name: "nginx".to_owned(),
                image_tag: "alpine".to_owned(),
                network: Some("exp-test-net".to_owned()),
                network_subnet: None,
                command: None,
                ports: Some(vec![("90".to_owned(), "80".to_owned())]),
                capabilities: None,
                cpus: None,
                memory: None,
                pull: true,
                tmpfs: Vec::new(),
                volumes: Vec::new(),
            })
            .await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        runner.finish().await;
        Ok(())
    }
    async fn post_run(&mut self, _: &Self::Configuration) -> ExpResult<()> {
        println!("postrun a");
        Ok(())
    }

    fn analyse(
        &mut self,
        _exp_dir: &Path,
        _environment: Environment,
        _configurations: Vec<(Self::Configuration, PathBuf)>,
    ) {
    }
}

#[tokio::test]
async fn multiple() {
    let mut exp = ExpA {
        configurations: vec![ExpAConfig {}],
    };
    let results_dir = PathBuf::from("results/multiple");
    let run_config = exp::RunConfig {
        results_dir: results_dir.clone(),
    };
    exp::run(&mut exp, &run_config).await.unwrap();
    let analyse_config = exp::AnalyseConfig { results_dir };
    exp::analyse(&mut exp, &analyse_config).await.unwrap();
}
