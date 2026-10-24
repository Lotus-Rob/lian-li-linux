import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { invoke } from "@/utils/backendText";
import { boundedFetch } from "@/utils/boundedFetch";
import { readStoredJson, writeStoredJson } from "@/utils/storedPreference";

export const RELEASES_URL = "https://github.com/sgtaziz/lian-li-linux/releases";
const LATEST_RELEASE_API = "https://api.github.com/repos/sgtaziz/lian-li-linux/releases/latest";
const ENABLED_KEY = "lianli-linux.updateCheck";
const CACHE_KEY = "lianli-linux.updateCache";
const DISMISSED_KEY = "lianli-linux.updateDismissed";
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
const RETRY_INTERVAL_MS = 60 * 60 * 1000;

interface CachedRelease {
  checkedAt: number;
  version: string | null;
}

export function parseVersion(value: string): number[] | null {
  const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(value);
  return match ? match.slice(1).map(Number) : null;
}

function normalizeVersion(value: string): string | null {
  return parseVersion(value)?.join(".") ?? null;
}

export function isNewer(candidate: string, current: string): boolean {
  const next = parseVersion(candidate);
  const running = parseVersion(current);
  if (!next || !running) return false;
  for (let i = 0; i < 3; i++) {
    if (next[i] !== running[i]) return next[i] > running[i];
  }
  return false;
}

function readCache(): CachedRelease | null {
  const value = readStoredJson(CACHE_KEY) as Partial<CachedRelease> | null;
  if (typeof value?.checkedAt !== "number") return null;
  return {
    checkedAt: value.checkedAt,
    version: typeof value.version === "string" ? normalizeVersion(value.version) : null,
  };
}

async function fetchLatestVersion(): Promise<string | null> {
  const blob = await boundedFetch(LATEST_RELEASE_API, 256 * 1024, new AbortController().signal, 10000);
  const release = JSON.parse(await blob.text()) as { tag_name?: unknown; draft?: unknown; prerelease?: unknown };
  if (release.draft || release.prerelease || typeof release.tag_name !== "string") return null;
  return normalizeVersion(release.tag_name);
}

export const useUpdatesStore = defineStore("updates", () => {
  const enabled = ref(readStoredJson(ENABLED_KEY) !== false);
  const current = ref<string | null>(null);
  const latest = ref<string | null>(readCache()?.version ?? null);
  const dismissed = ref(readStoredJson(DISMISSED_KEY));
  let timer: ReturnType<typeof setInterval> | null = null;
  let checking = false;

  const available = computed(() =>
    enabled.value && current.value && latest.value && isNewer(latest.value, current.value) ? latest.value : null,
  );
  const showNotice = computed(() => available.value !== null && available.value !== dismissed.value);

  async function check() {
    if (!enabled.value || checking) return;
    checking = true;
    try {
      current.value ??= await invoke<string>("app_version");
      const cached = readCache();
      if (cached && Date.now() - cached.checkedAt < CHECK_INTERVAL_MS) {
        latest.value = cached.version ?? latest.value;
        return;
      }
      const version = await fetchLatestVersion().catch(() => undefined);
      // A failed check is recorded so it retries after an hour rather than on every start.
      const checkedAt = version === undefined ? Date.now() - CHECK_INTERVAL_MS + RETRY_INTERVAL_MS : Date.now();
      if (version) latest.value = version;
      writeStoredJson(CACHE_KEY, { checkedAt, version: version ?? latest.value });
    } catch {
      // The running version is unavailable; the next interval retries.
    } finally {
      checking = false;
    }
  }

  function start() {
    void check();
    timer ??= setInterval(() => void check(), RETRY_INTERVAL_MS);
  }

  function setEnabled(value: boolean) {
    enabled.value = value;
    writeStoredJson(ENABLED_KEY, value);
    if (value) void check();
  }

  function dismiss() {
    if (!available.value) return;
    dismissed.value = available.value;
    writeStoredJson(DISMISSED_KEY, available.value);
  }

  return { enabled, available, showNotice, start, setEnabled, dismiss };
});
