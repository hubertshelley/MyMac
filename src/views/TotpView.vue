<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ClipboardPaste, Copy, ImagePlus, KeyRound, Plus, Timer, Trash2 } from "@lucide/vue";
import type { TotpAccount } from "@/types";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";

const accounts = ref<TotpAccount[]>([]);
const name = ref("");
const issuer = ref("");
const secret = ref("");
const digits = ref(6);
const period = ref(30);
const adding = ref(false);
const decodingQr = ref(false);
const deletingId = ref<string | null>(null);
const pendingDeleteId = ref<string | null>(null);
const error = ref("");
const toast = ref("");
let refreshTimer: number | undefined;
let toastTimer: number | undefined;

const isOtpauth = computed(() => secret.value.trim().toLowerCase().startsWith("otpauth://"));

function parseOtpauth(input: string) {
  try {
    const url = new URL(input.trim());
    if (url.protocol !== "otpauth:" || url.hostname !== "totp") return null;

    const label = decodeURIComponent(url.pathname.replace(/^\//, "")).trim();
    const separator = label.indexOf(":");
    const labelIssuer = separator >= 0 ? label.slice(0, separator).trim() : "";
    const accountName = separator >= 0 ? label.slice(separator + 1).trim() : label;
    const parsedDigits = Number(url.searchParams.get("digits") || 6);
    const parsedPeriod = Number(url.searchParams.get("period") || 30);

    if (!accountName) return null;
    return {
      name: accountName,
      issuer: url.searchParams.get("issuer")?.trim() || labelIssuer,
      digits: parsedDigits === 8 ? 8 : 6,
      period: parsedPeriod === 60 ? 60 : 30,
    };
  } catch {
    return null;
  }
}

watch(secret, (value) => {
  if (!value.trim().toLowerCase().startsWith("otpauth://")) return;
  const parsed = parseOtpauth(value);
  if (!parsed) return;
  name.value = parsed.name;
  issuer.value = parsed.issuer;
  digits.value = parsed.digits;
  period.value = parsed.period;
  error.value = "";
});

function displayCode(code: string) {
  const middle = Math.ceil(code.length / 2);
  return `${code.slice(0, middle)} ${code.slice(middle)}`;
}

function showToast(message: string) {
  toast.value = message;
  if (toastTimer) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.value = ""), 1800);
}

async function refresh() {
  accounts.value = await invoke<TotpAccount[]>("get_totp_accounts");
}

async function applyQrResult(action: () => Promise<string>) {
  error.value = "";
  decodingQr.value = true;
  try {
    secret.value = await action();
    showToast("二维码识别成功，已自动填入");
  } catch (e) {
    error.value = String(e);
  } finally {
    decodingQr.value = false;
  }
}

async function captureQr() {
  await applyQrResult(() => invoke<string>("capture_totp_qr"));
}

async function decodeQrClipboard() {
  await applyQrResult(() => invoke<string>("decode_totp_qr_clipboard"));
}

async function addAccount() {
  error.value = "";
  adding.value = true;
  try {
    await invoke("add_totp_account", {
      name: name.value,
      issuer: issuer.value,
      secret: secret.value,
      digits: digits.value,
      period: period.value,
    });
    name.value = "";
    issuer.value = "";
    secret.value = "";
    digits.value = 6;
    period.value = 30;
    await refresh();
    showToast("账户已添加");
  } catch (e) {
    error.value = String(e);
  } finally {
    adding.value = false;
  }
}

async function copyCode(account: TotpAccount) {
  try {
    const code = await invoke<string>("copy_totp_code", { id: account.id });
    showToast(`已复制 ${code}`);
    await refresh();
  } catch (e) {
    error.value = String(e);
  }
}

function requestRemove(account: TotpAccount) {
  error.value = "";
  pendingDeleteId.value = account.id;
}

async function removeAccount(account: TotpAccount) {
  error.value = "";
  deletingId.value = account.id;
  try {
    await invoke("delete_totp_account", { id: account.id });
    accounts.value = accounts.value.filter((item) => item.id !== account.id);
    pendingDeleteId.value = null;
    await refresh();
    showToast("账户已删除");
  } catch (e) {
    error.value = `删除失败：${String(e)}`;
  } finally {
    deletingId.value = null;
  }
}

onMounted(async () => {
  await refresh();
  refreshTimer = window.setInterval(refresh, 1000);
});

onUnmounted(() => {
  if (refreshTimer) window.clearInterval(refreshTimer);
  if (toastTimer) window.clearTimeout(toastTimer);
});
</script>

