<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Rocket, Lock, Search, Trash2 } from "@lucide/vue";
import type { LaunchItem } from "@/types";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Switch from "@/components/ui/Switch.vue";

const items = ref<LaunchItem[]>([]);
const loading = ref(true);
const search = ref("");

async function load() {
  loading.value = true;
  items.value = await invoke<LaunchItem[]>("list_launch_items");
  loading.value = false;
}

onMounted(load);

const filtered = computed(() => {
  const q = search.value.trim().toLowerCase();
  if (!q) return items.value;
  return items.value.filter(
    (i) =>
      i.name.toLowerCase().includes(q) ||
      i.program.toLowerCase().includes(q) ||
      i.location.toLowerCase().includes(q)
  );
});

async function toggle(item: LaunchItem, enabled: boolean) {
  try {
    await invoke("set_launch_item", { path: item.path, enabled });
    item.enabled = enabled;
  } catch (e) {
    window.alert(String(e));
  }
}

async function remove(item: LaunchItem) {
  if (!window.confirm(`确定要删除启动项「${item.name}」吗？它将移入废纸篓。`)) {
    return;
  }
  try {
    await invoke("delete_launch_item", { path: item.path });
    await load();
  } catch (e) {
    window.alert(String(e));
  }
}
</script>

<template>
  <div class="space-y-4">
    <p class="text-sm text-muted-foreground">
      管理登录时自动启动的项目。用户级登录项可开关或删除，系统级登录项需要管理员权限，仅支持查看。
    </p>

    <div class="flex items-center gap-3">
      <div class="relative flex-1">
        <Search class="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <input
          v-model="search"
          type="text"
          placeholder="搜索启动项…"
          class="h-9 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
        />
      </div>
      <span class="text-xs text-muted-foreground">共 {{ items.length }} 项</span>
    </div>

    <Card v-if="loading" class="flex items-center justify-center gap-2 p-10 text-sm text-muted-foreground">
      <Rocket class="size-4 animate-pulse" /> 正在读取启动项…
    </Card>

    <Card v-else class="overflow-hidden">
      <ul class="divide-y divide-border">
        <li
          v-for="item in filtered"
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
          <div v-if="item.is_user" class="flex items-center gap-3">
            <span class="text-xs text-muted-foreground">
              {{ item.enabled ? "已启用" : "已禁用" }}
            </span>
            <Switch
              :model-value="item.enabled"
              @update:model-value="(v) => toggle(item, v)"
            />
            <Button variant="ghost" size="icon" class="text-muted-foreground hover:text-destructive" @click="remove(item)">
              <Trash2 />
            </Button>
          </div>
          <div v-else class="flex items-center gap-1 text-xs text-muted-foreground">
            <Lock class="size-3.5" /> 仅查看
          </div>
        </li>
      </ul>
      <div v-if="!filtered.length" class="p-10 text-center text-sm text-muted-foreground">
        未找到匹配的启动项
      </div>
    </Card>
  </div>
</template>
