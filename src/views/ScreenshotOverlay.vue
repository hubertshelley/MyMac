<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { ArrowUpRight, Copy, Download, Pin, Square, Type, Undo2, X } from "@lucide/vue";

interface OverlayDisplay {
  displayId: number;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  imageUrl: string;
}

interface CaptureWindowInfo {
  windowId: number;
  x: number;
  y: number;
  width: number;
  height: number;
  title: string;
  owner: string;
}

interface Selection {
  x: number;
  y: number;
  w: number;
  h: number;
}

type ToolKind = "none" | "rect" | "arrow" | "text";
type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

interface Shape {
  kind: Exclude<ToolKind, "none">;
  /** 相对选区左上角的坐标 */
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  color: string;
  lineWidth: number;
  fontSize: number;
  text?: string;
}

const COLORS = ["#ef4444", "#f59e0b", "#22c55e", "#3b82f6", "#ffffff", "#111111"];
const WIDTHS = [2, 4, 6];
const FONT_SIZES = [14, 18, 24];
const HANDLE_SIZE = 16;
const display = ref<OverlayDisplay>({
  displayId: 0,
  x: 0,
  y: 0,
  width: window.innerWidth,
  height: window.innerHeight,
  scale: 1,
  imageUrl: "",
});
const windows = ref<CaptureWindowInfo[]>([]);
const ready = ref(false);

const phase = ref<"idle" | "edit">("idle");
const hoverWindow = ref<CaptureWindowInfo | null>(null);
const selection = ref<Selection | null>(null);
const activeTool = ref<ToolKind>("none");
const color = ref(COLORS[0]);
const strokeWidth = ref(2);
const fontSize = ref(18);
const shapes = ref<Shape[]>([]);
const drawingShape = ref<Shape | null>(null);
const tip = ref("");
let tipTimer: ReturnType<typeof setTimeout> | undefined;

const bgImage = ref<HTMLImageElement | null>(null);
const bgSrc = ref("");
const canvasEl = ref<HTMLCanvasElement | null>(null);
const toolbarEl = ref<HTMLDivElement | null>(null);
const toolbarSize = reactive({ width: 460, height: 46 });

const textInput = reactive({
  active: false,
  x: 0,
  y: 0,
  value: "",
});

type DragState =
  | { mode: "creating"; startX: number; startY: number }
  | { mode: "moving"; startX: number; startY: number; orig: Selection }
  | { mode: "resizing"; handle: Handle; startX: number; startY: number; orig: Selection };

let drag: DragState | null = null;

function toolClass(active: boolean): string {
  return [
    "flex size-7 items-center justify-center rounded-md transition-colors",
    active ? "bg-blue-500 text-white" : "text-neutral-300 hover:bg-white/10 hover:text-white",
  ].join(" ");
}

const actionClass =
  "flex size-7 items-center justify-center rounded-md text-neutral-300 transition-colors hover:bg-white/10 hover:text-white";
const primaryActionClass =
  "flex size-7 items-center justify-center rounded-md bg-blue-500 text-white transition-colors hover:bg-blue-400";

// ---------------------------------------------------------------------------
// 初始化
// ---------------------------------------------------------------------------

onMounted(async () => {
  try {
    const context = await invoke<{ sessionId: string; display: OverlayDisplay; windows: CaptureWindowInfo[] }>(
      "get_capture_context",
    );
    display.value = context.display;
    windows.value = context.windows;
    bgSrc.value = convertFileSrc(context.display.imageUrl);
    ready.value = true;
    await nextTick();
    render();
  } catch (error) {
    showTip(`初始化截图失败：${error}`);
  }
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("blur", onWindowBlur);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeyDown, true);
  window.removeEventListener("blur", onWindowBlur);
});

function onWindowBlur() {
  // 失焦时清除悬停态，避免残留高亮
  hoverWindow.value = null;
  render();
}

