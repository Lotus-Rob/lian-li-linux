import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useMessage } from "naive-ui";

export function useExternalLink() {
  const message = useMessage();

  return async function openExternal(url: string) {
    try {
      await invoke("open_external", { url });
    } catch (error) {
      const copied = await writeText(url).then(() => true, () => false);
      message.error(
        `Could not open the link: ${error}. ${copied ? "The link was copied to the clipboard." : url}`,
        { duration: 8000, closable: true },
      );
    }
  };
}
