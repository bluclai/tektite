import { platform as getOSPlatform } from "@tauri-apps/plugin-os";

export type Platform = "macos" | "linux" | "windows";

let _platform = $state<Platform>("linux");

export const platformStore = {
  get value(): Platform {
    return _platform;
  },
};

export async function initPlatform(): Promise<void> {
  const p = getOSPlatform();
  if (p === "macos" || p === "linux" || p === "windows") {
    _platform = p;
  }
}
