<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Search, Trash2, Package, X, LoaderCircle } from "@lucide/vue";
import type { AppInfo, AppRelatedItem } from "@/types";
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
const selectedApp = ref<AppInfo | null>(null);
const relatedLoading = ref(false);
const relatedItems = ref<AppRelatedItem[]>([]);
const selectedPaths = ref(new Set<string>());
const deleting = ref(false);

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
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  return colors[hash % colors.length];
}

async function load() {
  loading.value = true;
  apps.value = await invoke<AppInfo[]>("list_apps");
  loading.value = false;
}

let unlistenSize: (() => void) | undefined;
onMounted(async () => {
  // 先监听再加载，避免小应用的大小事件在监听建立前丢失。
  unlistenSize = await listen<AppInfo>("app-size", (event) => {
    const target = apps.value.find((app) => app.id === event.payload.id);
    if (target) target.size = event.payload.size;
  });
  await load();
});

onUnmounted(() => unlistenSize?.());

const filtered = computed(() => {
  const query = search.value.trim().toLocaleLowerCase();
  if (!query) return apps.value;
  return apps.value.filter(
    (app) =>
      app.name.toLocaleLowerCase().includes(query) ||
      app.path.toLocaleLowerCase().includes(query)
  );
});

const selectedSize = computed(() =>
  relatedItems.value
    .filter((item) => selectedPaths.value.has(item.path))
    .reduce((total, item) => total + item.size, 0)
);

async function prepareUninstall(app: AppInfo) {
  selectedApp.value = app;
  relatedItems.value = [];
  selectedPaths.value = new Set();
  relatedLoading.value = true;
  error.value = "";
  try {
    relatedItems.value = await invoke<AppRelatedItem[]>("scan_app_related", { app });
    // 应用本体默认选中，关联项由用户主动勾选。
    selectedPaths.value = new Set(
      relatedItems.value.filter((item) => item.is_app).map((item) => item.path)
    );
  } catch (e) {
    error.value = String(e);
    selectedApp.value = null;
  } finally {
    relatedLoading.value = false;
  }
}

function toggleRelated(path: string, checked: boolean) {
  const next = new Set(selectedPaths.value);
  if (checked) next.add(path);
  else next.delete(path);
  selectedPaths.value = next;
}

function closeDialog() {
  if (!deleting.value) selectedApp.value = null;
}

async function confirmUninstall() {
  if (!selectedApp.value || selectedPaths.value.size === 0) return;
  const appName = selectedApp.value.name;
  deleting.value = true;
  try {
    await invoke("uninstall_app_items", { paths: [...selectedPaths.value] });
    message.value = `已处理「${appName}」选中的项目`;
    error.value = "";
    selectedApp.value = null;
    await load();
  } catch (e) {
    error.value = String(e);
    message.value = "";
  } finally {
    deleting.value = false;
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
          type="search"
          placeholder="按名称或路径搜索应用…"
          class="h-9 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
        />
      </div>
      <span class="text-xs text-muted-foreground">共 {{ apps.length }} 个可卸载应用</span>
    </div>

    <div v-if="message" class="rounded-md bg-emerald-50 px-3 py-2 text-sm text-emerald-700">
      {{ message }}
    </div>
    <div v-if="error" class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">
      {{ error }}
    </div>

    <Card v-if="loading" class="flex items-center justify-center gap-2 p-10 text-sm text-muted-foreground">
      <Package class="size-4 animate-pulse" /> 正在扫描应用…
    </Card>

    <Card v-else class="overflow-hidden">
      <ul class="divide-y divide-border">
        <li v-for="app in filtered" :key="app.id" class="flex items-center gap-3 px-4 py-3">
          <div :class="cn('flex size-10 shrink-0 items-center justify-center rounded-lg text-sm font-semibold text-white', appColor(app.name))">
            {{ app.name.charAt(0).toUpperCase() }}
          </div>
          <div class="min-w-0 flex-1">
            <span class="truncate text-sm font-medium">{{ app.name }}</span>
            <div class="truncate text-xs text-muted-foreground">
              {{ app.version || "未知版本" }} · {{ app.size > 0 ? formatBytes(app.size) : "计算中…" }}
            </div>
          </div>
          <Button variant="destructive" size="sm" @click="prepareUninstall(app)">
            <Trash2 /> 卸载
          </Button>
        </li>
      </ul>
      <div v-if="!filtered.length" class="p-10 text-center text-sm text-muted-foreground">
        未找到匹配的应用
      </div>
    </Card>

    <!-- 卸载项目选择弹窗 -->
    <div v-if="selectedApp" class="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6" @click.self="closeDialog">
      <Card class="flex max-h-[80vh] w-full max-w-2xl flex-col overflow-hidden shadow-xl">
        <div class="flex items-start justify-between border-b p-5">
          <div>
            <h2 class="text-lg font-semibold">卸载 {{ selectedApp.name }}</h2>
            <p class="mt-1 text-sm text-muted-foreground">请选择要移入废纸篓的项目。应用本体已默认选中。</p>
          </div>
          <Button variant="ghost" size="icon" @click="closeDialog"><X /></Button>
        </div>

        <div v-if="relatedLoading" class="flex items-center justify-center gap-2 p-12 text-sm text-muted-foreground">
          <LoaderCircle class="size-4 animate-spin" /> 正在查找关联项…
        </div>
        <div v-else class="overflow-y-auto p-3">
          <label
            v-for="item in relatedItems"
            :key="item.path"
            class="flex cursor-pointer items-start gap-3 rounded-md p-3 hover:bg-muted"
          >
            <input
              type="checkbox"
              class="mt-1 size-4 accent-primary"
              :checked="selectedPaths.has(item.path)"
              @change="toggleRelated(item.path, ($event.target as HTMLInputElement).checked)"
            />
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="truncate text-sm font-medium">{{ item.name }}</span>
                <Badge :variant="item.is_app ? 'default' : 'outline'">{{ item.kind }}</Badge>
              </div>
              <div class="truncate text-xs text-muted-foreground">{{ item.path }}</div>
            </div>
            <span class="whitespace-nowrap text-xs text-muted-foreground">{{ formatBytes(item.size) }}</span>
          </label>
        </div>

        <div class="flex items-center justify-between border-t p-4">
          <span class="text-sm text-muted-foreground">
            已选 {{ selectedPaths.size }} 项，共 {{ formatBytes(selectedSize) }}
          </span>
          <div class="flex gap-2">
            <Button variant="outline" :disabled="deleting" @click="closeDialog">取消</Button>
            <Button variant="destructive" :disabled="deleting || selectedPaths.size === 0" @click="confirmUninstall">
              <LoaderCircle v-if="deleting" class="animate-spin" />
              <Trash2 v-else />
              移入废纸篓
            </Button>
          </div>
        </div>
      </Card>
    </div>
  </div>
</template>
