<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Clipboard, ClipboardX, Clock, Copy, Search, Trash2 } from "@lucide/vue";
import type { ClipItem } from "@/types";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";

const items = ref<ClipItem[]>([]);
const keyword = ref("");
let timer: number | undefined;
let unlisten: (() => void) | undefined;

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return items.value;
  return items.value.filter((i) => i.content.toLowerCase().includes(kw));
});

async function refresh() {
  items.value = await invoke<ClipItem[]>("get_clip_history");
}

async function copyItem(item: ClipItem) {
  try {
    await invoke("copy_clip_item", { id: item.id });
    await refresh();
  } catch (e) {
    window.alert(String(e));
  }
}

async function removeItem(item: ClipItem) {
  try {
    await invoke("delete_clip_item", { id: item.id });
    await refresh();
  } catch (e) {
    window.alert(String(e));
  }
}

async function clearAll() {
  if (!window.confirm("确定要清空全部粘贴板历史记录吗？")) return;
  try {
    await invoke("clear_clip_history");
    await refresh();
  } catch (e) {
    window.alert(String(e));
  }
}

onMounted(async () => {
  await refresh();
  timer = window.setInterval(refresh, 2000);
  unlisten = await listen("clip-history-changed", refresh);
});

onUnmounted(() => {
  if (timer) window.clearInterval(timer);
  unlisten?.();
});
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center gap-2">
      <div class="relative flex-1">
        <Search
          class="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <input
          v-model="keyword"
          placeholder="搜索粘贴板历史…"
          class="h-9 w-full rounded-md border border-input bg-background pl-8 pr-3 text-sm outline-none transition-colors focus:ring-2 focus:ring-ring"
        />
      </div>
      <Button variant="destructive" size="sm" :disabled="!items.length" @click="clearAll">
        <ClipboardX class="size-4" />
        清空记录
      </Button>
    </div>

    <Card class="overflow-hidden">
      <div
        v-if="!filtered.length"
        class="flex flex-col items-center gap-2 px-4 py-12 text-muted-foreground"
      >
        <Clipboard class="size-8" />
        <p class="text-sm">
          {{ items.length ? "没有匹配的记录" : "暂无粘贴板历史，复制任意文本后自动记录" }}
        </p>
      </div>
      <ul v-else class="divide-y divide-border">
        <li
          v-for="item in filtered"
          :key="item.id"
          class="group flex cursor-pointer items-start gap-3 px-4 py-3 transition-colors hover:bg-accent/50"
          @click="copyItem(item)"
        >
          <div class="min-w-0 flex-1">
            <p class="line-clamp-3 whitespace-pre-wrap break-all text-sm">
              {{ item.content }}
            </p>
            <div class="mt-1 flex items-center gap-1.5 text-xs text-muted-foreground">
              <Clock class="size-3" />
              <span>{{ item.created_at }}</span>
              <span>·</span>
              <span>{{ item.content.length }} 字符</span>
            </div>
          </div>
          <div
            class="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100"
          >
            <Button
              variant="ghost"
              size="icon"
              class="size-8"
              title="复制"
              @click.stop="copyItem(item)"
            >
              <Copy class="size-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              class="size-8 text-destructive hover:text-destructive"
              title="删除"
              @click.stop="removeItem(item)"
            >
              <Trash2 class="size-4" />
            </Button>
          </div>
        </li>
      </ul>
    </Card>

    <div class="rounded-md bg-muted px-3 py-2 text-xs text-muted-foreground">
      提示：点击记录即可复制回系统剪贴板；记录上限 200 条，应用退出后仍会保留。
    </div>
  </div>
</template>