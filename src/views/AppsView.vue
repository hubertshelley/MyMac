<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Search, Trash2, Package } from "@lucide/vue";
import type { AppInfo } from "@/types";
import { formatBytes } from "@/lib/format";
import { cn } from "@/lib/utils";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";

const apps = ref<AppInfo[]>([]);
const loading = ref(true);
const search = ref("");
const message = ref("");
const error = ref("");

const colors = [
  "bg-blue-500",
  "bg-emerald-500",
  "bg-orange-500",
  "bg-purple-500",
  "bg-pink-500",
  "bg-teal-500",
  "bg-indigo-500",
  "bg-rose-500",
];

function appColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  }
  return colors[hash % colors.length];
}

async function load() {
  loading.value = true;
  apps.value = await invoke<AppInfo[]>("list_apps");
  loading.value = false;
}

onMounted(load);

const filtered = computed(() => {
  const q = search.value.trim().toLowerCase();
  if (!q) return apps.value;
  return apps.value.filter((a) => a.name.toLowerCase().includes(q));
});

async function uninstall(app: AppInfo) {
  if (!window.confirm(`确定要卸载「${app.name}」吗？它将移入废纸篓，可随时从废纸篓恢复。`)) {
    return;
  }
  try {
    await invoke("uninstall_app", { path: app.path });
    message.value = `已卸载「${app.name}」`;
    error.value = "";
    await load();
  } catch (e) {
    error.value = String(e);
    message.value = "";
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center gap-3">
      <div class="relative flex-1">
        <Search class="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <input
          v-model="search"
          type="text"
          placeholder="搜索应用…"
          class="h-9 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
        />
      </div>
      <span class="text-xs text-muted-foreground">共 {{ apps.length }} 个应用</span>
    </div>

    <div
      v-if="message"
      class="rounded-md bg-emerald-50 px-3 py-2 text-sm text-emerald-700 dark:bg-emerald-950 dark:text-emerald-300"
    >
      {{ message }}
    </div>
    <div
      v-if="error"
      class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700 dark:bg-red-950 dark:text-red-300"
    >
      {{ error }}
    </div>

    <Card v-if="loading" class="flex items-center justify-center gap-2 p-10 text-sm text-muted-foreground">
      <Package class="size-4 animate-pulse" /> 正在扫描应用…
    </Card>

    <Card v-else class="overflow-hidden">
      <ul class="divide-y divide-border">
        <li
          v-for="app in filtered"
          :key="app.id"
          class="flex items-center gap-3 px-4 py-3"
        >
          <div
            :class="
              cn(
                'flex size-10 shrink-0 items-center justify-center rounded-lg text-sm font-semibold text-white',
                appColor(app.name)
              )
            "
          >
            {{ app.name.charAt(0).toUpperCase() }}
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate text-sm font-medium">{{ app.name }}</span>
              <Badge v-if="app.is_system" variant="secondary">系统</Badge>
            </div>
            <div class="truncate text-xs text-muted-foreground">
              {{ app.version || "未知版本" }} · {{ formatBytes(app.size) }}
            </div>
          </div>
          <Button
            variant="destructive"
            size="sm"
            :disabled="app.is_system"
            @click="uninstall(app)"
          >
            <Trash2 /> 卸载
          </Button>
        </li>
      </ul>
      <div v-if="!filtered.length" class="p-10 text-center text-sm text-muted-foreground">
        未找到匹配的应用
      </div>
    </Card>
  </div>
</template>
