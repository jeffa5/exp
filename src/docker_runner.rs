use std::{
    collections::HashMap,
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
    models::{EndpointSettings, HostConfig, PortBinding},
    network::{ConnectNetworkOptions, CreateNetworkOptions, ListNetworksOptions},
    Docker,
};
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// The docker runner for a particular experiment run
// handles creation of resources and teardown after
#[derive(Debug)]
pub struct Runner {
    containers: Vec<String>,
    networks: Vec<String>,
    docker: Docker,
    repeat_dir: PathBuf,
    end_tx: tokio::sync::watch::Sender<()>,
    end_rx: tokio::sync::watch::Receiver<()>,
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
        let (end_tx, end_rx) = tokio::sync::watch::channel(());
        Self {
            containers: Vec::new(),
            networks: Vec::new(),
            docker,
            repeat_dir,
            end_tx,
            end_rx,
        }
    }

    pub async fn add_container(&mut self, config: &ContainerConfig) {
        let config_dir =
            create_config_dir(&self.repeat_dir).expect("Failed to create docker config dir");
        let logs_dir = create_logs_dir(&self.repeat_dir).expect("Failed to create logs dir");
        let metrics_dir =
            create_metrics_dir(&self.repeat_dir).expect("Failed to create metrics dir");
        let config_file = File::create(&config_dir.join(format!("docker-{}.json", config.name)))
            .expect("Failed to create docker config file");
        serde_json::to_writer(config_file, &config).expect("Failed to write docker config");

        let _create_res = self
            .docker
            .create_container(
                Some(CreateContainerOptions { name: &config.name }),
                config.to_create_container_config(),
            )
            .await
            .expect("Failed to create container");

        if let Some(network_name) = &config.network {
            let mut net_filters = HashMap::new();
            net_filters.insert("name", vec![network_name.as_str()]);
            let net_count = self
                .docker
                .list_networks(Some(ListNetworksOptions {
                    filters: net_filters,
                }))
                .await
                .expect("Failed to list networks")
                .iter()
                .filter(|n| n.name.as_ref() == Some(network_name))
                .count();
            if net_count == 0 {
                self.docker
                    .create_network(CreateNetworkOptions {
                        name: network_name.as_str(),
                        check_duplicate: true,
                        ..Default::default()
                    })
                    .await
                    .expect("Failed to create network");
                self.networks.push(network_name.clone());
            }

            self.docker
                .connect_network(
                    network_name,
                    ConnectNetworkOptions {
                        container: config.name.as_str(),
                        endpoint_config: EndpointSettings {
                            ..Default::default()
                        },
                    },
                )
                .await
                .expect("Failed to connect container to network")
        }

        self.containers.push(config.name.to_owned());

        self.docker
            .start_container::<String>(&config.name, None)
            .await
            .expect("Failed to start container");

        let docker = self.docker.clone();
        let name_owned = config.name.to_owned();
        let mut end_rx_clone = self.end_rx.clone();
        tokio::spawn(async move {
            let mut logs = docker.logs(
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
            loop {
                tokio::select! {
                    _ = end_rx_clone.changed() => {
                        break
                    }
                    Some(item) = logs.next() => {
                        if let Ok(item) = item {
                            write!(logs_file, "{}", item).unwrap()
                        } else {
                            warn!("Error getting log line: {:?}", item)
                        }
                    }
                    else => break
                }
            }
        });

        let docker = self.docker.clone();
        let name_owned = config.name.to_owned();
        let metrics_dir_c = metrics_dir.clone();
        let mut end_rx_clone = self.end_rx.clone();
        tokio::spawn(async move {
            let mut stats = docker.stats(&name_owned, Some(StatsOptions { stream: true }));
            let mut stats_file =
                File::create(&metrics_dir_c.join(format!("docker-{}.stat", name_owned)))
                    .expect("Failed to create stats file");
            loop {
                tokio::select! {
                    _ = end_rx_clone.changed() => break,
                    Some(stat) = stats.next() => {
                        if let Ok(stat) = stat {
                            serde_json::to_writer(&mut stats_file, &stat).unwrap();
                            writeln!(stats_file).unwrap();
                        } else {
                            warn!("Error getting stats statistics: {:?}", stat);
                        }
                    }
                    else => break,
                }
            }
        });

        let docker = self.docker.clone();
        let name_owned = config.name.to_owned();
        let mut end_rx_clone = self.end_rx.clone();
        tokio::spawn(async move {
            let interval = tokio::time::interval(std::time::Duration::from_secs(1));
            tokio::pin!(interval);

            let mut top_file =
                File::create(&metrics_dir.join(format!("docker-{}.top", name_owned)))
                    .expect("Failed to create top file");
            loop {
                tokio::select! {
                    _ = end_rx_clone.changed() => break,
                    _ = interval.tick() => {
                        let top = docker
                            .top_processes(&name_owned, Some(TopOptions { ps_args: "aux" }))
                            .await;
                        if let Ok(top) = top {
                            serde_json::to_writer(&mut top_file, &top).unwrap();
                            writeln!(top_file).unwrap();
                        }else {
                            warn!("Error getting top statistics: {:?}", top);
                        }
                    }
                    else => break,
                }
            }
        });
    }

    pub async fn finish(self) {
        let r = self.end_tx.send(());
        if let Err(e) = r {
            warn!("Error sending shutdown signal to monitoring tasks: {}", e)
        }
        for c in self.containers {
            let r = self
                .docker
                .stop_container(
                    &c,
                    Some(StopContainerOptions {
                        t: 10, // seconds until kill
                    }),
                )
                .await;
            if let Err(e) = r {
                warn!("Error stopping container '{}': {}", c, e)
            }
            let r = self
                .docker
                .remove_container(
                    &c,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
            if let Err(e) = r {
                warn!("Error removing container '{}': {}", c, e)
            }
        }

        for n in self.networks {
            let r = self.docker.remove_network(&n).await;
            if let Err(e) = r {
                warn!("Error removing network '{}': {}", n, e)
            }
        }
    }

    pub fn docker_client(&self) -> &Docker {
        &self.docker
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub name: String,
    pub image_name: String,
    pub image_tag: String,
    pub network: Option<String>,
    pub command: Option<Vec<String>>,
    pub ports: Option<Vec<(String, String)>>,
}

impl ContainerConfig {
    fn to_create_container_config(&self) -> Config<String> {
        let mut exposed_ports = HashMap::new();
        let mut port_bindings = HashMap::new();
        if let Some(ports) = &self.ports {
            for (e, i) in ports {
                exposed_ports.insert(e.clone(), HashMap::new());
                port_bindings.insert(
                    e.clone(),
                    Some(vec![PortBinding {
                        host_ip: None,
                        host_port: Some(i.clone()),
                    }]),
                );
            }
        }
        Config {
            image: Some(format!("{}:{}", self.image_name, self.image_tag)),
            cmd: self.command.clone(),
            exposed_ports: Some(exposed_ports),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                ..Default::default()
            }),
            ..Default::default()
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
