<script setup lang="ts">
import { ref, onMounted, type Component } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Gauge, Cpu, MemoryStick, HardDrive, Wifi } from "@lucide/vue";
import type { StatusConfig } from "@/types";
import Card from "@/components/ui/Card.vue";
import Switch from "@/components/ui/Switch.vue";

const config = ref<StatusConfig>({
  show_logo: true,
  show_cpu: true,
  show_memory: true,
  show_disk: false,
  show_network: false,
});

async function load() {
  config.value = await invoke<StatusConfig>("get_status_config");
}

onMounted(load);

async function update(key: keyof StatusConfig, value: boolean) {
  config.value[key] = value;
  try {
    await invoke("set_status_config", { config: config.value });
  } catch (e) {
    window.alert(String(e));
    await load();
  }
}

const items: { key: keyof StatusConfig; label: string; desc: string; icon: Component }[] = [
  { key: "show_logo", label: "Logo 图形", desc: "在数据前显示彩色 Logo", icon: Gauge },
  { key: "show_cpu", label: "CPU 占用", desc: "显示处理器使用率", icon: Cpu },
  { key: "show_memory", label: "内存占用", desc: "显示内存使用率", icon: MemoryStick },
  { key: "show_disk", label: "磁盘占用", desc: "显示磁盘使用率", icon: HardDrive },
  { key: "show_network", label: "网络速率", desc: "显示实时下载速度", icon: Wifi },
];
</script>

<template>
  <div class="space-y-4">
    <p class="text-sm text-muted-foreground">
      自定义菜单栏状态栏的显示内容。修改后立即生效，并会保存到本地。
    </p>

    <Card class="overflow-hidden">
      <ul class="divide-y divide-border">
        <li
          v-for="item in items"
          :key="item.key"
          class="flex items-center gap-3 px-4 py-3"
        >
          <div
            class="flex size-9 shrink-0 items-center justify-center rounded-md bg-secondary"
          >
            <component :is="item.icon" class="size-5 text-muted-foreground" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="text-sm font-medium">{{ item.label }}</div>
            <div class="text-xs text-muted-foreground">{{ item.desc }}</div>
          </div>
          <Switch
            :model-value="config[item.key]"
            @update:model-value="(v) => update(item.key, v)"
          />
        </li>
      </ul>
    </Card>

    <div class="rounded-md bg-muted px-3 py-2 text-xs text-muted-foreground">
      提示：状态栏为彩色 Logo 加单色指标图标，图标与数字会跟随菜单栏深浅色主题自动切换。
    </div>
  </div>
</template>
