use std::{
    fs::{create_dir_all, File},
    io,
    io::Write,
    path::{Path, PathBuf},
};

use bollard::{
    container::{
        Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StatsOptions,
        StopContainerOptions, TopOptions,
    },
    Docker,
};
use futures::{future, stream::StreamExt};
use tracing::info;

// The docker runner for a particular experiment run
// handles creation of resources and teardown after
#[derive(Debug, Clone)]
pub struct Runner {
    containers: Vec<String>,
    docker: Docker,
    repeat_dir: PathBuf,
}

impl Runner {
    pub async fn new(repeat_dir: PathBuf) -> Self {
        let config_dir =
            create_config_dir(&repeat_dir).expect("Failed to create docker config dir");
        let docker = bollard::Docker::connect_with_local_defaults()
            .expect("Failed to connect to docker api");
        let version = docker
            .version()
            .await
            .expect("Failed to get docker version");
        let version_file = File::create(config_dir.join("docker-version.json"))
            .expect("Failed to create docker version file");
        serde_json::to_writer(version_file, &version).unwrap();
        let info = docker.info().await.expect("Failed to get docker info");
        let info_file = File::create(config_dir.join("docker-info.json"))
            .expect("Failed to create docker info file");
        serde_json::to_writer(info_file, &info).unwrap();
        Self {
            containers: Vec::new(),
            docker,
            repeat_dir,
        }
    }

    pub async fn add_container(&mut self, name: &str, config: Config<String>) {
        let config_dir =
            create_config_dir(&self.repeat_dir).expect("Failed to create docker config dir");
        let logs_dir = create_logs_dir(&self.repeat_dir).expect("Failed to create logs dir");
        let metrics_dir =
            create_metrics_dir(&self.repeat_dir).expect("Failed to create metrics dir");
        let config_file = File::create(&config_dir.join(format!("docker-{}.json", name)))
            .expect("Failed to create docker config file");
        serde_json::to_writer(config_file, &config).expect("Failed to write docker config");

        let _create_res = self
            .docker
            .create_container(Some(CreateContainerOptions { name }), config.clone())
            .await
            .expect("Failed to create container");

        self.docker
            .start_container::<String>(&name, None)
            .await
            .expect("Failed to start container");

        let docker = self.docker.clone();
        let name_owned = name.to_owned();
        tokio::spawn(async move {
            let logs = docker.logs(
                &name_owned,
                Some(LogsOptions::<String> {
                    follow: true,
                    stdout: true,
                    stderr: true,
                    timestamps: true,
                    ..Default::default()
                }),
            );
            let mut logs_file = File::create(logs_dir.join(format!("docker-{}.log", name_owned)))
                .expect("Failed to create logs file");
            logs.for_each(|item| {
                if let Ok(item) = item {
                    write!(logs_file, "{}", item).unwrap()
                }
                future::ready(())
            })
            .await
        });

        let docker = self.docker.clone();
        let name_owned = name.to_owned();
        let metrics_dir_c = metrics_dir.clone();
        tokio::spawn(async move {
            let stats = docker.stats(&name_owned, Some(StatsOptions { stream: true }));
            let mut stats_file =
                File::create(&metrics_dir_c.join(format!("docker-{}.stat", name_owned)))
                    .expect("Failed to create stats file");
            stats
                .for_each(|stat| {
                    writeln!(stats_file, "{:?}", stat).unwrap();
                    if let Ok(stat) = stat {
                        serde_json::to_writer(&mut stats_file, &stat).unwrap();
                        writeln!(stats_file).unwrap();
                    }
                    future::ready(())
                })
                .await
        });

        let docker = self.docker.clone();
        let name_owned = name.to_owned();
        tokio::spawn(async move {
            let interval = tokio::time::interval(std::time::Duration::from_secs(5));
            tokio::pin!(interval);

            let mut top_file =
                File::create(&metrics_dir.join(format!("docker-{}.top", name_owned)))
                    .expect("Failed to create top file");
            loop {
                let top = docker
                    .top_processes(&name_owned, Some(TopOptions { ps_args: "aux" }))
                    .await
                    .expect("Failed to get top info");
                serde_json::to_writer(&mut top_file, &top).unwrap();
                writeln!(top_file).unwrap();
                interval.tick().await;
            }
        });

        self.containers.push(name.to_owned());
    }

    pub async fn finish(self) {
        for c in self.containers {
            self.docker
                .stop_container(
                    &c,
                    Some(StopContainerOptions {
                        t: 10, // seconds until kill
                    }),
                )
                .await
                .expect("Failed to stop container");
            self.docker
                .remove_container(
                    &c,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await
                .expect("Failed to remove container")
        }
    }
}

fn create_config_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let conf_path = parent.join("config");
    info!(path = ?conf_path, "Creating config directory");
    create_dir_all(&conf_path)?;
    Ok(conf_path)
}

fn create_logs_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let logs_path = parent.join("logs");
    info!(path = ?logs_path, "Creating logs directory");
    create_dir_all(&logs_path)?;
    Ok(logs_path)
}

fn create_metrics_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let metrics_path = parent.join("metrics");
    info!(path = ?metrics_path, "Creating metrics directory");
    create_dir_all(&metrics_path)?;
    Ok(metrics_path)
}
