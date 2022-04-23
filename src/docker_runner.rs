use std::{
    collections::HashMap,
    fs::{create_dir_all, File},
    io,
    io::{BufRead, ErrorKind, Write},
    path::{Path, PathBuf},
};

use bollard::{
    container::{
        Config, CreateContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
        StatsOptions, StopContainerOptions, TopOptions,
    },
    image::CreateImageOptions,
    models::{HostConfig, Ipam, Mount, MountTypeEnum, PortBinding},
    network::{CreateNetworkOptions, ListNetworksOptions},
    Docker,
};
use futures::{future::join_all, stream::StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
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
    futures: Vec<JoinHandle<()>>,
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
            futures: Vec::new(),
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
                let mut network_config = HashMap::new();
                if let Some(subnet) = &config.network_subnet {
                    network_config.insert("Subnet".to_owned(), subnet.clone());
                }
                self.docker
                    .create_network(CreateNetworkOptions {
                        name: network_name.as_str(),
                        check_duplicate: true,
                        ipam: Ipam {
                            config: if network_config.is_empty() {
                                None
                            } else {
                                Some(vec![network_config])
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .await
                    .expect("Failed to create network");
                self.networks.push(network_name.clone());
            }
        }

        if config.pull {
            pull_image(&config.image_name, &config.image_tag)
                .await
                .expect("Failed to pull image");
        }

        let _create_res = self
            .docker
            .create_container(
                Some(CreateContainerOptions { name: &config.name }),
                config.to_create_container_config(),
            )
            .await
            .expect("Failed to create container");

        self.containers.push(config.name.to_owned());

        self.docker
            .start_container::<String>(&config.name, None)
            .await
            .expect("Failed to start container");

        let docker = self.docker.clone();
        let name_owned = config.name.to_owned();
        let mut end_rx_clone = self.end_rx.clone();
        self.futures.push(tokio::spawn(async move {
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
        }));

        let docker = self.docker.clone();
        let name_owned = config.name.to_owned();
        let metrics_dir_c = metrics_dir.clone();
        let mut end_rx_clone = self.end_rx.clone();
        self.futures.push(tokio::spawn(async move {
            let mut stats = docker.stats(
                &name_owned,
                Some(StatsOptions {
                    stream: true,
                    one_shot: false,
                }),
            );
            let mut stats_file =
                File::create(&metrics_dir_c.join(format!("docker-{}.stat", name_owned)))
                    .expect("Failed to create stats file");
            loop {
                tokio::select! {
                    _ = end_rx_clone.changed() => break,
                    Some(stat) = stats.next() => {
                        if let Ok(stat) = stat {
                            let time = chrono::Utc::now().to_rfc3339();
                            write!(stats_file, "{} ", time).unwrap();
                            serde_json::to_writer(&mut stats_file, &stat).unwrap();
                            writeln!(stats_file).unwrap();
                        } else {
                            warn!("Error getting stats statistics: {:?}", stat);
                        }
                    }
                    else => break,
                }
            }
        }));

        let docker = self.docker.clone();
        let name_owned = config.name.to_owned();
        let mut end_rx_clone = self.end_rx.clone();
        self.futures.push(tokio::spawn(async move {
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
                            let time = chrono::Utc::now().to_rfc3339();
                            write!(top_file, "{} ", time).unwrap();
                            serde_json::to_writer(&mut top_file, &top).unwrap();
                            writeln!(top_file).unwrap();
                        }else {
                            warn!("Error getting top statistics: {:?}", top);
                        }
                    }
                    else => break,
                }
            }
        }));
    }

    pub async fn finish(self) {
        let r = self.end_tx.send(());
        if let Err(e) = r {
            warn!("Error sending shutdown signal to monitoring tasks: {}", e)
        }
        join_all(self.futures).await;
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

#[derive(Debug, Clone)]
pub struct Logs {
    pub container_name: String,
    pub lines: Vec<(chrono::DateTime<chrono::Utc>, String)>,
}

impl Logs {
    pub fn from_file(path: &Path) -> io::Result<Self> {
        if let Some(file_name) = path.file_stem() {
            if let Some(name) = file_name.to_string_lossy().strip_prefix("docker-") {
                let file = File::open(path)?;
                let mut lines = Vec::new();
                for line in std::io::BufReader::new(file).lines() {
                    let line = line.unwrap();
                    let split = line.splitn(2, ' ').collect::<Vec<_>>();
                    if let [date, text] = split[..] {
                        let date = chrono::DateTime::parse_from_rfc3339(date)
                            .unwrap()
                            .with_timezone(&chrono::Utc);
                        lines.push((date, text.to_owned()));
                    }
                }
                Ok(Logs {
                    container_name: name.to_owned(),
                    lines,
                })
            } else {
                Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "filename should start with docker-",
                ))
            }
        } else {
            Err(io::Error::new(ErrorKind::NotFound, "missing file_stem"))
        }
    }
}

#[derive(Debug, Clone)]
pub struct Stats {
    pub container_name: String,
    pub lines: Vec<(chrono::DateTime<chrono::Utc>, bollard::container::Stats)>,
}

