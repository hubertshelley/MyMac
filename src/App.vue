<script setup lang="ts">
import { ref, computed } from "vue";
import { Gauge, AppWindow, Rocket, Settings, ClipboardList } from "@lucide/vue";
import DashboardView from "@/views/DashboardView.vue";
import AppsView from "@/views/AppsView.vue";
import LaunchView from "@/views/LaunchView.vue";
import SettingsView from "@/views/SettingsView.vue";
import ClipboardView from "@/views/ClipboardView.vue";
import { cn } from "@/lib/utils";

type Tab = "dashboard" | "apps" | "launch" | "clipboard" | "settings";
const current = ref<Tab>("dashboard");

const tabs: { key: Tab; label: string; icon: unknown }[] = [
  { key: "dashboard", label: "资源监控", icon: Gauge },
  { key: "apps", label: "应用卸载", icon: AppWindow },
  { key: "launch", label: "启动项", icon: Rocket },
  { key: "clipboard", label: "粘贴板", icon: ClipboardList },
  { key: "settings", label: "设置", icon: Settings },
];

const currentView = computed(() => {
  if (current.value === "apps") return AppsView;
  if (current.value === "launch") return LaunchView;
  if (current.value === "clipboard") return ClipboardView;
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
  </div>
</template>
