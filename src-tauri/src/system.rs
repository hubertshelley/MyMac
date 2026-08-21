use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
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
    pub net_down_rate: f64,
    pub net_up_rate: f64,
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
    last_net: HashMap<String, (u64, u64)>,
    last_time: Option<Instant>,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        // 第一次采样，为 CPU 使用率建立基线
        sys.refresh_cpu_usage();
        let networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();

        let last_net: HashMap<String, (u64, u64)> = networks
            .list()
            .iter()
            .map(|(name, data)| (name.clone(), (data.received(), data.transmitted())))
            .collect();
        let last_time = Some(Instant::now());

        let cache = build_snapshot(&mut sys, &networks, &disks, 0.0, 0.0);
        Self {
            sys,
            networks,
            disks,
            cache,
            last_net,
            last_time,
        }
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.networks.refresh(false);
        self.disks.refresh(false);

        // 计算网络速率（bytes/s）
        let now = Instant::now();
        let elapsed = self
            .last_time
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(1.0)
            .max(0.1);
        let mut down = 0.0f64;
        let mut up = 0.0f64;
        let mut new_last: HashMap<String, (u64, u64)> = HashMap::new();
        for (name, data) in self.networks.list() {
            let cur = (data.received(), data.transmitted());
            if let Some(&(lr, lt)) = self.last_net.get(name) {
                down += cur.0.saturating_sub(lr) as f64 / elapsed;
                up += cur.1.saturating_sub(lt) as f64 / elapsed;
            }
            new_last.insert(name.clone(), cur);
        }
        self.last_net = new_last;
        self.last_time = Some(now);

        self.cache = build_snapshot(&mut self.sys, &self.networks, &self.disks, down, up);
    }

    pub fn snapshot(&self) -> SystemInfo {
        self.cache.clone()
    }
}

fn build_snapshot(
    sys: &mut System,
    networks: &Networks,
    disks: &Disks,
    net_down_rate: f64,
    net_up_rate: f64,
) -> SystemInfo {
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
        net_down_rate,
        net_up_rate,
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
