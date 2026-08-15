import { check } from "@tauri-apps/plugin-updater";
import type { Update } from "@tauri-apps/plugin-updater";

export type { Update };

export const updaterEnabled =
  import.meta.env.VITE_SH_UPDATES_ENABLED === "true";

export const LAST_UPDATE_KEY = "sh-launcher-last-update";

export async function checkForUpdate(
  timeoutMs = 20_000,
): Promise<Update | undefined> {
  return (await check({ timeout: timeoutMs })) ?? undefined;
}
