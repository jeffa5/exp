use std::{
    fs::{create_dir_all, File},
    path::Path,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ExperimentConfiguration, RunnableExperiment};

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
}

pub fn run_all<'a, E: RunnableExperiment<'a>>(experiments: &[E]) -> Result<(), RunError> {
    let exp_path = Path::new("experiments").join(chrono::Local::now().to_rfc3339());
    create_dir_all(&exp_path).unwrap();
    for e in experiments {
        run(e, &exp_path)?
    }
    Ok(())
}

pub fn run<'a, E: RunnableExperiment<'a>>(experiment: &E, dir: &Path) -> Result<(), RunError> {
    let experiment_dir = dir.join(experiment.name());
    create_dir_all(&experiment_dir)?;
    collect_environment_data(&experiment_dir);

    let configurations = experiment.run_configurations();
    let width = configurations.len().to_string().len();
    for (i, config) in configurations.iter().enumerate() {
        let config_dir =
            experiment_dir.join(format!("configuration-{:0>width$}", i + 1, width = width));
        create_dir_all(&config_dir)?;
        let config_file = File::create(&config_dir.join("configuration.json"))?;
        serde_json::to_writer(config_file, &config)?;
        experiment.pre_run(&config);
        let repeats = config.repeats();
        for i in 0..repeats {
            let repeat_dir = config_dir.join(format!(
                "repeat-{:0>width$}",
                i + 1,
                width = repeats.to_string().len()
            ));
            create_dir_all(&repeat_dir)?;
            let logs_dir = repeat_dir.join("logs");
            create_dir_all(logs_dir)?;
            let metrics_dir = repeat_dir.join("metrics");
            create_dir_all(metrics_dir)?;
            let data_dir = repeat_dir.join("data");
            create_dir_all(&data_dir)?;
            experiment.run(&config, data_dir);
        }
        experiment.post_run(&config);
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Environment {
    hostname: String,
    os: String,
    release: String,
    version: String,
    architecture: String,
}

fn collect_environment_data(path: &Path) {
    let utsname = nix::sys::utsname::uname();
    let env = Environment {
        hostname: utsname.nodename().to_owned(),
        os: utsname.sysname().to_owned(),
        release: utsname.release().to_owned(),
        version: utsname.version().to_owned(),
        architecture: utsname.machine().to_owned(),
    };
    let env_file = File::create(path.join("environment.json")).unwrap();
    serde_json::to_writer(env_file, &env).unwrap();
}
