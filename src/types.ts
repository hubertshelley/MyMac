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
  net_down_rate: number;
  net_up_rate: number;
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
  show_network: boolean;
}

export interface AppRelatedItem {
  path: string;
  name: string;
  kind: string;
  size: number;
  is_app: boolean;
}

export type ClipKind = "text" | "image";

export interface ClipItem {
  id: string;
  content: string;
  created_at: string;
  kind: ClipKind;
  image_size: [number, number] | null;
  thumbnail: string | null;
}

export interface TotpAccount {
  id: string;
  name: string;
  issuer: string;
  digits: number;
  period: number;
  code: string;
  remaining: number;
}


export type BrewSource = "official" | "tsinghua" | "ustc";

export interface BrewStatus {
  installed: boolean;
  path: string;
  version: string;
  source: BrewSource;
}

export interface BrewPackage {
  name: string;
  version: string;
  kind: "formula" | "cask";
  installed: boolean;
  outdated: boolean;
  trusted: boolean;
  tap: string | null;
}

export interface BrewOperationResult {
  message: string;
  output: string;
}