// ---------------------------------------------------------------------------
// 坐标与几何辅助
// ---------------------------------------------------------------------------

/** 全局窗口矩形转为相对本屏坐标 */
function toLocalRect(win: CaptureWindowInfo): Selection {
  return {
    x: win.x - display.value.x,
    y: win.y - display.value.y,
    w: win.width,
    h: win.height,
  };
}

function contains(rect: Selection, px: number, py: number): boolean {
  return px >= rect.x && px <= rect.x + rect.w && py >= rect.y && py <= rect.y + rect.h;
}

function hitWindow(px: number, py: number): CaptureWindowInfo | null {
  let best: CaptureWindowInfo | null = null;
  let bestArea = Number.POSITIVE_INFINITY;
  for (const win of windows.value) {
    if (!contains(toLocalRect(win), px, py)) continue;
    const area = win.width * win.height;
    if (area < bestArea) {
      bestArea = area;
      best = win;
    }
  }
  return best;
}

function clampSelection(sel: Selection): Selection {
  const d = display.value;
  const w = Math.min(Math.max(2, sel.w), d.width);
  const h = Math.min(Math.max(2, sel.h), d.height);
  const x = Math.min(Math.max(-w + 20, sel.x), d.width - 20);
  const y = Math.min(Math.max(-h + 20, sel.y), d.height - 20);
  return { x, y, w, h };
}

function handlePoints(sel: Selection): Array<{ handle: Handle; cx: number; cy: number }> {
  const { x, y, w, h } = sel;
  const midX = x + w / 2;
  const midY = y + h / 2;
  return [
    { handle: "nw", cx: x, cy: y },
    { handle: "n", cx: midX, cy: y },
    { handle: "ne", cx: x + w, cy: y },
    { handle: "e", cx: x + w, cy: midY },
    { handle: "se", cx: x + w, cy: y + h },
    { handle: "s", cx: midX, cy: y + h },
    { handle: "sw", cx: x, cy: y + h },
    { handle: "w", cx: x, cy: midY },
  ];
}

