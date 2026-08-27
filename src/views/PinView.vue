<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { X } from "@lucide/vue";

interface PinContext {
  imageUrl: string;
  width: number;
  height: number;
}

const context = ref<PinContext | null>(null);
const src = ref("");
/** 相对原始逻辑尺寸的显示比例 */
const zoom = ref(1);

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
  try {
    await invoke("close_pin_window");
  } catch (error) {
    console.error("关闭贴图失败", error);
  }
}

function onKeyDown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    void close();
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
  </div>
</template>
