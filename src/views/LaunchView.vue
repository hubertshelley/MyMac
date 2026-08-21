<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Rocket, Lock } from "@lucide/vue";
import type { LaunchItem } from "@/types";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Switch from "@/components/ui/Switch.vue";

const items = ref<LaunchItem[]>([]);
const loading = ref(true);

async function load() {
  loading.value = true;
  items.value = await invoke<LaunchItem[]>("list_launch_items");
  loading.value = false;
}

onMounted(load);

async function toggle(item: LaunchItem, enabled: boolean) {
  try {
    await invoke("set_launch_item", { path: item.path, enabled });
    item.enabled = enabled;
  } catch (e) {
    window.alert(String(e));
  }
}
</script>

<template>
  <div class="space-y-4">
    <p class="text-sm text-muted-foreground">
      管理登录时自动启动的项目。用户级登录项可随时开关，系统级登录项需要管理员权限，仅支持查看。
    </p>

    <Card v-if="loading" class="flex items-center justify-center gap-2 p-10 text-sm text-muted-foreground">
      <Rocket class="size-4 animate-pulse" /> 正在读取启动项…
    </Card>

    <Card v-else class="overflow-hidden">
      <ul class="divide-y divide-border">
        <li
          v-for="item in items"
          :key="item.id"
          class="flex items-center gap-3 px-4 py-3"
        >
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate text-sm font-medium">{{ item.name }}</span>
              <Badge variant="outline">{{ item.location }}</Badge>
            </div>
            <div class="truncate text-xs text-muted-foreground">{{ item.program || item.path }}</div>
          </div>
          <div v-if="item.is_user" class="flex items-center gap-2">
            <span class="text-xs text-muted-foreground">
              {{ item.enabled ? "已启用" : "已禁用" }}
            </span>
            <Switch
              :model-value="item.enabled"
              @update:model-value="(v) => toggle(item, v)"
            />
          </div>
          <div v-else class="flex items-center gap-1 text-xs text-muted-foreground">
            <Lock class="size-3.5" /> 仅查看
          </div>
        </li>
      </ul>
      <div v-if="!items.length" class="p-10 text-center text-sm text-muted-foreground">
        未找到启动项
      </div>
    </Card>
  </div>
</template>
