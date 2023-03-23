use std::time::Instant;
use std::{fs::File, path::Path, thread::sleep, time::Duration};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::Serialize;
use sysinfo::PidExt;
use sysinfo::Process;
use sysinfo::{Pid, ProcessExt, System, SystemExt};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessMonitorMeasurement {
    time: chrono::DateTime<chrono::Utc>,
    pid: u32,
    parent: u32,
    cpu_usage_percentage: f32,
    memory_usage_bytes: u64,
    virtual_memory_usage_bytes: u64,
    disk_bytes_written: u64,
    disk_bytes_read: u64,
    name: String,
}

/// Monitor a running process.
#[derive(Debug)]
pub struct ProcessMonitor {
    pid: Pid,
    writer: csv::Writer<File>,
    interval: Duration,
}

impl ProcessMonitor {
    pub fn new<P: AsRef<Path>>(pid: u32, filename: P, interval: Duration) -> Self {
        assert!(
            interval >= System::MINIMUM_CPU_UPDATE_INTERVAL,
            "process monitor refresh interval too low, should be above {:?} but was {:?}",
            System::MINIMUM_CPU_UPDATE_INTERVAL,
            interval
        );
        Self {
            pid: Pid::from_u32(pid),
            writer: csv::Writer::from_path(filename).unwrap(),
            interval,
        }
    }

    pub fn run(&mut self) {
        let mut sys = System::new_all();
        println!("running");
        loop {
            let loop_start = Instant::now();
            let time = Utc::now();
            sys.refresh_all();

            if let Some(process) = sys.process(self.pid) {
                println!("found process");
                self.write_process(time, self.pid, process)
            } else {
                println!("found no process");
                break;
            }

            self.writer.flush().unwrap();

            let loop_end = Instant::now();
            let loop_duration = loop_end - loop_start;
            if loop_duration < self.interval {
                let sleep_duration = self.interval - loop_duration;
                sleep(sleep_duration)
            }
        }
    }

    fn write_process(&mut self, time: DateTime<Utc>, pid: Pid, process: &Process) {
        let disk_usage = process.disk_usage();
        let measurement = ProcessMonitorMeasurement {
            time,
            pid: pid.as_u32(),
            parent: process.parent().unwrap().as_u32(),
            cpu_usage_percentage: process.cpu_usage(),
            memory_usage_bytes: process.memory(),
            virtual_memory_usage_bytes: process.virtual_memory(),
            disk_bytes_written: disk_usage.written_bytes,
            disk_bytes_read: disk_usage.read_bytes,
            name: process.name().to_owned(),
        };
        self.writer.serialize(measurement).unwrap();
        for (pid, process) in &process.tasks {
            self.write_process(time, *pid, process);
        }
    }
}