impl Stats {
    pub fn from_file(path: &Path) -> io::Result<Self> {
        if let Some(file_name) = path.file_stem() {
            if path.extension().unwrap_or_default().to_string_lossy() == "stat" {
                if let Some(name) = file_name.to_string_lossy().strip_prefix("docker-") {
                    let file = File::open(path)?;
                    let mut lines = Vec::new();
                    for line in std::io::BufReader::new(file).lines() {
                        let line = line.unwrap();
                        let split = line.splitn(2, ' ').collect::<Vec<_>>();
                        if let [date, text] = split[..] {
                            let date = chrono::DateTime::parse_from_rfc3339(date)
                                .unwrap()
                                .with_timezone(&chrono::Utc);
                            let stats: bollard::container::Stats =
                                serde_json::from_str(text).unwrap();
                            lines.push((date, stats));
                        }
                    }
                    Ok(Stats {
                        container_name: name.to_owned(),
                        lines,
                    })
                } else {
                    Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        "filename should start with docker-",
                    ))
                }
            } else {
                Err(io::Error::new(ErrorKind::InvalidInput, "wrong file format"))
            }
        } else {
            Err(io::Error::new(ErrorKind::NotFound, "missing file_stem"))
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tops {
    pub container_name: String,
    pub lines: Vec<(
        chrono::DateTime<chrono::Utc>,
        bollard::models::ContainerTopResponse,
    )>,
}

impl Tops {
    pub fn from_file(path: &Path) -> io::Result<Self> {
        if let Some(file_name) = path.file_stem() {
            if path.extension().unwrap_or_default().to_string_lossy() == "stat" {
                if let Some(name) = file_name.to_string_lossy().strip_prefix("docker-") {
                    let file = File::open(path)?;
                    let mut lines = Vec::new();
                    for line in std::io::BufReader::new(file).lines() {
                        let line = line.unwrap();
                        let split = line.splitn(2, ' ').collect::<Vec<_>>();
                        if let [date, text] = split[..] {
                            let date = chrono::DateTime::parse_from_rfc3339(date)
                                .unwrap()
                                .with_timezone(&chrono::Utc);
                            let top: bollard::models::ContainerTopResponse =
                                serde_json::from_str(text).unwrap();
                            lines.push((date, top));
                        }
                    }
                    Ok(Tops {
                        container_name: name.to_owned(),
                        lines,
                    })
                } else {
                    Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        "filename should start with docker-",
                    ))
                }
            } else {
                Err(io::Error::new(ErrorKind::InvalidInput, "wrong file format"))
            }
        } else {
            Err(io::Error::new(ErrorKind::NotFound, "missing file_stem"))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub name: String,
    pub image_name: String,
    pub image_tag: String,
    pub pull: bool,
    pub network: Option<String>,
    pub network_subnet: Option<String>,
    pub command: Option<Vec<String>>,
    pub ports: Option<Vec<(String, String)>>,
    pub capabilities: Option<Vec<String>>,
    pub cpus: Option<f64>,
    pub memory: Option<i64>,
    /// Mount the given paths as tmpfs directories.
    pub tmpfs: Vec<String>,
    pub volumes: Vec<(String, String)>,
}

impl ContainerConfig {
    fn to_create_container_config(&self) -> Config<String> {
        let mut exposed_ports = HashMap::new();
        let mut port_bindings = HashMap::new();
        if let Some(ports) = &self.ports {
            for (i, e) in ports {
                let e = format!("{}/tcp", e);
                exposed_ports.insert(e.clone(), HashMap::new());
                port_bindings.insert(
                    e.clone(),
                    Some(vec![PortBinding {
                        host_ip: Some("0.0.0.0".to_owned()),
                        host_port: Some(i.clone()),
                    }]),
                );
            }
        }
        let cpu_period = 100000;

        let mut tmpfs_mounts = self
            .tmpfs
            .iter()
            .map(|path| Mount {
                target: Some(path.clone()),
                typ: Some(MountTypeEnum::TMPFS),
                ..Default::default()
            })
            .collect();

        let mut volume_mounts = self
            .volumes
            .iter()
            .map(|(host, target)| Mount {
                target: Some(target.clone()),
                source: Some(host.clone()),
                typ: Some(MountTypeEnum::BIND),
                ..Default::default()
            })
            .collect();

        let mut mounts = Vec::new();
        mounts.append(&mut tmpfs_mounts);
        mounts.append(&mut volume_mounts);

        Config {
            image: Some(format!("{}:{}", self.image_name, self.image_tag)),
            cmd: self.command.clone(),
            exposed_ports: Some(exposed_ports),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                network_mode: Some(
                    self.network
                        .as_ref()
                        .unwrap_or(&"default".to_owned())
                        .to_owned(),
                ),
                cap_add: self.capabilities.clone(),
                cpu_period: self.cpus.map(|_| cpu_period),
                cpu_quota: self.cpus.map(|cpus| (cpu_period as f64 * cpus) as i64),
                memory: self.memory,
                mounts: Some(mounts),
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

pub async fn pull_image(image_name: &str, image_tag: &str) -> Result<(), bollard::errors::Error> {
    let docker =
        bollard::Docker::connect_with_local_defaults().expect("Failed to connect to docker api");

    docker
        .create_image(
            Some(CreateImageOptions {
                from_image: image_name,
                tag: image_tag,
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await?;
    Ok(())
}

pub async fn clean(prefix: &str) -> Result<(), bollard::errors::Error> {
    let docker = bollard::Docker::connect_with_local_defaults()?;
    let mut filters = HashMap::new();
    filters.insert("name", vec![prefix]);
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            limit: None,
            size: false,
            filters,
        }))
        .await?;
    for container in containers {
        let name = &container
            .names
            .and_then(|names| names.first().cloned())
            .unwrap_or_default();
        info!(?name, "Removing container");
        docker
            .remove_container(
                name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;
    }
    Ok(())
}
