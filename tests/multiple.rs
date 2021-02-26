use std::path::PathBuf;

use exp::{AnalysableExperiment, ExperimentConfiguration, NamedExperiment, RunnableExperiment};
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

impl RunnableExperiment<'_> for ExpA {
    type RunConfiguration = ExpAConfig;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration> {
        self.configurations.clone()
    }
    fn pre_run(&self, _: &Self::RunConfiguration) {
        println!("prerun a")
    }
    fn run(&self, _: &Self::RunConfiguration, data_dir: PathBuf) {
        println!("run a")
    }
    fn post_run(&self, _: &Self::RunConfiguration) {
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

impl RunnableExperiment<'_> for ExpB {
    type RunConfiguration = ExpBConfig;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration> {
        self.configurations.clone()
    }
    fn pre_run(&self, _: &Self::RunConfiguration) {
        todo!()
    }
    fn run(&self, _: &Self::RunConfiguration, data_dir: PathBuf) {
        todo!()
    }
    fn post_run(&self, _: &Self::RunConfiguration) {
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

impl RunnableExperiment<'_> for Exp {
    type RunConfiguration = ExpConfig;

    fn run_configurations(&self) -> Vec<Self::RunConfiguration> {
        match self {
            Self::A(a) => a
                .run_configurations()
                .into_iter()
                .map(|c| ExpConfig::A(c))
                .collect::<Vec<_>>(),
            Self::B(b) => b
                .run_configurations()
                .into_iter()
                .map(|c| ExpConfig::B(c))
                .collect::<Vec<_>>(),
        }
    }
    fn pre_run(&self, config: &Self::RunConfiguration) {
        match (self, config) {
            (Self::A(a), Self::RunConfiguration::A(ac)) => a.pre_run(ac),
            (Self::B(b), Self::RunConfiguration::B(bc)) => b.pre_run(bc),
            _ => panic!("found mismatching experiment and configuration"),
        }
    }
    fn run(&self, _: &Self::RunConfiguration, data_dir: PathBuf) {
        println!("run")
    }
    fn post_run(&self, _: &Self::RunConfiguration) {
        println!("postrun")
    }
}

#[test]
fn multiple() {
    let exps = vec![
        Exp::A(ExpA {
            configurations: vec![ExpAConfig {}],
        }),
        Exp::B(ExpB {
            configurations: vec![],
        }),
    ];
    exp::run_all(&exps).unwrap()
}
