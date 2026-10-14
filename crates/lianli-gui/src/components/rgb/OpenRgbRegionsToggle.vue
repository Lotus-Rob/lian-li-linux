<script setup lang="ts">
import { computed } from "vue";
import { useConfigStore } from "@/stores/config";
import { useTranslate } from "@/i18n";

const t = useTranslate();
const config = useConfigStore();
const enabled = computed({
  get: () => config.ensureRgb().openrgb_regions ?? false,
  set: (value: boolean) => {
    config.ensureRgb().openrgb_regions = value;
    config.markDirty();
  },
});
</script>

<template>
  <div>
    <n-checkbox v-model:checked="enabled">{{ t("rgb.openrgbRegions.label") }}</n-checkbox>
    <p class="hint">{{ t("rgb.openrgbRegions.hint") }}</p>
  </div>
</template>
