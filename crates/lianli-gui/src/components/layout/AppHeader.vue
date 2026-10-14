<script setup lang="ts">
import { computed, ref } from "vue";
import { useMessage } from "naive-ui";
import { useRoute } from "vue-router";
import { Moon, RefreshCw, Save, Sun } from "lucide-vue-next";
import { useDaemonStore } from "@/stores/daemon";
import { useConfigStore } from "@/stores/config";
import { useThemeStore } from "@/stores/theme";
import { useTranslate } from "@/i18n";
import { MAIN_ROUTES } from "@/router";

const daemon = useDaemonStore();
const config = useConfigStore();
const theme = useThemeStore();
const route = useRoute();
const message = useMessage();
const t = useTranslate();
const saving = ref(false);

const title = computed(() => {
  const match = MAIN_ROUTES.find((r) => r.name === route.name);
  return t(match?.labelKey ?? "nav.appName");
});

const refreshing = ref(false);

async function onRefresh() {
  if (refreshing.value) return;
  refreshing.value = true;
  try {
    await daemon.refresh();
  } finally {
    refreshing.value = false;
    // Drop focus so the button doesn't stay in its active/pressed colour.
    (document.activeElement as HTMLElement | null)?.blur();
  }
}

async function onSave() {
  if (saving.value) return;
  saving.value = true;
  try {
    await config.save();
  } catch (error) {
    message.error(t("shell.header.saveFailed", { error: error instanceof Error ? error.message : String(error) }), { duration: 10000 });
  } finally {
    saving.value = false;
    (document.activeElement as HTMLElement | null)?.blur();
  }
}

function onToggleTheme() {
  theme.toggle();
  (document.activeElement as HTMLElement | null)?.blur();
}
</script>

<template>
  <header class="header">
    <h1 class="title">{{ title }}</h1>
    <div class="spacer" />
    <n-button
      quaternary
      size="small"
      :title="theme.isDark ? t('shell.header.switchToLight') : t('shell.header.switchToDark')"
      @click="onToggleTheme"
    >
      <template #icon>
        <Sun v-if="theme.isDark" :size="15" />
        <Moon v-else :size="15" />
      </template>
    </n-button>
    <n-button
      quaternary
      size="small"
      :loading="refreshing"
      :disabled="refreshing"
      :title="refreshing ? t('shell.header.refreshing') : t('shell.header.refreshNow')"
      @click="onRefresh"
    >
      <template #icon>
        <RefreshCw :size="15" :class="{ spin: refreshing }" />
      </template>
    </n-button>
    <n-button
      type="primary"
      size="small"
      :disabled="saving || !config.dirty || !daemon.canWrite"
      :loading="saving"
      :class="{ dirty: config.dirty }"
      @click="onSave"
    >
      <template #icon><Save :size="15" /></template>
      {{ t("common.save") }}{{ config.dirty ? "*" : "" }}
    </n-button>
  </header>
</template>

<style scoped>
.header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-6);
  border-bottom: 1px solid var(--border);
  background: var(--bg-surface);
  flex-shrink: 0;
}
.title {
  font-size: var(--font-size-xl);
  font-weight: 600;
  margin: 0;
}
.spacer {
  flex: 1;
}
.spin {
  animation: header-spin 0.9s linear infinite;
}
@keyframes header-spin {
  to {
    transform: rotate(360deg);
  }
}
.dirty {
  box-shadow: 0 0 0 1px var(--warning), 0 0 8px rgba(251, 191, 36, 0.4);
}
</style>
