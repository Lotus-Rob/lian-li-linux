<script setup lang="ts">
import { useDaemonStore } from "@/stores/daemon";
import { useTranslate } from "@/i18n";

const daemon = useDaemonStore();
const t = useTranslate();
</script>

<template>
  <n-alert v-if="daemon.connected && daemon.writeError" type="warning" :title="t('shell.compatibilityNotice.title')">
    {{ daemon.writeError }}
  </n-alert>
</template>