<template>
  <div class="space-y-5">
    <div>
      <h1 class="text-xl font-semibold">2FA 验证码</h1>
      <p class="mt-1 text-sm text-muted-foreground">管理 TOTP 账户，点击验证码即可复制。</p>
    </div>

    <Card class="p-4">
      <form class="space-y-3" @submit.prevent="addAccount">
        <div class="flex items-center gap-2 text-sm font-medium">
          <Plus class="size-4" />
          添加账户
        </div>
        <label class="block space-y-1 text-xs text-muted-foreground">
          <span>Base32 密钥或 otpauth:// 链接</span>
          <input v-model="secret" required autocomplete="off" spellcheck="false" placeholder="JBSWY3DPEHPK3PXP 或 otpauth://totp/…" class="h-9 w-full rounded-md border border-input bg-background px-3 font-mono text-sm text-foreground outline-none focus:ring-2 focus:ring-ring" />
        </label>
        <div class="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" size="sm" :disabled="decodingQr" @click="captureQr">
            <ImagePlus class="size-4" />截图识别
          </Button>
          <Button type="button" variant="outline" size="sm" :disabled="decodingQr" @click="decodeQrClipboard">
            <ClipboardPaste class="size-4" />读取剪贴板截图
          </Button>
          <span v-if="decodingQr" class="text-xs text-muted-foreground">正在识别二维码…</span>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <label class="space-y-1 text-xs text-muted-foreground">
            <span>账户名称{{ isOtpauth ? "（已从链接获取）" : "" }}</span>
            <input v-model="name" :readonly="isOtpauth" placeholder="例如：name@example.com" class="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none focus:ring-2 focus:ring-ring read-only:bg-muted/50" />
          </label>
          <label class="space-y-1 text-xs text-muted-foreground">
            <span>发行方{{ isOtpauth ? "（已从链接获取）" : "（可选）" }}</span>
            <input v-model="issuer" :readonly="isOtpauth" placeholder="例如：GitHub" class="h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground outline-none focus:ring-2 focus:ring-ring read-only:bg-muted/50" />
          </label>
        </div>
        <div v-if="!isOtpauth" class="flex items-end gap-3">
          <label class="space-y-1 text-xs text-muted-foreground">
            <span>位数</span>
            <select v-model.number="digits" class="h-9 rounded-md border border-input bg-background px-3 text-sm text-foreground">
              <option :value="6">6 位</option>
              <option :value="8">8 位</option>
            </select>
          </label>
          <label class="space-y-1 text-xs text-muted-foreground">
            <span>刷新周期</span>
            <select v-model.number="period" class="h-9 rounded-md border border-input bg-background px-3 text-sm text-foreground">
              <option :value="30">30 秒</option>
              <option :value="60">60 秒</option>
            </select>
          </label>
          <Button type="submit" size="sm" :disabled="adding || !secret.trim()">
            <Plus class="size-4" />{{ adding ? "添加中…" : "添加" }}
          </Button>
        </div>
        <Button v-else type="submit" size="sm" :disabled="adding || !secret.trim()">
          <Plus class="size-4" />{{ adding ? "添加中…" : "从链接添加" }}
        </Button>
        <p v-if="error" class="text-sm text-destructive">{{ error }}</p>
      </form>
    </Card>

    <div v-if="!accounts.length" class="flex flex-col items-center gap-2 rounded-lg border border-dashed py-12 text-muted-foreground">
      <KeyRound class="size-9" />
      <p class="text-sm">暂无 2FA 账户</p>
    </div>

    <div v-else class="grid grid-cols-2 gap-4">
      <Card v-for="account in accounts" :key="account.id" class="group relative overflow-hidden p-4">
        <div
          class="cursor-pointer pr-9 text-left"
          role="button"
          tabindex="0"
          title="复制验证码"
          @click="copyCode(account)"
          @keydown.enter="copyCode(account)"
          @keydown.space.prevent="copyCode(account)"
        >
          <div class="min-w-0">
            <p class="truncate text-sm font-semibold">{{ account.name }}</p>
            <p class="truncate text-xs text-muted-foreground">{{ account.issuer || "TOTP" }}</p>
          </div>
          <p class="mt-4 font-mono text-3xl font-semibold tracking-[0.18em]">{{ displayCode(account.code) }}</p>
          <div class="mt-3 flex items-center gap-2 text-xs text-muted-foreground">
            <Timer class="size-3.5" />
            <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
              <div class="h-full rounded-full bg-primary transition-[width] duration-1000" :style="{ width: `${(account.remaining / account.period) * 100}%` }" />
            </div>
            <span class="w-8 text-right">{{ account.remaining }} 秒</span>
          </div>
        </div>
        <div v-if="pendingDeleteId !== account.id" class="absolute right-2 top-2 z-10 flex items-center gap-1 rounded-md bg-background/90 opacity-0 shadow-sm transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
          <Button type="button" variant="ghost" size="icon" class="size-8" title="复制" @click.stop="copyCode(account)">
            <Copy class="size-4" />
          </Button>
          <Button type="button" variant="ghost" size="icon" class="size-8 text-muted-foreground hover:text-destructive" title="删除账户" @click.stop="requestRemove(account)">
            <Trash2 class="size-4" />
          </Button>
        </div>
        <div v-else class="absolute inset-x-0 bottom-0 z-20 flex items-center justify-between gap-3 border-t border-destructive/20 bg-background px-4 py-2 shadow-lg" @click.stop>
          <span class="min-w-0 truncate text-xs text-destructive">确定删除“{{ account.name }}”？</span>
          <div class="flex shrink-0 gap-2">
            <Button type="button" variant="ghost" size="sm" :disabled="deletingId === account.id" @click.stop="pendingDeleteId = null">取消</Button>
            <Button type="button" variant="destructive" size="sm" :disabled="deletingId === account.id" @click.stop="removeAccount(account)">
              {{ deletingId === account.id ? "删除中…" : "确认删除" }}
            </Button>
          </div>
        </div>
      </Card>
    </div>

    <div v-if="toast" class="fixed bottom-6 right-6 rounded-md bg-foreground px-4 py-2 text-sm text-background shadow-lg">{{ toast }}</div>
  </div>
</template>
