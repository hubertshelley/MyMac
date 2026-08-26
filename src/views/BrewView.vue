<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  Download,
  ChevronDown,
  ChevronRight,
  LoaderCircle,
  Package,
  RefreshCw,
  Search,
  Settings2,
  Trash2,
  Upload,
} from "@lucide/vue";
import type {
  BrewOperationResult,
  BrewPackage,
  BrewSource,
  BrewStatus,
} from "@/types";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";

const status = ref<BrewStatus | null>(null);
const packages = ref<BrewPackage[]>([]);
const results = ref<BrewPackage[]>([]);
const query = ref("");
const loading = ref(true);
const busy = ref("");
const message = ref("");
const error = ref("");
const output = ref("");
const section = ref<"installed" | "search">("installed");

interface BrewTreeNode extends BrewPackage {
  nodeId: string;
  depth: number;
  expanded: boolean;
  loadingChildren: boolean;
  childrenLoaded: boolean;
  children: BrewTreeNode[];
}

const treeRoots = ref<BrewTreeNode[]>([]);

function createTreeNode(item: BrewPackage, depth: number, parentId = "root"): BrewTreeNode {
  return {
    ...item,
    nodeId: `${parentId}/${item.kind}:${item.name}`,
    depth,
    expanded: false,
    loadingChildren: false,
    childrenLoaded: item.kind === "cask",
    children: [],
  };
}

const visibleInstalledNodes = computed(() => {
  const visible: BrewTreeNode[] = [];
  const append = (nodes: BrewTreeNode[]) => {
    for (const node of nodes) {
      visible.push(node);
      if (node.expanded) append(node.children);
    }
  };
  append(treeRoots.value);
  return visible;
});

const sourceOptions: { value: BrewSource; label: string }[] = [
  { value: "official", label: "官方源" },
  { value: "tsinghua", label: "清华大学" },
  { value: "ustc", label: "中科大" },
];

const outdatedCount = computed(() => packages.value.filter((item) => item.outdated).length);

function showError(value: unknown) {
  error.value = String(value);
  message.value = "";
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    status.value = await invoke<BrewStatus>("get_brew_status");
    packages.value = status.value.installed
      ? await invoke<BrewPackage[]>("list_brew_packages")
      : [];
    treeRoots.value = packages.value
      .filter((item) => item.top_level)
      .map((item) => createTreeNode(item, 0));
  } catch (e) {
    showError(e);
  } finally {
    loading.value = false;
  }
}

onMounted(load);

async function installBrew() {
  busy.value = "install-brew";
  try {
    message.value = await invoke<string>("start_brew_install");
    error.value = "";
  } catch (e) {
    showError(e);
  } finally {
    busy.value = "";
  }
}

async function changeSource(event: Event) {
  const source = (event.target as HTMLSelectElement).value as BrewSource;
  if (!status.value || source === status.value.source) return;
  busy.value = "source";
  try {
    message.value = await invoke<string>("set_brew_source", { source });
    status.value.source = source;
    error.value = "";
  } catch (e) {
    showError(e);
    (event.target as HTMLSelectElement).value = status.value.source;
  } finally {
    busy.value = "";
  }
}

async function search() {
  if (query.value.trim().length < 2) {
    error.value = "请输入至少 2 个字符的软件名称";
    return;
  }
  busy.value = "search";
  error.value = "";
  try {
    results.value = await invoke<BrewPackage[]>("search_brew_packages", {
      query: query.value,
    });
    section.value = "search";
  } catch (e) {
    showError(e);
  } finally {
    busy.value = "";
  }
}

async function toggleDependencies(node: BrewTreeNode) {
  if (node.kind === "cask") return;
  if (node.childrenLoaded) {
    node.expanded = !node.expanded;
    return;
  }
  node.loadingChildren = true;
  error.value = "";
  try {
    const dependencies = await invoke<BrewPackage[]>("get_brew_dependencies", {
      name: node.name,
      kind: node.kind,
    });
    node.children = dependencies.map((item) =>
      createTreeNode(item, node.depth + 1, node.nodeId)
    );
    node.childrenLoaded = true;
    node.expanded = node.children.length > 0;
  } catch (e) {
    showError(e);
  } finally {
    node.loadingChildren = false;
  }
}

