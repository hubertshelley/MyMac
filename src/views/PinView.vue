<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { ClipboardCopy, X } from "@lucide/vue";

interface PinContext {
  imageUrl: string;
  width: number;
  height: number;
}

const context = ref<PinContext | null>(null);
const src = ref("");
/** 相对原始逻辑尺寸的显示比例 */
const zoom = ref(1);
/** 复制成功提示 */
const copied = ref(false);
let copiedTimer: ReturnType<typeof setTimeout> | undefined;

const contextMenu = reactive({ visible: false, x: 0, y: 0 });

let disposed = false;

onMounted(async () => {
  try {
    context.value = await invoke<PinContext>("get_pin_context");
    src.value = convertFileSrc(context.value.imageUrl);
    // 初始比例与窗口实际大小一致（Rust 端创建时已限制最大尺寸）
    zoom.value = window.innerWidth / Math.max(1, context.value.width);
  } catch (error) {
    console.error("加载贴图失败", error);
    await close();
  }
  window.addEventListener("keydown", onKeyDown);
});

onBeforeUnmount(() => {
  disposed = true;
  window.removeEventListener("keydown", onKeyDown);
  if (copiedTimer) clearTimeout(copiedTimer);
});

const displaySize = computed(() => {
  if (!context.value) return { width: 0, height: 0 };
  return {
    width: Math.max(1, Math.round(context.value.width * zoom.value)),
    height: Math.max(1, Math.round(context.value.height * zoom.value)),
  };
});

async function resizeWindow() {
  if (!context.value) return;
  try {
    await invoke("resize_pin_window", {
      width: displaySize.value.width,
      height: displaySize.value.height,
    });
  } catch (error) {
    console.error("调整贴图窗口失败", error);
  }
}

function onWheel(event: WheelEvent) {
  event.preventDefault();
  if (!context.value) return;
  const factor = event.deltaY < 0 ? 1.1 : 1 / 1.1;
  const next = Math.min(6, Math.max(0.12, zoom.value * factor));
  if (Math.abs(next - zoom.value) < 0.001) return;
  zoom.value = next;
  void resizeWindow();
}

async function close() {
  if (disposed) return;
  disposed = true;
  contextMenu.visible = false;
  try {
    await invoke("close_pin_window");
  } catch (error) {
    console.error("关闭贴图失败", error);
  }
}

function onKeyDown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    if (contextMenu.visible) {
      contextMenu.visible = false;
      return;
    }
    void close();
  }
}

// ---------------------------------------------------------------------------
// 右键菜单
// ---------------------------------------------------------------------------

function onContextMenu(event: MouseEvent) {
  event.preventDefault();
  const menuW = 150;
  const menuH = 84;
  contextMenu.x = Math.min(event.clientX, window.innerWidth - menuW - 4);
  contextMenu.y = Math.min(event.clientY, window.innerHeight - menuH - 4);
  contextMenu.visible = true;
}

function closeContextMenu() {
  contextMenu.visible = false;
}

async function copyToClipboard() {
  closeContextMenu();
  try {
    await invoke("copy_pin_to_clipboard");
    copied.value = true;
    if (copiedTimer) clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copied.value = false), 1500);
  } catch (error) {
    console.error("复制贴图失败", error);
  }
}
</script>

<template>
  <div
    v-if="context"
    class="group relative h-screen w-screen overflow-hidden"
    data-tauri-drag-region
    @wheel="onWheel"
    @dblclick="close"
    @contextmenu="onContextMenu"
    @mousedown="closeContextMenu"
  >
    <img
      :src="src"
      alt=""
      draggable="false"
      class="pointer-events-none absolute left-0 top-0 max-w-none"
      :style="{ width: `${displaySize.width}px`, height: `${displaySize.height}px` }"
    />
    <button
      title="关闭贴图"
      class="absolute right-1 top-1 z-10 hidden size-6 items-center justify-center rounded-full bg-black/60 text-white opacity-0 transition-opacity hover:bg-black/80 group-hover:flex group-hover:opacity-100"
      @click="close"
    >
      <X class="size-3.5" />
    </button>

    <!-- 复制成功提示 -->
    <div
      v-if="copied"
      class="absolute left-1/2 top-3 z-20 -translate-x-1/2 rounded-md bg-black/75 px-3 py-1.5 text-xs text-white"
    >
      已复制到粘贴板
    </div>

    <!-- 右键菜单 -->
    <div
      v-if="contextMenu.visible"
      class="absolute z-30 flex flex-col rounded-lg border border-white/10 bg-neutral-900/95 py-1 shadow-xl backdrop-blur"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px`, minWidth: '150px' }"
      @mousedown.stop
      @contextmenu.prevent.stop
    >
      <button
        class="flex items-center gap-2 px-3 py-1.5 text-left text-xs text-neutral-200 transition-colors hover:bg-white/10"
        @click="copyToClipboard"
      >
        <ClipboardCopy class="size-3.5" />
        复制到粘贴板
      </button>
      <button
        class="flex items-center gap-2 px-3 py-1.5 text-left text-xs text-neutral-200 transition-colors hover:bg-white/10"
        @click="close"
      >
        <X class="size-3.5" />
        关闭贴图
      </button>
    </div>
  </div>
</template>
