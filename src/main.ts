import { createApp } from "vue";
import "./index.css";

// 根据窗口 URL 参数分发视图：主面板 / 截图覆盖层 / 贴图窗口
const mode = new URLSearchParams(window.location.search).get("mode");

if (mode === "screenshot" || mode === "pin") {
  // 透明窗口：页面背景必须透明
  document.documentElement.classList.add("transparent-body");
}

async function bootstrap() {
  if (mode === "screenshot") {
    const { default: ScreenshotOverlay } = await import("./views/ScreenshotOverlay.vue");
    createApp(ScreenshotOverlay).mount("#app");
    return;
  }
  if (mode === "pin") {
    const { default: PinView } = await import("./views/PinView.vue");
    createApp(PinView).mount("#app");
    return;
  }
  const { default: App } = await import("./App.vue");
  createApp(App).mount("#app");
}

bootstrap();
