use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use exp::{
    docker_runner::ContainerConfig, AnalysableExperiment, ExperimentConfiguration, NamedExperiment,
    RunnableExperiment,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct ExpAConfig {}

impl ExperimentConfiguration<'_> for ExpAConfig {
    fn repeats(&self) -> u32 {
        2
    }
}

struct ExpA {
    configurations: Vec<ExpAConfig>,
}

impl NamedExperiment for ExpA {
    fn name(&self) -> &str {
        "a"
    }
}

#[async_trait]
impl RunnableExperiment<'_> for ExpA {
    type RunConfiguration = ExpAConfig;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration> {
        self.configurations.clone()
    }
    async fn pre_run(&self, _: &Self::RunConfiguration) {
        println!("prerun a")
    }
    async fn run(&self, _: &Self::RunConfiguration, repeat_dir: PathBuf) {
        println!("run a {:?}", repeat_dir)
    }
    async fn post_run(&self, _: &Self::RunConfiguration) {
        println!("postrun a")
    }
}

impl AnalysableExperiment for ExpA {
    fn pre_analyse(&self) {
        todo!()
    }
    fn analyse(&self) {
        todo!()
    }
    fn post_analyse(&self) {
        todo!()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct ExpBConfig {}

impl ExperimentConfiguration<'_> for ExpBConfig {
    fn repeats(&self) -> u32 {
        1
    }
}

struct ExpB {
    configurations: Vec<ExpBConfig>,
}

impl NamedExperiment for ExpB {
    fn name(&self) -> &str {
        "b"
    }
}

#[async_trait]
impl RunnableExperiment<'_> for ExpB {
    type RunConfiguration = ExpBConfig;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration> {
        self.configurations.clone()
    }
    async fn pre_run(&self, _: &Self::RunConfiguration) {
        todo!()
    }
    async fn run(&self, _: &Self::RunConfiguration, repeat_dir: PathBuf) {
        todo!()
    }
    async fn post_run(&self, _: &Self::RunConfiguration) {
        todo!()
    }
}

impl AnalysableExperiment for ExpB {
    fn pre_analyse(&self) {
        todo!()
    }
    fn analyse(&self) {
        todo!()
    }
    fn post_analyse(&self) {
        todo!()
    }
}

#[derive(Serialize, Deserialize)]
enum ExpConfig {
    A(ExpAConfig),
    B(ExpBConfig),
}

impl ExperimentConfiguration<'_> for ExpConfig {
    fn repeats(&self) -> u32 {
        match self {
            Self::A(a) => a.repeats(),
            Self::B(b) => b.repeats(),
        }
    }
}

enum Exp {
    A(ExpA),
    B(ExpB),
}

impl NamedExperiment for Exp {
    fn name(&self) -> &str {
        match self {
            Self::A(a) => a.name(),
            Self::B(b) => b.name(),
        }
    }
}

#[async_trait]
impl RunnableExperiment<'_> for Exp {
    type RunConfiguration = ExpConfig;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration> {
        match self {
            Self::A(a) => a
                .run_configurations()
                .into_iter()
                .map(ExpConfig::A)
                .collect::<Vec<_>>(),
            Self::B(b) => b
                .run_configurations()
                .into_iter()
                .map(ExpConfig::B)
                .collect::<Vec<_>>(),
        }
    }
    async fn pre_run(&self, config: &Self::RunConfiguration) {
        match (self, config) {
            (Self::A(a), ExpConfig::A(ac)) => a.pre_run(ac).await,
            (Self::B(b), ExpConfig::B(bc)) => b.pre_run(bc).await,
            _ => {
                panic!("found mismatching experiment and configuration")
            }
        }
    }
    async fn run(&self, _: &Self::RunConfiguration, repeat_dir: PathBuf) {
        let mut runner = exp::docker_runner::Runner::new(repeat_dir).await;
        println!("run");

        runner
            .add_container(&ContainerConfig {
                name: "exp-test-1".to_owned(),
                image_name: "nginx".to_owned(),
                image_tag: "alpine".to_owned(),
                network: Some("exp-test-net".to_owned()),
            })
            .await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        runner.finish().await
    }
    async fn post_run(&self, _: &Self::RunConfiguration) {
        println!("postrun")
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
    exp::run(&exps, &run_config).await.unwrap()
}
