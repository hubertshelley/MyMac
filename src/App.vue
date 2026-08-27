<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Gauge, AppWindow, Rocket, Settings, ClipboardList, KeyRound, Beer } from "@lucide/vue";
import DashboardView from "@/views/DashboardView.vue";
import AppsView from "@/views/AppsView.vue";
import LaunchView from "@/views/LaunchView.vue";
import SettingsView from "@/views/SettingsView.vue";
import ClipboardView from "@/views/ClipboardView.vue";
import TotpView from "@/views/TotpView.vue";
import BrewView from "@/views/BrewView.vue";
import { cn } from "@/lib/utils";

type Tab = "dashboard" | "apps" | "launch" | "brew" | "clipboard" | "totp" | "settings";
const current = ref<Tab>("dashboard");
let unlistenNavigation: (() => void) | undefined;

// 屏幕录制权限缺失提示
const permissionTipVisible = ref(false);
let unlistenPermission: (() => void) | undefined;

function openScreenSettings() {
  invoke("open_screen_capture_settings").catch((error) => console.error(error));
}

onMounted(async () => {
  unlistenNavigation = await listen<string>("navigate-to", (event) => {
    if (event.payload === "totp") current.value = "totp";
  });
  unlistenPermission = await listen("screenshot-permission-needed", () => {
    permissionTipVisible.value = true;
  });
});

onUnmounted(() => {
  unlistenNavigation?.();
  unlistenPermission?.();
});

const tabs: { key: Tab; label: string; icon: unknown }[] = [
  { key: "dashboard", label: "资源监控", icon: Gauge },
  { key: "apps", label: "应用卸载", icon: AppWindow },
  { key: "launch", label: "启动项", icon: Rocket },
  { key: "brew", label: "Homebrew", icon: Beer },
  { key: "clipboard", label: "粘贴板", icon: ClipboardList },
  { key: "totp", label: "2FA 验证码", icon: KeyRound },
  { key: "settings", label: "设置", icon: Settings },
];

const currentView = computed(() => {
  if (current.value === "apps") return AppsView;
  if (current.value === "launch") return LaunchView;
  if (current.value === "brew") return BrewView;
  if (current.value === "clipboard") return ClipboardView;
  if (current.value === "totp") return TotpView;
  if (current.value === "settings") return SettingsView;
  return DashboardView;
});
</script>

<template>
  <div class="flex h-screen w-full bg-background">
    <aside class="flex w-52 shrink-0 flex-col border-r border-border bg-muted/30">
      <div class="flex items-center gap-2 px-4 py-4">
        <div
          class="flex size-7 items-center justify-center rounded-md bg-primary text-primary-foreground"
        >
          <Gauge class="size-4" />
        </div>
        <span class="text-sm font-semibold">MyMac 管家</span>
      </div>
      <nav class="flex flex-col gap-1 px-2">
        <button
          v-for="tab in tabs"
          :key="tab.key"
          :class="
            cn(
              'flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors',
              current === tab.key
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
            )
          "
          @click="current = tab.key"
        >
          <component :is="tab.icon" class="size-4" />
          {{ tab.label }}
        </button>
      </nav>
    </aside>
    <main class="flex-1 overflow-y-auto p-6">
      <component :is="currentView" />
    </main>

    <!-- 屏幕录制权限提示 -->
    <div
      v-if="permissionTipVisible"
      class="absolute bottom-6 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-lg border border-border bg-popover px-4 py-3 shadow-lg"
    >
      <span class="text-sm">截图功能需要「屏幕录制」权限，请在系统设置中允许 MyMac。</span>
      <button
        class="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground hover:opacity-90"
        @click="openScreenSettings"
      >
        打开设置
      </button>
      <button
        class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-accent"
        @click="permissionTipVisible = false"
      >
        稍后
      </button>
    </div>
  </div>
</template>
