// ЕДИНСТВЕННОЕ место, знающее имена Tauri-команд (спека §4.3).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CoreEvent,
  Instance,
  JavaInstall,
  LoaderVersionEntry,
  Settings,
  VersionEntry,
} from "./types";

export const api = {
  instanceList: () => invoke<Instance[]>("instance_list"),
  instanceCreate: (mcVersion: string, name: string) =>
    invoke<Instance>("instance_create", { mcVersion, name }),
  instanceRename: (id: string, name: string) =>
    invoke<Instance>("instance_rename", { id, name }),
  instanceDuplicate: (id: string) => invoke<Instance>("instance_duplicate", { id }),
  instanceDelete: (id: string, wipe: boolean) =>
    invoke<{ ok: boolean }>("instance_delete", { id, wipe }),
  instanceOpenDir: (id: string) => invoke<{ ok: boolean }>("instance_open_dir", { id }),
  instanceSettingsGet: (id: string) => invoke<Instance>("instance_settings_get", { id }),
  instanceSettingsSet: (instance: Instance) =>
    invoke<Instance>("instance_settings_set", { instance }),
  instanceLaunch: (id: string, player: string) =>
    invoke<{ ok: boolean }>("instance_launch", { id, player }),

  manifestVersions: (showSnapshots: boolean, showOld: boolean) =>
    invoke<VersionEntry[]>("manifest_versions", { showSnapshots, showOld }),
  loaderVersions: (loader: string) =>
    invoke<LoaderVersionEntry[]>("loader_versions", { loader }),
  loaderInstall: (id: string, loader: string, loaderVersion?: string) =>
    invoke<Instance>("loader_install", { id, loader, loaderVersion }),

  javaList: () => invoke<JavaInstall[]>("java_list"),
  javaInstall: (major: number) => invoke<JavaInstall>("java_install", { major }),

  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (settings: Settings) => invoke<{ ok: boolean }>("settings_set", { settings }),

  dataDir: () => invoke<string>("data_dir"),
  logsTail: (lines: number) => invoke<string>("logs_tail", { lines }),
  dirOpen: () => invoke<{ ok: boolean }>("dir_open"),
};

/** Подписка на события ядра (dl_progress, launch_state, game_log_line). */
export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  const unlisteners: Promise<UnlistenFn>[] = [
    listen<CoreEvent>("dl_progress", (e) => handler(e.payload)),
    listen<CoreEvent>("launch_state", (e) => handler(e.payload)),
    listen<CoreEvent>("game_log_line", (e) => handler(e.payload)),
  ];
  return Promise.all(unlisteners).then((uns) => () => uns.forEach((u) => u()));
}
