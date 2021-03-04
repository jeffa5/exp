use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use exp::{docker_runner::ContainerConfig, Experiment, ExperimentConfiguration};
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
        println!("run a {:?}", repeat_dir)
    }
    async fn post_run(&self, _: &Self::Configuration) {
        println!("postrun a")
    }

    fn analyse(
        &self,
        exp_dir: PathBuf,
        date: chrono::DateTime<chrono::Local>,
        configurations: &[(Self::Configuration, PathBuf)],
    ) {
        println!("analyse")
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct ExpBConfig {}

impl ExperimentConfiguration for ExpBConfig {
    fn repeats(&self) -> u32 {
        1
    }

    fn description(&self) -> &str {
        "exp b"
    }
}

struct ExpB {
    configurations: Vec<ExpBConfig>,
}

#[async_trait]
impl Experiment for ExpB {
    type Configuration = ExpBConfig;

    fn name(&self) -> &str {
        "b"
    }

    fn configurations(&self) -> Vec<Self::Configuration> {
        self.configurations.clone()
    }
    async fn pre_run(&self, _: &Self::Configuration) {
        todo!()
    }
    async fn run(&self, _: &Self::Configuration, repeat_dir: PathBuf) {
        todo!()
    }
    async fn post_run(&self, _: &Self::Configuration) {
        todo!()
    }

    fn analyse(
        &self,
        exp_dir: PathBuf,
        date: chrono::DateTime<chrono::Local>,
        configurations: &[(Self::Configuration, PathBuf)],
    ) {
        println!("analyse")
    }
}

#[derive(Serialize, Deserialize)]
enum ExpConfig {
    A(ExpAConfig),
    B(ExpBConfig),
}

impl ExperimentConfiguration for ExpConfig {
    fn repeats(&self) -> u32 {
        match self {
            Self::A(a) => a.repeats(),
            Self::B(b) => b.repeats(),
        }
    }

    fn description(&self) -> &str {
        match self {
            Self::A(a) => a.description(),
            Self::B(b) => b.description(),
        }
    }
}

enum Exp {
    A(ExpA),
    B(ExpB),
}

#[async_trait]
impl Experiment for Exp {
    type Configuration = ExpConfig;

    fn name(&self) -> &str {
        match self {
            Self::A(a) => a.name(),
            Self::B(b) => b.name(),
        }
    }

    fn configurations(&self) -> Vec<Self::Configuration> {
        match self {
            Self::A(a) => a
                .configurations()
                .into_iter()
                .map(ExpConfig::A)
                .collect::<Vec<_>>(),
            Self::B(b) => b
                .configurations()
                .into_iter()
                .map(ExpConfig::B)
                .collect::<Vec<_>>(),
        }
    }
    async fn pre_run(&self, config: &Self::Configuration) {
        match (self, config) {
            (Self::A(a), ExpConfig::A(ac)) => a.pre_run(ac).await,
            (Self::B(b), ExpConfig::B(bc)) => b.pre_run(bc).await,
            _ => {
                panic!("found mismatching experiment and configuration")
            }
        }
    }
    async fn run(&self, _: &Self::Configuration, repeat_dir: PathBuf) {
        let mut runner = exp::docker_runner::Runner::new(repeat_dir).await;
        println!("run");

        runner
            .add_container(&ContainerConfig {
                name: "exp-test-1".to_owned(),
                image_name: "nginx".to_owned(),
                image_tag: "alpine".to_owned(),
                network: Some("exp-test-net".to_owned()),
                network_subnet: None,
                command: None,
                ports: Some(vec![("90".to_owned(), "80".to_owned())]),
            })
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        runner.finish().await
    }
    async fn post_run(&self, _: &Self::Configuration) {
        println!("postrun")
    }

    fn analyse(
        &self,
        exp_dir: PathBuf,
        date: chrono::DateTime<chrono::Local>,
        configurations: &[(Self::Configuration, PathBuf)],
    ) {
        match self {
            Self::A(a) => {
                let confs = configurations
                    .iter()
                    .map(|(c, p)| match c {
                        ExpConfig::A(a) => (a.clone(), p.clone()),
                        ExpConfig::B(_) => panic!("found wrong config"),
                    })
                    .collect::<Vec<_>>();
                a.analyse(exp_dir, date, &confs)
            }
            Self::B(b) => {
                let confs = configurations
                    .iter()
                    .map(|(c, p)| match c {
                        ExpConfig::A(_) => panic!("found wrong config"),
                        ExpConfig::B(b) => (b.clone(), p.clone()),
                    })
                    .collect::<Vec<_>>();
                b.analyse(exp_dir, date, &confs)
            }
        }
    }
}

#[tokio::test]
async fn multiple() {
    let exps = vec![
        Exp::A(ExpA {
            configurations: vec![ExpAConfig {}],
        }),
        Exp::B(ExpB {
            configurations: vec![],
        }),
    ];
    let run_config = exp::RunConfig {
        output_dir: std::env::current_dir().unwrap(),
    };
    exp::run(&exps, &run_config).await.unwrap();
    let analyse_config = exp::AnalyseConfig {
        output_dir: std::env::current_dir().unwrap(),
        date: None,
    };
    exp::analyse(&exps, &analyse_config).await.unwrap();
}
