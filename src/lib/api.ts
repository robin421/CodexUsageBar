import type { AppSnapshot, DiagnosticEntry, ProviderId, SettingsPatch } from "./types";
import { mockBackend } from "./mock";

// In a plain browser (vite dev, screenshots) the UI runs against mock data.
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) return mockBackend.invoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const api = {
  getSnapshot: () => invoke<AppSnapshot>("get_snapshot"),
  refresh: () => invoke<AppSnapshot>("refresh"),
  updateSettings: (patch: SettingsPatch) => invoke<AppSnapshot>("update_settings", { patch }),
  setApiKey: (provider: ProviderId, key: string | null) => invoke<AppSnapshot>("set_api_key", { provider, key }),
  diagnostics: () => invoke<DiagnosticEntry[]>("diagnostics"),
  testNotification: () => invoke<void>("test_notification"),
  openSettings: () => invoke<void>("open_settings"),
  hidePanel: () => invoke<void>("hide_panel"),
  quit: () => invoke<void>("quit"),

  async onSnapshot(cb: (s: AppSnapshot) => void): Promise<() => void> {
    if (!inTauri) return mockBackend.subscribe(cb);
    const { listen } = await import("@tauri-apps/api/event");
    return listen<AppSnapshot>("snapshot", (e) => cb(e.payload));
  },

  async copy(text: string) {
    if (inTauri) {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
    } else {
      await navigator.clipboard?.writeText(text).catch(() => {});
    }
  },

  async openUrl(url: string) {
    if (inTauri) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    } else {
      window.open(url, "_blank", "noopener");
    }
  },

  async autostart(enable?: boolean): Promise<boolean> {
    if (!inTauri) return mockBackend.autostart(enable);
    const m = await import("@tauri-apps/plugin-autostart");
    if (enable === true) await m.enable();
    if (enable === false) await m.disable();
    return m.isEnabled();
  },

  async fitPanelHeight(px: number) {
    if (inTauri) await invoke<void>("fit_panel", { height: px });
  },
};
