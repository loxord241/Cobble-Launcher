// ЕДИНСТВЕННОЕ место, знающее имена Tauri-команд (спека §4.3).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Account,
  ContentEntry,
  Diagnosis,
  RamGuide,
  CoreEvent,
  Instance,
  JavaInstall,
  LoaderVersionEntry,
  SearchResult,
  Settings,
  UpdateCheck,
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
  instanceImport: (path: string, name?: string) =>
    invoke<Instance>("instance_import", { path, name }),

  manifestVersions: (showSnapshots: boolean, showOld: boolean) =>
    invoke<VersionEntry[]>("manifest_versions", { showSnapshots, showOld }),
  loaderVersions: (loader: string) =>
    invoke<LoaderVersionEntry[]>("loader_versions", { loader }),
  loaderInstall: (id: string, loader: string, loaderVersion?: string) =>
    invoke<Instance>("loader_install", { id, loader, loaderVersion }),

  modrinthSearch: (
    query: string,
    mcVersion?: string,
    loader?: string,
    projectType?: string,
    limit?: number,
    offset?: number,
  ) =>
    invoke<SearchResult>("modrinth_search", {
      query,
      mcVersion,
      loader,
      projectType,
      limit,
      offset,
    }),
  contentInstall: (instanceId: string, projectId: string, versionId?: string) =>
    invoke<ContentEntry[]>("content_install", { instanceId, projectId, versionId }),
  contentInstalled: (instanceId: string) =>
    invoke<ContentEntry[]>("content_installed", { instanceId }),
  contentUpdateCheck: (instanceId: string) =>
    invoke<UpdateCheck[]>("content_update_check", { instanceId }),
  contentUpdateAll: (instanceId: string) => invoke<number>("content_update_all", { instanceId }),
  mrpackInstall: (path: string, name?: string) =>
    invoke<Instance>("mrpack_install", { path, name }),
  modpackInstall: (projectId: string, name?: string) =>
    invoke<Instance>("modpack_install", { projectId, name }),

  instanceOptimize: (instanceId: string) =>
    invoke<string[]>("instance_optimize", { instanceId }),
  instanceKill: (instanceId: string) => invoke<{ ok: boolean }>("instance_kill", { instanceId }),
  processStatus: () => invoke<[string, number][]>("process_status"),
  ramGuide: () => invoke<RamGuide>("ram_guide"),

  javaList: () => invoke<JavaInstall[]>("java_list"),
  javaInstall: (major: number) => invoke<JavaInstall>("java_install", { major }),

  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (settings: Settings) => invoke<{ ok: boolean }>("settings_set", { settings }),

  dataDir: () => invoke<string>("data_dir"),
  logsTail: (lines: number) => invoke<string>("logs_tail", { lines }),
  dirOpen: () => invoke<{ ok: boolean }>("dir_open"),

  crashAnalyze: (logText: string) => invoke<Diagnosis[]>("crash_analyze", { logText }),

  // Аккаунты (M8)
  accountList: () => invoke<Account[]>("account_list"),
  accountAddOffline: (name: string) => invoke<Account>("account_add_offline", { name }),
  accountAddMsaBrowser: () => invoke<Account>("account_add_msa_browser"),
  accountAddMsaStart: () =>
    invoke<{
      user_code: string;
      verification_uri: string;
      device_code: string;
      interval_secs: number;
      expires_in_secs: number;
    }>("account_add_msa_start"),
  accountAddMsaPoll: (deviceCode: string) =>
    invoke<Account | null>("account_add_msa_poll", { deviceCode }),
  accountAddEly: (username: string, password: string) =>
    invoke<Account>("account_add_ely", { username, password }),
  accountRemove: (id: string) => invoke<{ ok: boolean }>("account_remove", { id }),
  accountActiveSet: (id: string) => invoke<{ ok: boolean }>("account_active_set", { id }),
};

/** Подписка на события ядра (dl_progress, launch_state, game_log_line, account_refresh_failed). */
export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  const unlisteners: Promise<UnlistenFn>[] = [
    listen<CoreEvent>("dl_progress", (e) => handler(e.payload)),
    listen<CoreEvent>("launch_state", (e) => handler(e.payload)),
    listen<CoreEvent>("game_log_line", (e) => handler(e.payload)),
    listen<CoreEvent>("account_refresh_failed", (e) => handler(e.payload)),
  ];
  return Promise.all(unlisteners).then((uns) => () => uns.forEach((u) => u()));
}