function hitHandle(px: number, py: number): Handle | null {
  if (!selection.value) return null;
  for (const point of handlePoints(selection.value)) {
    if (Math.abs(px - point.cx) <= HANDLE_SIZE / 2 && Math.abs(py - point.cy) <= HANDLE_SIZE / 2) {
      return point.handle;
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// 鼠标交互
// ---------------------------------------------------------------------------

function onMouseDown(event: MouseEvent) {
  if (event.button !== 0) return;
  const px = event.clientX;
  const py = event.clientY;

  // 正在输入文本时，点击输入框以外提交
  if (textInput.active) {
    commitTextInput();
    return;
  }

  if (phase.value === "idle") {
    drag = { mode: "creating", startX: px, startY: py };
    selection.value = { x: px, y: py, w: 0, h: 0 };
    render();
    return;
  }

  // 编辑态
  if (activeTool.value === "text") {
    if (selection.value && contains(selection.value, px, py)) {
      // 阻止 mousedown 的默认焦点转移，否则输入框刚聚焦就被夺走
      event.preventDefault();
      openTextInput(px, py);
    }
    return;
  }

  if (activeTool.value === "rect" || activeTool.value === "arrow") {
    if (selection.value && contains(selection.value, px, py)) {
      drawingShape.value = {
        kind: activeTool.value,
        x1: px - selection.value.x,
        y1: py - selection.value.y,
        x2: px - selection.value.x,
        y2: py - selection.value.y,
        color: color.value,
        lineWidth: strokeWidth.value,
        fontSize: fontSize.value,
      };
      render();
      return;
    }
  }

  const handle = hitHandle(px, py);
  if (handle && selection.value) {
    drag = { mode: "resizing", handle, startX: px, startY: py, orig: { ...selection.value } };
    return;
  }

  if (selection.value && contains(selection.value, px, py)) {
    drag = { mode: "moving", startX: px, startY: py, orig: { ...selection.value } };
    return;
  }

  // 选区外重新框选
  shapes.value = [];
  activeTool.value = "none";
  phase.value = "idle";
  drag = { mode: "creating", startX: px, startY: py };
  selection.value = { x: px, y: py, w: 0, h: 0 };
  render();
}

function onMouseMove(event: MouseEvent) {
  const px = event.clientX;
  const py = event.clientY;

  // 正在绘制标注：更新终点
  if (drawingShape.value && selection.value) {
    drawingShape.value = {
      ...drawingShape.value,
      x2: px - selection.value.x,
      y2: py - selection.value.y,
    };
    render();
    return;
  }

  if (!drag) {
    updateCursor(px, py);
    if (phase.value === "idle") {
      const hit = hitWindow(px, py);
      if (hit?.windowId !== hoverWindow.value?.windowId) {
        hoverWindow.value = hit;
        render();
      }
    }
    return;
  }

  if (drag.mode === "creating") {
    selection.value = clampSelection({
      x: Math.min(drag.startX, px),
      y: Math.min(drag.startY, py),
      w: Math.abs(px - drag.startX),
      h: Math.abs(py - drag.startY),
    });
    hoverWindow.value = null;
    render();
    return;
  }

  if (drag.mode === "moving") {
    const dx = px - drag.startX;
    const dy = py - drag.startY;
    selection.value = clampSelection({
      x: drag.orig.x + dx,
      y: drag.orig.y + dy,
      w: drag.orig.w,
      h: drag.orig.h,
    });
    render();
    return;
  }

  if (drag.mode === "resizing") {
    const dx = px - drag.startX;
    const dy = py - drag.startY;
    const o = drag.orig;
    let { x, y, w, h } = o;
    if (drag.handle.includes("w")) {
      x = o.x + dx;
      w = o.w - dx;
    }
    if (drag.handle.includes("e")) {
      w = o.w + dx;
    }
    if (drag.handle.includes("n")) {
      y = o.y + dy;
      h = o.h - dy;
    }
    if (drag.handle.includes("s")) {
      h = o.h + dy;
    }
    if (w < 10) w = 10;
    if (h < 10) h = 10;
    selection.value = clampSelection({ x, y, w, h });
    render();
  }
}

function onMouseUp(event: MouseEvent) {
  // 完成标注绘制
  if (drawingShape.value) {
    const shape = drawingShape.value;
    drawingShape.value = null;
    if (Math.abs(shape.x2 - shape.x1) > 3 || Math.abs(shape.y2 - shape.y1) > 3) {
      shapes.value.push(shape);
    }
    render();
    return;
  }
  if (!drag) return;
  const state = drag;
  drag = null;

  if (state.mode === "creating") {
    const sel = selection.value;
    if (!sel) return;
    // 拖拽距离过小视为单击：选中悬停窗口
    if (sel.w < 6 || sel.h < 6) {
      const hit = hitWindow(event.clientX, event.clientY);
      if (hit) {
        selection.value = clampSelection(toLocalRect(hit));
        enterEdit();
      } else {
        selection.value = null;
        render();
      }
      return;
    }
    enterEdit();
  }
}

function enterEdit() {
  phase.value = "edit";
  hoverWindow.value = null;
  activeTool.value = "none";
  shapes.value = [];
  render();
  // 工具栏渲染后测量实际尺寸，用于定位夹取
  nextTick(measureToolbar);
}

function measureToolbar() {
  const el = toolbarEl.value;
  if (el) {
    toolbarSize.width = el.offsetWidth || toolbarSize.width;
    toolbarSize.height = el.offsetHeight || toolbarSize.height;
  }
}

function updateCursor(px: number, py: number) {
  let cursor = "crosshair";
  if (phase.value === "edit" && selection.value) {
    const handle = hitHandle(px, py);
    if (handle) {
      cursor = `${handle}-resize`;
    } else if (contains(selection.value, px, py)) {
      cursor = activeTool.value === "none" ? "move" : "crosshair";
    }
  }
  document.body.style.cursor = cursor;
}

// ---------------------------------------------------------------------------
// 文本标注输入
// ---------------------------------------------------------------------------

function openTextInput(px: number, py: number) {
  textInput.active = true;
  textInput.x = px - (selection.value?.x ?? 0);
  textInput.y = py - (selection.value?.y ?? 0);
  textInput.value = "";
  nextTick(() => {
    const input = document.getElementById("annotation-text-input") as HTMLInputElement | null;
    input?.focus();
  });
}

function commitTextInput() {
  const value = textInput.value.trim();
  textInput.active = false;
  textInput.value = "";
  if (value && selection.value) {
    shapes.value.push({
      kind: "text",
      x1: textInput.x,
      y1: textInput.y,
      x2: textInput.x,
      y2: textInput.y,
      color: color.value,
      lineWidth: strokeWidth.value,
      fontSize: fontSize.value,
      text: value,
    });
    render();
  }
}

// ---------------------------------------------------------------------------
// Canvas 渲染
// ---------------------------------------------------------------------------

function drawShape(ctx: CanvasRenderingContext2D, shape: Shape, offsetX: number, offsetY: number) {
  ctx.save();
  ctx.strokeStyle = shape.color;
  ctx.fillStyle = shape.color;
  ctx.lineWidth = shape.lineWidth;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  const x1 = shape.x1 + offsetX;
  const y1 = shape.y1 + offsetY;
  const x2 = shape.x2 + offsetX;
  const y2 = shape.y2 + offsetY;

  if (shape.kind === "rect") {
    ctx.strokeRect(Math.min(x1, x2), Math.min(y1, y2), Math.abs(x2 - x1), Math.abs(y2 - y1));
  } else if (shape.kind === "arrow") {
    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
    ctx.stroke();

    const angle = Math.atan2(y2 - y1, x2 - x1);
    const headLength = Math.max(14, shape.lineWidth * 4.5);
    ctx.beginPath();
    ctx.moveTo(x2, y2);
    ctx.lineTo(
      x2 - headLength * Math.cos(angle - Math.PI / 7),
      y2 - headLength * Math.sin(angle - Math.PI / 7),
    );
    ctx.lineTo(
      x2 - headLength * Math.cos(angle + Math.PI / 7),
      y2 - headLength * Math.sin(angle + Math.PI / 7),
    );
    ctx.closePath();
    ctx.fill();
  } else if (shape.kind === "text") {
    ctx.font = `${shape.fontSize}px -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif`;
    ctx.textBaseline = "top";
    ctx.fillText(shape.text ?? "", x1, y1);
  }
  ctx.restore();
}

function render() {
  const canvas = canvasEl.value;
  if (!canvas) return;
  const dpr = display.value.scale > 1 ? 2 : 1; // 画布按 2x 渲染保证清晰
  const cssW = display.value.width;
  const cssH = display.value.height;
  if (canvas.width !== cssW * dpr || canvas.height !== cssH * dpr) {
    canvas.width = cssW * dpr;
    canvas.height = cssH * dpr;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssW, cssH);

  // 半透明遮罩
  ctx.fillStyle = "rgba(0, 0, 0, 0.32)";
  ctx.fillRect(0, 0, cssW, cssH);

  const sel = selection.value;

  if (!sel) {
    const hover = hoverWindow.value;
    if (hover) {
      const rect = toLocalRect(hover);
      ctx.fillStyle = "rgba(250, 204, 21, 0.10)";
      ctx.fillRect(rect.x, rect.y, rect.w, rect.h);
      ctx.strokeStyle = "#facc15";
      ctx.lineWidth = 2;
      ctx.strokeRect(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2);
    }
    return;
  }

  // 挖空选区并描边
  ctx.clearRect(sel.x, sel.y, sel.w, sel.h);
  ctx.strokeStyle = "rgba(255, 255, 255, 0.95)";
  ctx.lineWidth = 1;
  ctx.strokeRect(sel.x + 0.5, sel.y + 0.5, sel.w - 1, sel.h - 1);

  // 四角加粗角标
  ctx.strokeStyle = "#3b82f6";
  ctx.lineWidth = 3;
  const corner = Math.min(22, sel.w / 3, sel.h / 3);
  const corners: Array<[number, number, number, number, number, number]> = [
    [sel.x, sel.y + corner, sel.x, sel.y, sel.x + corner, sel.y],
    [sel.x + sel.w - corner, sel.y, sel.x + sel.w, sel.y, sel.x + sel.w, sel.y + corner],
    [sel.x + sel.w, sel.y + sel.h - corner, sel.x + sel.w, sel.y + sel.h, sel.x + sel.w - corner, sel.y + sel.h],
    [sel.x + corner, sel.y + sel.h, sel.x, sel.y + sel.h, sel.x, sel.y + sel.h - corner],
  ];
  for (const [ax, ay, bx, by, cx, cy] of corners) {
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
    ctx.lineTo(cx, cy);
    ctx.stroke();
  }

  // 尺寸提示
  const label = `${Math.round(sel.w)} × ${Math.round(sel.h)}`;
  ctx.font = '12px -apple-system, "PingFang SC", sans-serif';
  const metrics = ctx.measureText(label);
  const padX = 8;
  const boxW = metrics.width + padX * 2;
  const boxH = 22;
  let labelY = sel.y - boxH - 6;
  if (labelY < 4) labelY = sel.y + 6;
  const labelX = Math.min(Math.max(sel.x, 4), cssW - boxW - 4);
  ctx.fillStyle = "rgba(59, 130, 246, 0.92)";
  ctx.beginPath();
  ctx.roundRect(labelX, labelY, boxW, boxH, 4);
  ctx.fill();
  ctx.fillStyle = "#ffffff";
  ctx.textBaseline = "middle";
  ctx.fillText(label, labelX + padX, labelY + boxH / 2 + 1);

  // 编辑态控制点
  if (phase.value === "edit") {
    for (const point of handlePoints(sel)) {
      ctx.fillStyle = "#ffffff";
      ctx.strokeStyle = "#3b82f6";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.rect(point.cx - 4, point.cy - 4, 8, 8);
      ctx.fill();
      ctx.stroke();
    }
  }

  // 标注图形
  for (const shape of shapes.value) {
    drawShape(ctx, shape, sel.x, sel.y);
  }
  if (drawingShape.value) {
    drawShape(ctx, drawingShape.value, sel.x, sel.y);
  }
}

// ---------------------------------------------------------------------------
// 工具栏动作
// ---------------------------------------------------------------------------

function setTool(tool: ToolKind) {
  activeTool.value = tool;
  updateCursor(-100, -100);
}

function undo() {
  if (shapes.value.length > 0) {
    shapes.value.pop();
    render();
  }
}

function cancelSession() {
  invoke("cancel_screenshot").catch((error) => showTip(`取消失败：${error}`));
}

async function exportBase64(): Promise<string> {
  const sel = selection.value;
  const img = bgImage.value;
  if (!sel || !img) throw new Error("没有可导出的内容");
  const scale = display.value.scale;
  const outW = Math.max(1, Math.round(sel.w * scale));
  const outH = Math.max(1, Math.round(sel.h * scale));
  const canvas = document.createElement("canvas");
  canvas.width = outW;
  canvas.height = outH;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("无法创建导出画布");
  ctx.drawImage(img, sel.x * scale, sel.y * scale, sel.w * scale, sel.h * scale, 0, 0, outW, outH);
  ctx.scale(scale, scale);
  for (const shape of shapes.value) {
    drawShape(ctx, shape, -sel.x, -sel.y);
  }
  const blob = await new Promise<Blob>((resolve, reject) => {
    canvas.toBlob((item) => (item ? resolve(item) : reject(new Error("图片编码失败"))), "image/png");
  });
  return await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      resolve(result.slice(result.indexOf(",") + 1));
    };
    reader.onerror = () => reject(new Error("图片读取失败"));
    reader.readAsDataURL(blob);
  });
}

