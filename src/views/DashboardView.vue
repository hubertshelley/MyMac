<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Cpu, MemoryStick, HardDrive, Wifi, Server, Timer } from "@lucide/vue";
import type { SystemInfo } from "@/types";
import { formatBytes, formatUptime } from "@/lib/format";
import Card from "@/components/ui/Card.vue";
import Progress from "@/components/ui/Progress.vue";

const info = ref<SystemInfo | null>(null);
let timer: number | undefined;

async function refresh() {
  info.value = await invoke<SystemInfo>("get_system_info");
}

onMounted(() => {
  refresh();
  timer = window.setInterval(refresh, 2000);
});

onUnmounted(() => {
  if (timer) window.clearInterval(timer);
});
</script>

<template>
  <div class="space-y-4">
    <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <!-- CPU -->
      <Card class="p-5">
        <div class="mb-3 flex items-center gap-2">
          <Cpu class="size-4 text-muted-foreground" />
          <h3 class="text-sm font-medium">处理器</h3>
        </div>
        <div class="flex items-end justify-between">
          <span class="text-3xl font-semibold tracking-tight">
            {{ info?.cpu_usage.toFixed(1) ?? "--" }}%
          </span>
          <span class="text-xs text-muted-foreground">
            {{ info?.cpu_cores ?? "--" }} 核
          </span>
        </div>
        <Progress :value="info?.cpu_usage ?? 0" class="mt-3" />
        <div v-if="info && info.per_cpu_usage.length" class="mt-4 grid grid-cols-4 gap-2">
          <div
            v-for="(u, i) in info.per_cpu_usage"
            :key="i"
            class="rounded-md bg-secondary/60 p-2 text-center"
          >
            <div class="text-xs font-medium">{{ u.toFixed(0) }}%</div>
            <div class="text-[10px] text-muted-foreground">核 {{ i + 1 }}</div>
          </div>
        </div>
      </Card>

      <!-- 内存 -->
      <Card class="p-5">
        <div class="mb-3 flex items-center gap-2">
          <MemoryStick class="size-4 text-muted-foreground" />
          <h3 class="text-sm font-medium">内存</h3>
        </div>
        <div class="flex items-end justify-between">
          <span class="text-3xl font-semibold tracking-tight">
            {{ info?.memory_usage.toFixed(1) ?? "--" }}%
          </span>
          <span class="text-xs text-muted-foreground">
            {{ formatBytes(info?.memory_used ?? 0) }} / {{ formatBytes(info?.memory_total ?? 0) }}
          </span>
        </div>
        <Progress :value="info?.memory_usage ?? 0" class="mt-3" />
        <div class="mt-4 text-xs text-muted-foreground">
          交换分区：{{ formatBytes(info?.swap_used ?? 0) }} / {{ formatBytes(info?.swap_total ?? 0) }}
        </div>
      </Card>
    </div>

    <!-- 磁盘 -->
    <Card class="p-5">
      <div class="mb-3 flex items-center gap-2">
        <HardDrive class="size-4 text-muted-foreground" />
        <h3 class="text-sm font-medium">磁盘</h3>
      </div>
      <div class="space-y-4">
        <div v-for="d in info?.disks ?? []" :key="d.mount_point">
          <div class="mb-1 flex items-center justify-between text-sm">
            <div class="flex items-center gap-2">
              <span class="font-medium">{{ d.name }}</span>
              <span class="text-xs text-muted-foreground">{{ d.mount_point }}</span>
            </div>
            <span class="text-xs text-muted-foreground">
              {{ d.usage.toFixed(1) }}% · {{ formatBytes(d.used) }} / {{ formatBytes(d.total) }}
            </span>
          </div>
          <Progress :value="d.usage" />
        </div>
      </div>
    </Card>

    <!-- 网络 + 系统信息 -->
    <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <Card class="p-5">
        <div class="mb-3 flex items-center gap-2">
          <Wifi class="size-4 text-muted-foreground" />
          <h3 class="text-sm font-medium">网络</h3>
        </div>
        <div class="space-y-2">
          <div
            v-for="n in info?.networks ?? []"
            :key="n.name"
            class="flex items-center justify-between text-sm"
          >
            <span class="font-medium">{{ n.name }}</span>
            <span class="text-xs text-muted-foreground">
              ↓ {{ formatBytes(n.received) }} · ↑ {{ formatBytes(n.transmitted) }}
            </span>
          </div>
        </div>
      </Card>

      <Card class="p-5">
        <div class="mb-3 flex items-center gap-2">
          <Server class="size-4 text-muted-foreground" />
          <h3 class="text-sm font-medium">系统信息</h3>
        </div>
        <dl class="space-y-2 text-sm">
          <div class="flex justify-between">
            <dt class="text-muted-foreground">主机名</dt>
            <dd>{{ info?.hostname ?? "--" }}</dd>
          </div>
          <div class="flex justify-between">
            <dt class="text-muted-foreground">系统</dt>
            <dd>{{ info?.os_name ?? "--" }} {{ info?.os_version ?? "" }}</dd>
          </div>
          <div class="flex justify-between">
            <dt class="text-muted-foreground">内核</dt>
            <dd class="max-w-[60%] truncate text-right">{{ info?.kernel_version ?? "--" }}</dd>
          </div>
          <div class="flex items-center justify-between">
            <dt class="flex items-center gap-1 text-muted-foreground">
              <Timer class="size-3.5" /> 运行时间
            </dt>
            <dd>{{ info ? formatUptime(info.uptime) : "--" }}</dd>
          </div>
        </dl>
      </Card>
    </div>
  </div>
</template>
