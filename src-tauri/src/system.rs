use serde::Serialize;
use std::sync::{Arc, Mutex};
use sysinfo::{Disks, Networks, System};
use tauri::State;

#[derive(Serialize, Clone)]
pub struct SystemInfo {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub uptime: u64,
    pub cpu_usage: f32,
    pub cpu_cores: usize,
    pub per_cpu_usage: Vec<f32>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_usage: f32,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskInfo>,
    pub networks: Vec<NetworkInfo>,
}

#[derive(Serialize, Clone)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub kind: String,
    pub total: u64,
    pub available: u64,
    pub used: u64,
    pub usage: f32,
}

#[derive(Serialize, Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub received: u64,
    pub transmitted: u64,
}

pub struct SystemMonitor {
    sys: System,
    networks: Networks,
    disks: Disks,
    cache: SystemInfo,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        // 第一次采样，为 CPU 使用率建立基线
        sys.refresh_cpu_usage();
        let networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let cache = build_snapshot(&mut sys, &networks, &disks);
        Self {
            sys,
            networks,
            disks,
            cache,
        }
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.networks.refresh(false);
        self.disks.refresh(false);
        self.cache = build_snapshot(&mut self.sys, &self.networks, &self.disks);
    }

    pub fn snapshot(&self) -> SystemInfo {
        self.cache.clone()
    }
}

fn build_snapshot(sys: &mut System, networks: &Networks, disks: &Disks) -> SystemInfo {
    let cpu_usage = sys.global_cpu_usage();
    let per_cpu_usage: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();

    let memory_total = sys.total_memory();
    let memory_used = sys.used_memory();
    let memory_usage = if memory_total > 0 {
        memory_used as f32 / memory_total as f32 * 100.0
    } else {
        0.0
    };

    let disks_info: Vec<DiskInfo> = disks
        .list()
        .iter()
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            let used = total.saturating_sub(available);
            DiskInfo {
                name: d.name().to_string_lossy().to_string(),
                mount_point: d.mount_point().to_string_lossy().to_string(),
                file_system: d.file_system().to_string_lossy().to_string(),
                kind: format!("{:?}", d.kind()),
                total,
                available,
                used,
                usage: if total > 0 {
                    used as f32 / total as f32 * 100.0
                } else {
                    0.0
                },
            }
        })
        .collect();

    let networks_info: Vec<NetworkInfo> = networks
        .list()
        .iter()
        .map(|(name, data)| NetworkInfo {
            name: name.clone(),
            received: data.received(),
            transmitted: data.transmitted(),
        })
        .collect();

    SystemInfo {
        hostname: System::host_name().unwrap_or_default(),
        os_name: System::name().unwrap_or_default(),
        os_version: System::os_version().unwrap_or_default(),
        kernel_version: System::kernel_version().unwrap_or_default(),
        uptime: System::uptime(),
        cpu_usage,
        cpu_cores: sys.cpus().len(),
        per_cpu_usage,
        memory_total,
        memory_used,
        memory_usage,
        swap_total: sys.total_swap(),
        swap_used: sys.used_swap(),
        disks: disks_info,
        networks: networks_info,
    }
}

pub struct AppState {
    pub monitor: Arc<Mutex<SystemMonitor>>,
    pub config: Arc<Mutex<crate::config::StatusConfig>>,
}

#[tauri::command]
pub fn get_system_info(state: State<AppState>) -> SystemInfo {
    state.monitor.lock().unwrap().snapshot()
}
