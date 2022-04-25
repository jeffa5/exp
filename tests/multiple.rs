use std::{collections::HashMap, fs::read_dir, path::PathBuf, time::Duration};

use async_trait::async_trait;
use exp::{docker_runner::ContainerConfig, Environment, Experiment, ExperimentConfiguration};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct ExpAConfig {}

impl ExperimentConfiguration for ExpAConfig {
    fn repeats(&self) -> u32 {
        2
    }

    fn description(&self) -> &str {
        "exp a"
    }
}

struct ExpA {
    configurations: Vec<ExpAConfig>,
}

#[async_trait]
impl Experiment for ExpA {
    type Configuration = ExpAConfig;

    fn name(&self) -> &str {
        "a"
    }

    fn configurations(&self) -> Vec<Self::Configuration> {
        self.configurations.clone()
    }
    async fn pre_run(&self, _: &Self::Configuration) {
        println!("prerun a")
    }
    async fn run(&self, _: &Self::Configuration, repeat_dir: PathBuf) {
        println!("run a {:?}", repeat_dir);

        let mut runner = exp::docker_runner::Runner::new(repeat_dir).await;

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
                pull: false,
                tmpfs: Vec::new(),
                volumes: Vec::new(),
            })
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        runner.finish().await
    }
    async fn post_run(&self, _: &Self::Configuration) {
        println!("postrun a")
    }

    fn analyse(
        &self,
        _exp_dir: PathBuf,
        _date: chrono::DateTime<chrono::Utc>,
        _environment: Environment,
        configurations: Vec<(Self::Configuration, PathBuf)>,
    ) {
        let mut configs = HashMap::new();
        for (i, (_config, config_dir)) in configurations.iter().enumerate() {
            let mut repeats = HashMap::new();
            for (i, repeat_dir) in exp::repeat_dirs(config_dir).unwrap().iter().enumerate() {
                // get logs, stats and top for each container
                let mut logs = HashMap::new();
                for log_file in read_dir(repeat_dir.join("logs")).unwrap() {
                    if let Ok(log) = exp::docker_runner::Logs::from_file(&log_file.unwrap().path())
                    {
                        logs.insert(log.container_name.clone(), log);
                    }
                }
                let mut stats = HashMap::new();
                for stat_file in read_dir(repeat_dir.join("metrics")).unwrap() {
                    if let Ok(stat) =
                        exp::docker_runner::Stats::from_file(&stat_file.unwrap().path())
                    {
                        stats.insert(stat.container_name.clone(), stat);
                    }
                }
                let mut tops = HashMap::new();
                for top_file in read_dir(repeat_dir.join("metrics")).unwrap() {
                    if let Ok(top) = exp::docker_runner::Tops::from_file(&top_file.unwrap().path())
                    {
                        tops.insert(top.container_name.clone(), top);
                    }
                }
                repeats.insert(i, (logs, stats, tops));
            }
            configs.insert(i, repeats);
        }
    }
}

#[tokio::test]
async fn multiple() {
    let exp = ExpA {
        configurations: vec![ExpAConfig {}],
    };
    let results_dir = PathBuf::from("results");
    let run_config = exp::RunConfig {
        results_dir: results_dir.clone(),
    };
    exp::run(&exp, &run_config).await.unwrap();
    let analyse_config = exp::AnalyseConfig {
        results_dir,
        date: None,
    };
    exp::analyse(&exp, &analyse_config).await.unwrap();
}
