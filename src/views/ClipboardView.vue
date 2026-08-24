<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Clipboard,
  ClipboardX,
  Clock,
  Copy,
  Image as ImageIcon,
  Maximize2,
  Search,
  Trash2,
} from "@lucide/vue";
import type { ClipItem } from "@/types";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";

const items = ref<ClipItem[]>([]);
const keyword = ref("");
const preview = ref<string | null>(null);
const toast = ref("");
let timer: number | undefined;
let unlisten: (() => void) | undefined;
let toastTimer: number | undefined;

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return items.value;
  return items.value.filter((i) => {
    if (i.kind === "text") return i.content.toLowerCase().includes(kw);
    return kw === "图片" || kw === "image" || kw === "img";
  });
});

function showToast(msg: string) {
  toast.value = msg;
  if (toastTimer) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.value = ""), 2000);
}

async function refresh() {
  items.value = await invoke<ClipItem[]>("get_clip_history");
}

async function copyItem(item: ClipItem) {
  try {
    await invoke("copy_clip_item", { id: item.id });
    await refresh();
    showToast(item.kind === "text" ? "已复制到剪贴板" : "图片已复制到剪贴板");
  } catch (e) {
    window.alert(String(e));
  }
}

async function viewImage(item: ClipItem) {
  try {
    preview.value = await invoke<string>("get_clip_image", { id: item.id });
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
    showToast("已清空全部记录");
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
  if (toastTimer) window.clearTimeout(toastTimer);
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
          {{
            items.length
              ? "没有匹配的记录"
              : "暂无粘贴板历史，复制文本或图片后自动记录"
          }}
        </p>
      </div>
      <ul v-else class="divide-y divide-border">
        <li
          v-for="item in filtered"
          :key="item.id"
          class="group flex cursor-pointer items-start gap-3 px-4 py-3 transition-colors hover:bg-accent/50"
          @click="copyItem(item)"
        >
          <!-- 文本记录 -->
          <template v-if="item.kind === 'text'">
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
          </template>

          <!-- 图片记录 -->
          <template v-else>
            <div class="min-w-0 flex-1">
              <div class="flex items-start gap-3">
                <img
                  :src="item.thumbnail ?? undefined"
                  alt="剪贴板图片"
                  class="h-24 w-auto max-w-[180px] shrink-0 rounded-md border border-border object-cover"
                  draggable="false"
                  @click.stop="viewImage(item)"
                />
                <div class="min-w-0">
                  <div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                    <ImageIcon class="size-3.5" />
                    <span>{{ item.image_size ? `${item.image_size[0]}×${item.image_size[1]}` : "图片" }}</span>
                  </div>
                  <div class="mt-1 flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Clock class="size-3" />
                    <span>{{ item.created_at }}</span>
                  </div>
                </div>
              </div>
            </div>
          </template>

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
              v-if="item.kind === 'image'"
              variant="ghost"
              size="icon"
              class="size-8"
              title="查看原图"
              @click.stop="viewImage(item)"
            >
              <Maximize2 class="size-4" />
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
      提示：点击记录即可复制回系统剪贴板；图片记录点击缩略图可查看原图。记录上限 200 条，应用退出后仍会保留。
    </div>

    <!-- 复制成功提示 -->
    <Transition
      enter-active-class="transition-opacity duration-200"
      enter-from-class="opacity-0"
      leave-active-class="transition-opacity duration-200"
      leave-to-class="opacity-0"
    >
      <div
        v-if="toast"
        class="fixed bottom-6 left-1/2 z-40 -translate-x-1/2 rounded-md bg-foreground px-4 py-2 text-sm text-background shadow-lg"
      >
        {{ toast }}
      </div>
    </Transition>

    <!-- 原图预览 -->
    <Transition
      enter-active-class="transition-opacity duration-150"
      enter-from-class="opacity-0"
      leave-active-class="transition-opacity duration-150"
      leave-to-class="opacity-0"
    >
      <div
        v-if="preview"
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6"
        @click="preview = null"
      >
        <img
          :src="preview"
          alt="原图预览"
          class="max-h-[85vh] max-w-[85vw] rounded-lg bg-white object-contain shadow-2xl"
          @click.stop
        />
        <Button
          variant="secondary"
          size="sm"
          class="absolute right-6 top-6"
          @click="preview = null"
        >
          关闭
        </Button>
      </div>
    </Transition>
  </div>
</template>