async function withExport(action: (data: string) => Promise<void>) {
  try {
    await action(await exportBase64());
  } catch (error) {
    showTip(`${error}`);
  }
}

function copyToClipboard() {
  void withExport(async (data) => {
    await invoke("copy_screenshot_to_clipboard", { data });
    await invoke("finish_screenshot");
  });
}

function saveToFile() {
  void withExport(async (data) => {
    const saved = await invoke<string | null>("save_screenshot_to_file", { data });
    if (saved !== null) {
      await invoke("finish_screenshot");
    }
  });
}

function pinToScreen() {
  void withExport(async (data) => {
    const sel = selection.value!;
    await invoke("pin_screenshot", { data, width: sel.w, height: sel.h });
    await invoke("finish_screenshot");
  });
}

function onKeyDown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    if (textInput.active) {
      textInput.active = false;
      return;
    }
    cancelSession();
    return;
  }
  if (event.key === "Enter") {
    if (textInput.active) {
      event.preventDefault();
      commitTextInput();
      return;
    }
    if (phase.value === "edit" && selection.value) {
      event.preventDefault();
      copyToClipboard();
    }
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z") {
    event.preventDefault();
    undo();
  }
}

function showTip(message: string) {
  tip.value = message;
  if (tipTimer) clearTimeout(tipTimer);
  tipTimer = setTimeout(() => (tip.value = ""), 2600);
}

