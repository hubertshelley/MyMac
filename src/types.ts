export interface SystemInfo {
  hostname: string;
  os_name: string;
  os_version: string;
  kernel_version: string;
  uptime: number;
  cpu_usage: number;
  cpu_cores: number;
  per_cpu_usage: number[];
  memory_total: number;
  memory_used: number;
  memory_usage: number;
  swap_total: number;
  swap_used: number;
  disks: DiskInfo[];
  networks: NetworkInfo[];
}

export interface DiskInfo {
  name: string;
  mount_point: string;
  file_system: string;
  kind: string;
  total: number;
  available: number;
  used: number;
  usage: number;
}

export interface NetworkInfo {
  name: string;
  received: number;
  transmitted: number;
}

export interface AppInfo {
  id: string;
  name: string;
  path: string;
  version: string;
  size: number;
  is_system: boolean;
}

export interface LaunchItem {
  id: string;
  name: string;
  path: string;
  program: string;
  run_at_load: boolean;
  enabled: boolean;
  is_user: boolean;
  location: string;
}

export interface StatusConfig {
  show_logo: boolean;
  show_cpu: boolean;
  show_memory: boolean;
  show_disk: boolean;
}