async function runPackageAction(action: "install" | "uninstall" | "upgrade", item: BrewPackage) {
  if (action === "uninstall" && !window.confirm(`确定要卸载「${item.name}」吗？`)) return;
  const command = `${action}_brew_package`;
  busy.value = `${action}:${item.kind}:${item.name}`;
  try {
    const result = await invoke<BrewOperationResult>(command, {
      name: item.name,
      kind: item.kind,
    });
    message.value = result.message;
    output.value = result.output;
    error.value = "";
    await load();
    if (section.value === "search") await search();
  } catch (e) {
    showError(e);
  } finally {
    busy.value = "";
  }
}

async function upgradeAll() {
  if (!window.confirm("将更新 Homebrew 索引和全部过期软件，是否继续？")) return;
  busy.value = "upgrade-all";
  try {
    const result = await invoke<BrewOperationResult>("upgrade_all_brew_packages");
    message.value = result.message;
    output.value = result.output;
    error.value = "";
    await load();
  } catch (e) {
    showError(e);
  } finally {
    busy.value = "";
  }
}

function kindLabel(kind: BrewPackage["kind"]) {
  return kind === "cask" ? "图形应用" : "命令行";
}
</script>

<template>
  <div class="space-y-4">
    <Card v-if="loading" class="flex items-center justify-center gap-2 p-12 text-sm text-muted-foreground">
      <LoaderCircle class="size-4 animate-spin" /> 正在读取 Homebrew 信息…
    </Card>

    <template v-else-if="status">
      <Card v-if="!status.installed" class="p-6">
        <div class="flex items-start gap-4">
          <div class="flex size-11 shrink-0 items-center justify-center rounded-lg bg-amber-100 text-amber-700">
            <Package class="size-6" />
          </div>
          <div class="flex-1">
            <h2 class="font-semibold">尚未安装 Homebrew</h2>
            <p class="mt-1 text-sm text-muted-foreground">
              Homebrew 是 macOS 常用的软件管理工具。安装过程将在系统终端中进行，以便你查看输出并确认管理员授权。
            </p>
            <div class="mt-4 flex gap-2">
              <Button :disabled="!!busy" @click="installBrew">
                <LoaderCircle v-if="busy === 'install-brew'" class="animate-spin" />
                <Download v-else /> 开始安装
              </Button>
              <Button variant="outline" :disabled="!!busy" @click="load">
                <RefreshCw /> 安装完成，刷新状态
              </Button>
            </div>
          </div>
        </div>
      </Card>

      <template v-else>
        <Card class="p-4">
          <div class="flex flex-wrap items-center gap-4">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="font-semibold">Homebrew 已就绪</span>
                <Badge>{{ status.version || "版本未知" }}</Badge>
              </div>
              <p class="mt-1 truncate text-xs text-muted-foreground">{{ status.path }}</p>
            </div>
            <label class="flex items-center gap-2 text-sm">
              <Settings2 class="size-4 text-muted-foreground" /> 软件源
              <select
                :value="status.source"
                class="h-9 rounded-md border border-input bg-background px-3 text-sm outline-none focus:ring-2 focus:ring-ring"
                :disabled="!!busy"
                @change="changeSource"
              >
                <option v-for="option in sourceOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </option>
              </select>
            </label>
            <Button variant="outline" :disabled="!!busy" @click="load"><RefreshCw /> 刷新</Button>
            <Button :disabled="!!busy" @click="upgradeAll">
              <LoaderCircle v-if="busy === 'upgrade-all'" class="animate-spin" />
              <Upload v-else /> 更新全部
              <Badge v-if="outdatedCount" variant="secondary">{{ outdatedCount }}</Badge>
            </Button>
          </div>
        </Card>

        <div class="flex gap-2 border-b">
          <button
            class="border-b-2 px-3 py-2 text-sm font-medium"
            :class="section === 'installed' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground'"
            @click="section = 'installed'"
          >
            已安装（{{ treeRoots.length }} 个顶层软件）
          </button>
          <button
            class="border-b-2 px-3 py-2 text-sm font-medium"
            :class="section === 'search' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground'"
            @click="section = 'search'"
          >
            搜索软件
          </button>
        </div>

        <form v-if="section === 'search'" class="flex gap-2" @submit.prevent="search">
          <div class="relative flex-1">
            <Search class="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <input
              v-model="query"
              type="search"
              placeholder="搜索命令行软件和图形应用…"
              class="h-9 w-full rounded-md border border-input bg-background pl-9 pr-3 text-sm outline-none focus:ring-2 focus:ring-ring"
            />
          </div>
          <Button type="submit" :disabled="!!busy">
            <LoaderCircle v-if="busy === 'search'" class="animate-spin" />
            <Search v-else /> 搜索
          </Button>
        </form>

        <div v-if="message" class="rounded-md bg-emerald-50 px-3 py-2 text-sm text-emerald-700">{{ message }}</div>
        <div v-if="error" class="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">{{ error }}</div>
        <details v-if="output" class="rounded-md border bg-muted/30 px-3 py-2 text-xs">
          <summary class="cursor-pointer text-muted-foreground">查看最近一次操作输出</summary>
          <pre class="mt-2 max-h-48 overflow-auto whitespace-pre-wrap">{{ output }}</pre>
        </details>

        <Card class="overflow-hidden">
          <ul class="divide-y divide-border">
            <li
              v-for="item in section === 'installed' ? visibleInstalledNodes : results"
              :key="section === 'installed' ? (item as BrewTreeNode).nodeId : `${item.kind}:${item.name}`"
              class="flex items-center gap-3 px-4 py-3"
              :style="section === 'installed' ? { paddingLeft: `${16 + (item as BrewTreeNode).depth * 24}px` } : undefined"
            >
              <button
                v-if="section === 'installed' && item.kind === 'formula' && (!(item as BrewTreeNode).childrenLoaded || (item as BrewTreeNode).children.length > 0)"
                type="button"
                class="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-muted"
                :disabled="(item as BrewTreeNode).loadingChildren"
                :title="(item as BrewTreeNode).expanded ? '收起依赖' : '展开直接依赖'"
                @click="toggleDependencies(item as BrewTreeNode)"
              >
                <LoaderCircle v-if="(item as BrewTreeNode).loadingChildren" class="size-4 animate-spin" />
                <ChevronDown v-else-if="(item as BrewTreeNode).expanded" class="size-4" />
                <ChevronRight v-else class="size-4" />
              </button>
              <span v-else-if="section === 'installed'" class="size-6 shrink-0" />
              <div class="flex size-9 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
                <Package class="size-4" />
              </div>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate text-sm font-medium">{{ item.name }}</span>
                  <Badge variant="outline">{{ kindLabel(item.kind) }}</Badge>
                  <Badge v-if="section === 'installed' && !(item as BrewTreeNode).top_level" variant="secondary">依赖</Badge>
                  <Badge v-if="!item.trusted" variant="destructive" :title="item.tap ? `来源：${item.tap}` : undefined">
                    来源未信任
                  </Badge>
                  <Badge v-if="item.outdated" variant="secondary">可更新</Badge>
                  <Badge v-else-if="item.installed && section === 'search'" variant="secondary">已安装</Badge>
                </div>
                <p v-if="item.version" class="text-xs text-muted-foreground">{{ item.version }}</p>
                <p v-if="!item.trusted" class="text-xs text-amber-700">
                  {{ item.tap || "第三方来源" }} 尚未获得 Homebrew 信任，无法读取更新信息
                </p>
              </div>
              <template v-if="item.installed">
                <Button
                  v-if="item.outdated"
                  variant="outline"
                  size="sm"
                  :disabled="!!busy"
                  @click="runPackageAction('upgrade', item)"
                >
                  <LoaderCircle v-if="busy === `upgrade:${item.kind}:${item.name}`" class="animate-spin" />
                  <Upload v-else /> 更新
                </Button>
                <Button
                  variant="destructive"
                  size="sm"
                  :disabled="!!busy"
                  @click="runPackageAction('uninstall', item)"
                >
                  <LoaderCircle v-if="busy === `uninstall:${item.kind}:${item.name}`" class="animate-spin" />
                  <Trash2 v-else /> 卸载
                </Button>
              </template>
              <Button v-else size="sm" :disabled="!!busy" @click="runPackageAction('install', item)">
                <LoaderCircle v-if="busy === `install:${item.kind}:${item.name}`" class="animate-spin" />
                <Download v-else /> 安装
              </Button>
            </li>
          </ul>
          <div
            v-if="!(section === 'installed' ? visibleInstalledNodes : results).length"
            class="p-10 text-center text-sm text-muted-foreground"
          >
            {{ section === "installed" ? "暂无已安装软件" : "输入名称搜索软件" }}
          </div>
        </Card>
      </template>
    </template>
  </div>
</template>
