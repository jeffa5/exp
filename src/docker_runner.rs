use bollard::container::MemoryStatsStats;
use chrono::DateTime;
use chrono::Utc;
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
    models::{HostConfig, Ipam, IpamConfig, Mount, MountTypeEnum, PortBinding},
    network::{CreateNetworkOptions, ListNetworksOptions},
    Docker,
};
use futures::{future::join_all, stream::StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

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
        serde_json::to_writer_pretty(version_file, &version).unwrap();
        let info = docker.info().await.expect("Failed to get docker info");
        let info_file = File::create(config_dir.join("docker-info.json"))
            .expect("Failed to create docker info file");
        serde_json::to_writer_pretty(info_file, &info).unwrap();
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
        serde_json::to_writer_pretty(config_file, &config).expect("Failed to write docker config");

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
                let network_config = config.network_subnet.as_ref().map(|subnet| {
                    vec![IpamConfig {
                        subnet: Some(subnet.clone()),
                        ..Default::default()
                    }]
                });
                self.docker
                    .create_network(CreateNetworkOptions {
                        name: network_name.as_str(),
                        check_duplicate: true,
                        ipam: Ipam {
                            config: network_config,
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
                        match item {
                            Ok(item) => {
                                write!(logs_file, "{}", item).unwrap();
                            }
                            Err(error) => {
                                warn!(%error, "Error getting log line");
                            }
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
            let stats_file_name = metrics_dir_c.join(format!("docker-{}-stat.csv", name_owned));
            let mut writer = csv::Writer::from_path(stats_file_name).unwrap();
            loop {
                tokio::select! {
                    _ = end_rx_clone.changed() => break,
                    Some(stat) = stats.next() => {
                        match stat {
                            Ok(stats) => {
                                let stat = Stats::from_bollard(stats);
                                println!("got stats entry");
                                for stat in stat {
                                    writer.serialize(stat).unwrap();
                                }
                            }
                            Err(error) => {
                                warn!(%error, "Error getting stats statistics");
                            }
                        }
                    }
                    else => break,
                }
            }
            writer.flush().unwrap();
        }));

        let docker = self.docker.clone();
        let name_owned = config.name.to_owned();
        let mut end_rx_clone = self.end_rx.clone();
        self.futures.push(tokio::spawn(async move {
            let interval = tokio::time::interval(std::time::Duration::from_secs(1));
            tokio::pin!(interval);

            let top_file = metrics_dir.join(format!("docker-{}-top.csv", name_owned));
            let mut writer = csv::Writer::from_path(top_file).unwrap();
            let mut written_header = false;
            loop {
                tokio::select! {
                    _ = end_rx_clone.changed() => break,
                    _ = interval.tick() => {
                        let top = docker
                            .top_processes(&name_owned, Some(TopOptions { ps_args: "aux" }))
                            .await;
                        match top {
                            Ok(top) => {
                                if !written_header {
                                    let mut titles = top.titles.unwrap();
                                    titles.push("timestamp_nanos".to_owned());
                                    writer.write_record(titles).unwrap();
                                    written_header=true;
                                }
                                let now = chrono::Utc::now().timestamp_nanos().to_string();
                                for process in top.processes .unwrap(){
                                    let mut process= process;
                                    process.push(now.clone());
                                    writer.write_record(process).unwrap();
                                }
                            }
                            Err(error) => {
                                warn!(%error, "Error getting top statistics");
                            }
                        }
                    }
                    else => break,
                }
            }
            writer.flush().unwrap();
        }));
    }

    pub async fn finish(self) {
        let r = self.end_tx.send(());
        if let Err(error) = r {
            warn!(%error, "Error sending shutdown signal to monitoring tasks")
        }
        join_all(self.futures).await;
        for container in self.containers {
            let r = self
                .docker
                .stop_container(
                    &container,
                    Some(StopContainerOptions {
                        t: 10, // seconds until kill
                    }),
                )
                .await;
            if let Err(error) = r {
                warn!(%error, %container, "Error stopping container")
            }
            let r = self
                .docker
                .remove_container(
                    &container,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
            if let Err(error) = r {
                warn!(%error, %container, "Error removing container")
            }
        }

        for network in self.networks {
            let r = self.docker.remove_network(&network).await;
            if let Err(error) = r {
                warn!(%error, %network, "Error removing network")
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    // from bollard::container::Stats
    pub read: DateTime<Utc>,
    pub preread: DateTime<Utc>,
    pub num_procs: u32,
    pub pids_stats_current: Option<u64>,
    pub pids_stats_limit: Option<u64>,
    pub network_rx_dropped: Option<u64>,
    pub network_rx_bytes: Option<u64>,
    pub network_rx_errors: Option<u64>,
    pub network_rx_packets: Option<u64>,
    pub network_tx_packets: Option<u64>,
    pub network_tx_dropped: Option<u64>,
    pub network_tx_errors: Option<u64>,
    pub network_tx_bytes: Option<u64>,
    // flattened map from networks
    pub networks_name: Option<String>,
    pub networks_rx_dropped: Option<u64>,
    pub networks_rx_bytes: Option<u64>,
    pub networks_rx_errors: Option<u64>,
    pub networks_rx_packets: Option<u64>,
    pub networks_tx_packets: Option<u64>,
    pub networks_tx_dropped: Option<u64>,
    pub networks_tx_errors: Option<u64>,
    pub networks_tx_bytes: Option<u64>,

    // v1 memory stats
    pub memory_stats_stats_v1_cache: Option<u64>,
    pub memory_stats_stats_v1_dirty: Option<u64>,
    pub memory_stats_stats_v1_mapped_file: Option<u64>,
    pub memory_stats_stats_v1_total_inactive_file: Option<u64>,
    pub memory_stats_stats_v1_pgpgout: Option<u64>,
    pub memory_stats_stats_v1_rss: Option<u64>,
    pub memory_stats_stats_v1_total_mapped_file: Option<u64>,
    pub memory_stats_stats_v1_writeback: Option<u64>,
    pub memory_stats_stats_v1_unevictable: Option<u64>,
    pub memory_stats_stats_v1_pgpgin: Option<u64>,
    pub memory_stats_stats_v1_total_unevictable: Option<u64>,
    pub memory_stats_stats_v1_pgmajfault: Option<u64>,
    pub memory_stats_stats_v1_total_rss: Option<u64>,
    pub memory_stats_stats_v1_total_rss_huge: Option<u64>,
    pub memory_stats_stats_v1_total_writeback: Option<u64>,
    pub memory_stats_stats_v1_total_inactive_anon: Option<u64>,
    pub memory_stats_stats_v1_rss_huge: Option<u64>,
    pub memory_stats_stats_v1_hierarchical_memory_limit: Option<u64>,
    pub memory_stats_stats_v1_total_pgfault: Option<u64>,
    pub memory_stats_stats_v1_total_active_file: Option<u64>,
    pub memory_stats_stats_v1_active_anon: Option<u64>,
    pub memory_stats_stats_v1_total_active_anon: Option<u64>,
    pub memory_stats_stats_v1_total_pgpgout: Option<u64>,
    pub memory_stats_stats_v1_total_cache: Option<u64>,
    pub memory_stats_stats_v1_total_dirty: Option<u64>,
    pub memory_stats_stats_v1_inactive_anon: Option<u64>,
    pub memory_stats_stats_v1_active_file: Option<u64>,
    pub memory_stats_stats_v1_pgfault: Option<u64>,
    pub memory_stats_stats_v1_inactive_file: Option<u64>,
    pub memory_stats_stats_v1_total_pgmajfault: Option<u64>,
    pub memory_stats_stats_v1_total_pgpgin: Option<u64>,
    pub memory_stats_stats_v1_hierarchical_memsw_limit: Option<u64>, // only on OSX
    pub memory_stats_stats_v1_shmem: Option<u64>, // only on linux kernel > 4.15.0-1106
    pub memory_stats_stats_v1_total_shmem: Option<u64>, // only on linux kernel > 4.15.0-1106

    // v2 memory stats
    pub memory_stats_stats_v2_anon: Option<u64>,
    pub memory_stats_stats_v2_file: Option<u64>,
    pub memory_stats_stats_v2_kernel_stack: Option<u64>,
    pub memory_stats_stats_v2_slab: Option<u64>,
    pub memory_stats_stats_v2_sock: Option<u64>,
    pub memory_stats_stats_v2_shmem: Option<u64>,
    pub memory_stats_stats_v2_file_mapped: Option<u64>,
    pub memory_stats_stats_v2_file_dirty: Option<u64>,
    pub memory_stats_stats_v2_file_writeback: Option<u64>,
    pub memory_stats_stats_v2_anon_thp: Option<u64>,
    pub memory_stats_stats_v2_inactive_anon: Option<u64>,
    pub memory_stats_stats_v2_active_anon: Option<u64>,
    pub memory_stats_stats_v2_inactive_file: Option<u64>,
    pub memory_stats_stats_v2_active_file: Option<u64>,
    pub memory_stats_stats_v2_unevictable: Option<u64>,
    pub memory_stats_stats_v2_slab_reclaimable: Option<u64>,
    pub memory_stats_stats_v2_slab_unreclaimable: Option<u64>,
    pub memory_stats_stats_v2_pgfault: Option<u64>,
    pub memory_stats_stats_v2_pgmajfault: Option<u64>,
    pub memory_stats_stats_v2_workingset_refault: Option<u64>,
    pub memory_stats_stats_v2_workingset_activate: Option<u64>,
    pub memory_stats_stats_v2_workingset_nodereclaim: Option<u64>,
    pub memory_stats_stats_v2_pgrefill: Option<u64>,
    pub memory_stats_stats_v2_pgscan: Option<u64>,
    pub memory_stats_stats_v2_pgsteal: Option<u64>,
    pub memory_stats_stats_v2_pgactivate: Option<u64>,
    pub memory_stats_stats_v2_pgdeactivate: Option<u64>,
    pub memory_stats_stats_v2_pglazyfree: Option<u64>,
    pub memory_stats_stats_v2_pglazyfreed: Option<u64>,
    pub memory_stats_stats_v2_thp_fault_alloc: Option<u64>,
    pub memory_stats_stats_v2_thp_collapse_alloc: Option<u64>,

    pub memory_stats_max_usage: Option<u64>,
    pub memory_stats_usage: Option<u64>,
    pub memory_stats_failcnt: Option<u64>,
    pub memory_stats_limit: Option<u64>,
    pub memory_stats_commit: Option<u64>,
    pub memory_stats_commit_peak: Option<u64>,
    pub memory_stats_commitbytes: Option<u64>,
    pub memory_stats_commitpeakbytes: Option<u64>,
    pub memory_stats_privateworkingset: Option<u64>,

    pub blkio_stats_index: u32,
    // per blkio_stats_index
    pub blkio_stats_io_service_bytes_recursive_major: Option<u64>,
    pub blkio_stats_io_service_bytes_recursive_minor: Option<u64>,
    pub blkio_stats_io_service_bytes_recursive_op: Option<String>,
    pub blkio_stats_io_service_bytes_recursive_value: Option<u64>,
    pub blkio_stats_io_serviced_recursive_major: Option<u64>,
    pub blkio_stats_io_serviced_recursive_minor: Option<u64>,
    pub blkio_stats_io_serviced_recursive_op: Option<String>,
    pub blkio_stats_io_serviced_recursive_value: Option<u64>,
    pub blkio_stats_io_queue_recursive_major: Option<u64>,
    pub blkio_stats_io_queue_recursive_minor: Option<u64>,
    pub blkio_stats_io_queue_recursive_op: Option<String>,
    pub blkio_stats_io_queue_recursive_value: Option<u64>,
    pub blkio_stats_io_service_time_recursive_major: Option<u64>,
    pub blkio_stats_io_service_time_recursive_minor: Option<u64>,
    pub blkio_stats_io_service_time_recursive_op: Option<String>,
    pub blkio_stats_io_service_time_recursive_value: Option<u64>,
    pub blkio_stats_io_wait_time_recursive_major: Option<u64>,
    pub blkio_stats_io_wait_time_recursive_minor: Option<u64>,
    pub blkio_stats_io_wait_time_recursive_op: Option<String>,
    pub blkio_stats_io_wait_time_recursive_value: Option<u64>,
    pub blkio_stats_io_merged_recursive_major: Option<u64>,
    pub blkio_stats_io_merged_recursive_minor: Option<u64>,
    pub blkio_stats_io_merged_recursive_op: Option<String>,
    pub blkio_stats_io_merged_recursive_value: Option<u64>,
    pub blkio_stats_io_time_recursive_major: Option<u64>,
    pub blkio_stats_io_time_recursive_minor: Option<u64>,
    pub blkio_stats_io_time_recursive_op: Option<String>,
    pub blkio_stats_io_time_recursive_value: Option<u64>,
    pub blkio_stats_sectors_recursive_major: Option<u64>,
    pub blkio_stats_sectors_recursive_minor: Option<u64>,
    pub blkio_stats_sectors_recursive_op: Option<String>,
    pub blkio_stats_sectors_recursive_value: Option<u64>,

    pub cpu_stats_cpu_usage_percpu_usage: Option<Vec<u64>>,
    pub cpu_stats_cpu_usage_usage_in_usermode: u64,
    pub cpu_stats_cpu_usage_total_usage: u64,
    pub cpu_stats_cpu_usage_usage_in_kernelmode: u64,

    pub cpu_stats_system_cpu_usage: Option<u64>,
    pub cpu_stats_online_cpus: Option<u64>,

    pub cpu_stats_throttling_data_periods: u64,
    pub cpu_stats_throttling_data_throttled_periods: u64,
    pub cpu_stats_throttling_data_throttled_time: u64,

    pub precpu_stats_cpu_usage_percpu_usage: Option<Vec<u64>>,
    pub precpu_stats_cpu_usage_usage_in_usermode: u64,
    pub precpu_stats_cpu_usage_total_usage: u64,
    pub precpu_stats_cpu_usage_usage_in_kernelmode: u64,

    pub precpu_stats_system_cpu_usage: Option<u64>,
    pub precpu_stats_online_cpus: Option<u64>,
    pub precpu_stats_throttling_data_periods: u64,
    pub precpu_stats_throttling_data_throttled_periods: u64,
    pub precpu_stats_throttling_data_throttled_time: u64,

    pub storage_stats_read_count_normalized: Option<u64>,
    pub storage_stats_read_size_bytes: Option<u64>,
    pub storage_stats_write_count_normalized: Option<u64>,
    pub storage_stats_write_size_bytes: Option<u64>,

    pub name: String,
    pub id: String,
}

impl Stats {
    fn from_bollard(stats: bollard::container::Stats) -> Vec<Stats> {
        let bollard::container::Stats {
            read,
            preread,
            num_procs,
            pids_stats,
            network,
            networks,
            memory_stats,
            blkio_stats,
            cpu_stats,
            precpu_stats,
            storage_stats,
            name,
            id,
        } = stats;

        let mut v = Vec::new();

        let memv1 = memory_stats.stats.and_then(|v| {
            if let MemoryStatsStats::V1(v1) = v {
                Some(v1)
            } else {
                None
            }
        });
        let memv2 = memory_stats.stats.and_then(|v| {
            if let MemoryStatsStats::V2(v2) = v {
                Some(v2)
            } else {
                None
            }
        });
        let stat = Stats {
            read,
            preread,
            num_procs,
            pids_stats_current: pids_stats.current,
            pids_stats_limit: pids_stats.limit,
            network_rx_dropped: network.map(|v| v.rx_dropped),
            network_rx_bytes: network.map(|v| v.rx_bytes),
            network_rx_errors: network.map(|v| v.rx_errors),
            network_rx_packets: network.map(|v| v.rx_packets),
            network_tx_packets: network.map(|v| v.tx_packets),
            network_tx_dropped: network.map(|v| v.tx_dropped),
            network_tx_errors: network.map(|v| v.tx_errors),
            network_tx_bytes: network.map(|v| v.tx_bytes),

            networks_name: todo!(),
            networks_rx_dropped: todo!(),
            networks_rx_bytes: todo!(),
            networks_rx_errors: todo!(),
            networks_rx_packets: todo!(),
            networks_tx_packets: todo!(),
            networks_tx_dropped: todo!(),
            networks_tx_errors: todo!(),
            networks_tx_bytes: todo!(),

            memory_stats_stats_v1_cache: memv1.map(|v| v.cache),
            memory_stats_stats_v1_dirty: memv1.map(|v| v.dirty),
            memory_stats_stats_v1_mapped_file: memv1.map(|v| v.mapped_file),
            memory_stats_stats_v1_total_inactive_file: memv1.map(|v| v.total_inactive_file),
            memory_stats_stats_v1_pgpgout: memv1.map(|v| v.pgpgout),
            memory_stats_stats_v1_rss: memv1.map(|v| v.rss),
            memory_stats_stats_v1_total_mapped_file: memv1.map(|v| v.total_mapped_file),
            memory_stats_stats_v1_writeback: memv1.map(|v| v.writeback),
            memory_stats_stats_v1_unevictable: memv1.map(|v| v.unevictable),
            memory_stats_stats_v1_pgpgin: memv1.map(|v| v.pgpgin),
            memory_stats_stats_v1_total_unevictable: memv1.map(|v| v.total_unevictable),
            memory_stats_stats_v1_pgmajfault: memv1.map(|v| v.pgmajfault),
            memory_stats_stats_v1_total_rss: memv1.map(|v| v.total_rss),
            memory_stats_stats_v1_total_rss_huge: memv1.map(|v| v.total_rss_huge),
            memory_stats_stats_v1_total_writeback: memv1.map(|v| v.total_writeback),
            memory_stats_stats_v1_total_inactive_anon: memv1.map(|v| v.total_inactive_anon),
            memory_stats_stats_v1_rss_huge: memv1.map(|v| v.rss_huge),
            memory_stats_stats_v1_hierarchical_memory_limit: memv1
                .map(|v| v.hierarchical_memory_limit),
            memory_stats_stats_v1_total_pgfault: memv1.map(|v| v.total_pgfault),
            memory_stats_stats_v1_total_active_file: memv1.map(|v| v.total_active_file),
            memory_stats_stats_v1_active_anon: memv1.map(|v| v.active_anon),
            memory_stats_stats_v1_total_active_anon: memv1.map(|v| v.total_active_anon),
            memory_stats_stats_v1_total_pgpgout: memv1.map(|v| v.total_pgpgout),
            memory_stats_stats_v1_total_cache: memv1.map(|v| v.total_cache),
            memory_stats_stats_v1_total_dirty: memv1.map(|v| v.total_dirty),
            memory_stats_stats_v1_inactive_anon: memv1.map(|v| v.inactive_anon),
            memory_stats_stats_v1_active_file: memv1.map(|v| v.active_file),
            memory_stats_stats_v1_pgfault: memv1.map(|v| v.pgfault),
            memory_stats_stats_v1_inactive_file: memv1.map(|v| v.inactive_file),
            memory_stats_stats_v1_total_pgmajfault: memv1.map(|v| v.total_pgmajfault),
            memory_stats_stats_v1_total_pgpgin: memv1.map(|v| v.total_pgpgin),
            memory_stats_stats_v1_hierarchical_memsw_limit: memv1
                .and_then(|v| v.hierarchical_memsw_limit),
            memory_stats_stats_v1_shmem: memv1.and_then(|v| v.shmem),
            memory_stats_stats_v1_total_shmem: memv1.and_then(|v| v.total_shmem),

            memory_stats_stats_v2_anon: memv2.map(|v| v.anon),
            memory_stats_stats_v2_file: memv2.map(|v| v.file),
            memory_stats_stats_v2_kernel_stack: memv2.map(|v| v.kernel_stack),
            memory_stats_stats_v2_slab: memv2.map(|v| v.slab),
            memory_stats_stats_v2_sock: memv2.map(|v| v.sock),
            memory_stats_stats_v2_shmem: memv2.map(|v| v.shmem),
            memory_stats_stats_v2_file_mapped: memv2.map(|v| v.file_mapped),
            memory_stats_stats_v2_file_dirty: memv2.map(|v| v.file_dirty),
            memory_stats_stats_v2_file_writeback: memv2.map(|v| v.file_writeback),
            memory_stats_stats_v2_anon_thp: memv2.map(|v| v.anon_thp),
            memory_stats_stats_v2_inactive_anon: memv2.map(|v| v.inactive_anon),
            memory_stats_stats_v2_active_anon: memv2.map(|v| v.active_anon),
            memory_stats_stats_v2_inactive_file: memv2.map(|v| v.inactive_file),
            memory_stats_stats_v2_active_file: memv2.map(|v| v.active_file),
            memory_stats_stats_v2_unevictable: memv2.map(|v| v.unevictable),
            memory_stats_stats_v2_slab_reclaimable: memv2.map(|v| v.slab_reclaimable),
            memory_stats_stats_v2_slab_unreclaimable: memv2.map(|v| v.slab_unreclaimable),
            memory_stats_stats_v2_pgfault: memv2.map(|v| v.pgfault),
            memory_stats_stats_v2_pgmajfault: memv2.map(|v| v.pgmajfault),
            memory_stats_stats_v2_workingset_refault: memv2.map(|v| v.workingset_refault),
            memory_stats_stats_v2_workingset_activate: memv2.map(|v| v.workingset_activate),
            memory_stats_stats_v2_workingset_nodereclaim: memv2.map(|v| v.workingset_nodereclaim),
            memory_stats_stats_v2_pgrefill: memv2.map(|v| v.pgrefill),
            memory_stats_stats_v2_pgscan: memv2.map(|v| v.pgscan),
            memory_stats_stats_v2_pgsteal: memv2.map(|v| v.pgsteal),
            memory_stats_stats_v2_pgactivate: memv2.map(|v| v.pgactivate),
            memory_stats_stats_v2_pgdeactivate: memv2.map(|v| v.pgdeactivate),
            memory_stats_stats_v2_pglazyfree: memv2.map(|v| v.pglazyfree),
            memory_stats_stats_v2_pglazyfreed: memv2.map(|v| v.pglazyfreed),
            memory_stats_stats_v2_thp_fault_alloc: memv2.map(|v| v.thp_fault_alloc),
            memory_stats_stats_v2_thp_collapse_alloc: memv2.map(|v| v.thp_collapse_alloc),

            memory_stats_max_usage: todo!(),
            memory_stats_usage: todo!(),
            memory_stats_failcnt: todo!(),
            memory_stats_limit: todo!(),
            memory_stats_commit: todo!(),
            memory_stats_commit_peak: todo!(),
            memory_stats_commitbytes: todo!(),
            memory_stats_commitpeakbytes: todo!(),
            memory_stats_privateworkingset: todo!(),

            blkio_stats_index: todo!(),
            blkio_stats_io_service_bytes_recursive_major: todo!(),
            blkio_stats_io_service_bytes_recursive_minor: todo!(),
            blkio_stats_io_service_bytes_recursive_op: todo!(),
            blkio_stats_io_service_bytes_recursive_value: todo!(),
            blkio_stats_io_serviced_recursive_major: todo!(),
            blkio_stats_io_serviced_recursive_minor: todo!(),
            blkio_stats_io_serviced_recursive_op: todo!(),
            blkio_stats_io_serviced_recursive_value: todo!(),
            blkio_stats_io_queue_recursive_major: todo!(),
            blkio_stats_io_queue_recursive_minor: todo!(),
            blkio_stats_io_queue_recursive_op: todo!(),
            blkio_stats_io_queue_recursive_value: todo!(),
            blkio_stats_io_service_time_recursive_major: todo!(),
            blkio_stats_io_service_time_recursive_minor: todo!(),
            blkio_stats_io_service_time_recursive_op: todo!(),
            blkio_stats_io_service_time_recursive_value: todo!(),
            blkio_stats_io_wait_time_recursive_major: todo!(),
            blkio_stats_io_wait_time_recursive_minor: todo!(),
            blkio_stats_io_wait_time_recursive_op: todo!(),
            blkio_stats_io_wait_time_recursive_value: todo!(),
            blkio_stats_io_merged_recursive_major: todo!(),
            blkio_stats_io_merged_recursive_minor: todo!(),
            blkio_stats_io_merged_recursive_op: todo!(),
            blkio_stats_io_merged_recursive_value: todo!(),
            blkio_stats_io_time_recursive_major: todo!(),
            blkio_stats_io_time_recursive_minor: todo!(),
            blkio_stats_io_time_recursive_op: todo!(),
            blkio_stats_io_time_recursive_value: todo!(),
            blkio_stats_sectors_recursive_major: todo!(),
            blkio_stats_sectors_recursive_minor: todo!(),
            blkio_stats_sectors_recursive_op: todo!(),
            blkio_stats_sectors_recursive_value: todo!(),

            cpu_stats_cpu_usage_percpu_usage: cpu_stats.cpu_usage.percpu_usage,
            cpu_stats_cpu_usage_usage_in_usermode: cpu_stats.cpu_usage.usage_in_usermode,
            cpu_stats_cpu_usage_total_usage: cpu_stats.cpu_usage.total_usage,
            cpu_stats_cpu_usage_usage_in_kernelmode: cpu_stats.cpu_usage.usage_in_kernelmode,
            cpu_stats_system_cpu_usage: cpu_stats.system_cpu_usage,
            cpu_stats_online_cpus: cpu_stats.online_cpus,
            cpu_stats_throttling_data_periods: cpu_stats.throttling_data.periods,
            cpu_stats_throttling_data_throttled_periods: cpu_stats
                .throttling_data
                .throttled_periods,
            cpu_stats_throttling_data_throttled_time: cpu_stats.throttling_data.throttled_time,

            precpu_stats_cpu_usage_percpu_usage: precpu_stats.cpu_usage.percpu_usage,
            precpu_stats_cpu_usage_usage_in_usermode: precpu_stats.cpu_usage.usage_in_usermode,
            precpu_stats_cpu_usage_total_usage: precpu_stats.cpu_usage.total_usage,
            precpu_stats_cpu_usage_usage_in_kernelmode: precpu_stats.cpu_usage.usage_in_kernelmode,

            precpu_stats_system_cpu_usage: precpu_stats.system_cpu_usage,
            precpu_stats_online_cpus: precpu_stats.online_cpus,
            precpu_stats_throttling_data_periods: precpu_stats.throttling_data.periods,
            precpu_stats_throttling_data_throttled_periods: precpu_stats
                .throttling_data
                .throttled_periods,
            precpu_stats_throttling_data_throttled_time: precpu_stats
                .throttling_data
                .throttled_time,

            storage_stats_read_count_normalized: storage_stats.read_count_normalized,
            storage_stats_read_size_bytes: storage_stats.read_size_bytes,
            storage_stats_write_count_normalized: storage_stats.write_count_normalized,
            storage_stats_write_size_bytes: storage_stats.write_size_bytes,
            name,
            id,
        };
        v.push(stat);

        v
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
    if !conf_path.exists() {
        debug!(path = ?conf_path, "Creating config directory");
        create_dir_all(&conf_path)?;
    }
    Ok(conf_path)
}

fn create_logs_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let logs_path = parent.join("logs");
    if !logs_path.exists() {
        debug!(path = ?logs_path, "Creating logs directory");
        create_dir_all(&logs_path)?;
    }
    Ok(logs_path)
}

fn create_metrics_dir(parent: &Path) -> Result<PathBuf, io::Error> {
    let metrics_path = parent.join("metrics");
    if !metrics_path.exists() {
        debug!(path = ?metrics_path, "Creating metrics directory");
        create_dir_all(&metrics_path)?;
    }
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
        let id = container.id.as_ref().unwrap();
        let name = &container
            .names
            .and_then(|names| names.first().cloned())
            .unwrap_or_default();
        debug!(?name, "Removing container");
        docker
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;
    }
    Ok(())
}