// ---------------------------------------------------------------------------
// 工具栏布局
// ---------------------------------------------------------------------------

const toolbarStyle = computed(() => {
  const sel = selection.value;
  if (!sel) return { display: "none" };
  const tbW = toolbarSize.width;
  const tbH = toolbarSize.height;
  let top = sel.y + sel.h + 12;
  if (top + tbH > display.value.height - 8) {
    top = sel.y - tbH - 12;
  }
  if (top < 8) top = 8;
  let left = sel.x + sel.w - tbW;
  left = Math.max(8, Math.min(left, display.value.width - tbW - 8));
  return { top: `${Math.round(top)}px`, left: `${Math.round(left)}px` };
});

const textStyle = computed(() => {
  const sel = selection.value;
  if (!sel) return {};
  return {
    left: `${textInput.x + sel.x}px`,
    top: `${textInput.y + sel.y - 2}px`,
    color: color.value,
    font: `${fontSize.value}px -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif`,
    minWidth: "60px",
  };
});
</script>

<template>
  <div
    v-if="ready"
    class="fixed inset-0 select-none overflow-hidden"
    @mousedown="onMouseDown"
    @mousemove="onMouseMove"
    @mouseup="onMouseUp"
  >
    <!-- 冻结的全屏背景 -->
    <img
      ref="bgImage"
      :src="bgSrc"
      alt=""
      draggable="false"
      class="pointer-events-none absolute inset-0 h-full w-full"
    />

    <!-- 遮罩 / 选区 / 标注 -->
    <canvas
      ref="canvasEl"
      class="pointer-events-none absolute inset-0"
      :style="{ width: `${display.width}px`, height: `${display.height}px` }"
    ></canvas>

    <!-- 文本标注输入框 -->
    <input
      v-if="textInput.active && selection"
      id="annotation-text-input"
      v-model="textInput.value"
      :style="textStyle"
      class="absolute z-20 border-b border-dashed border-white/70 bg-transparent outline-none"
      placeholder="输入文字…"
      @mousedown.stop
      @keydown.enter.prevent="commitTextInput"
      @blur="commitTextInput"
    />

    <!-- 工具栏 -->
    <div
      v-if="phase === 'edit'"
      ref="toolbarEl"
      :style="toolbarStyle"
      class="absolute z-10 flex items-center gap-1 rounded-lg border border-white/10 bg-neutral-900/90 px-2 py-1.5 shadow-xl backdrop-blur"
      @mousedown.stop
      @mousemove.stop
      @mouseup.stop
    >
      <!-- 标注工具 -->
      <button
        title="矩形框线"
        :class="toolClass(activeTool === 'rect')"
        @click="setTool('rect')"
      >
        <Square class="size-4" />
      </button>
      <button
        title="箭头"
        :class="toolClass(activeTool === 'arrow')"
        @click="setTool('arrow')"
      >
        <ArrowUpRight class="size-4" />
      </button>
      <button
        title="文本"
        :class="toolClass(activeTool === 'text')"
        @click="setTool('text')"
      >
        <Type class="size-4" />
      </button>

      <div class="mx-1 h-5 w-px bg-white/15"></div>

      <!-- 颜色 -->
      <button
        v-for="item in COLORS"
        :key="item"
        :title="`颜色 ${item}`"
        :class="[
          'flex size-6 items-center justify-center rounded-full border transition-colors',
          color === item ? 'border-blue-400 ring-1 ring-blue-400' : 'border-transparent hover:border-white/30',
        ]"
        @click="color = item"
      >
        <span class="size-3.5 rounded-full border border-black/20" :style="{ background: item }"></span>
      </button>

      <div class="mx-1 h-5 w-px bg-white/15"></div>

      <!-- 文本工具时调节字号，其他工具调节线条粗细 -->
      <template v-if="activeTool === 'text'">
        <button
          v-for="size in FONT_SIZES"
          :key="size"
          :title="`字号 ${size}`"
          :class="[
            'flex h-6 w-7 items-center justify-center rounded-md text-[10px] transition-colors',
            fontSize === size ? 'bg-white/20 text-white' : 'text-neutral-300 hover:bg-white/10',
          ]"
          @click="fontSize = size"
        >
          {{ size }}
        </button>
      </template>
      <template v-else>
        <button
          v-for="width in WIDTHS"
          :key="width"
          :title="`粗细 ${width}`"
          :class="[
            'flex h-6 w-7 items-center justify-center rounded-md transition-colors',
            strokeWidth === width ? 'bg-white/20' : 'hover:bg-white/10',
          ]"
          @click="strokeWidth = width"
        >
          <span
            class="rounded-full bg-white"
            :style="{ width: `${width * 3 + 2}px`, height: `${width}px` }"
          ></span>
        </button>
      </template>

      <div class="mx-1 h-5 w-px bg-white/15"></div>

      <button title="撤销 (⌘Z)" :class="toolClass(false)" @click="undo">
        <Undo2 class="size-4" />
      </button>

      <div class="mx-1 h-5 w-px bg-white/15"></div>

      <!-- 输出动作 -->
      <button title="贴图置顶" :class="actionClass" @click="pinToScreen">
        <Pin class="size-4" />
      </button>
      <button title="保存到文件" :class="actionClass" @click="saveToFile">
        <Download class="size-4" />
      </button>
      <button title="复制到粘贴板 (↩)" :class="primaryActionClass" @click="copyToClipboard">
        <Copy class="size-4" />
      </button>
      <button title="关闭 (Esc)" :class="actionClass" @click="cancelSession">
        <X class="size-4" />
      </button>
    </div>

    <!-- 错误提示 -->
    <div
      v-if="tip"
      class="absolute left-1/2 top-6 z-30 -translate-x-1/2 rounded-md bg-red-600/90 px-4 py-2 text-sm text-white shadow-lg"
    >
      {{ tip }}
    </div>
  </div>
</template>
