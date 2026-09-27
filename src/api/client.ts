// ЕДИНСТВЕННОЕ место, знающее имена Tauri-команд (спека §4.3).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Ядро сериализует Instance в snake_case (mc_version), а контракт
 * (docs/UI-CONTRACT.md) и типы требуют camelCase. Rust-ядро не трогаем,
 * поэтому ключи приводятся на этой границе. Рекурсивно — вложенных
 * структур у Instance нет, но преобразование безопасно и для них.
 */
function snakeToCamel(k: string): string {
  return k.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}
function camelToSnake(k: string): string {
  return k.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);
}
function mapKeys<T>(v: T, f: (k: string) => string): T {
  if (Array.isArray(v)) return v.map((x) => mapKeys(x, f)) as unknown as T;
  if (v && typeof v === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
      out[f(k)] = mapKeys(val, f);
    }
    return out as T;
  }
  return v;
}
const camelize = <T>(v: T) => mapKeys(v, snakeToCamel);
const snakify = <T>(v: T) => mapKeys(v, camelToSnake);
import type {
  Account,
  ContentEntry,
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
  instanceList: () => invoke<Instance[]>("instance_list").then(camelize),
  instanceCreate: (mcVersion: string, name: string) =>
    invoke<Instance>("instance_create", { mcVersion, name }).then(camelize),
  instanceRename: (id: string, name: string) =>
    invoke<Instance>("instance_rename", { id, name }),
  instanceDuplicate: (id: string) =>
    invoke<Instance>("instance_duplicate", { id }).then(camelize),
  instanceDelete: (id: string, wipe: boolean) =>
    invoke<{ ok: boolean }>("instance_delete", { id, wipe }),
  instanceOpenDir: (id: string) => invoke<{ ok: boolean }>("instance_open_dir", { id }),
  instanceSettingsGet: (id: string) =>
    invoke<Instance>("instance_settings_get", { id }).then(camelize),
  instanceSettingsSet: (instance: Instance) =>
    invoke<Instance>("instance_settings_set", { instance: snakify(instance) }).then(camelize),
  instanceLaunch: (id: string, player: string) =>
    invoke<{ ok: boolean }>("instance_launch", { id, player }),

  manifestVersions: (showSnapshots: boolean, showOld: boolean) =>
    invoke<VersionEntry[]>("manifest_versions", { showSnapshots, showOld }),
  loaderVersions: (loader: string) =>
    invoke<LoaderVersionEntry[]>("loader_versions", { loader }),
  loaderInstall: (id: string, loader: string, loaderVersion?: string) =>
    invoke<Instance>("loader_install", { id, loader, loaderVersion }).then(camelize),

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
    invoke<Instance>("mrpack_install", { path, name }).then(camelize),
  modpackInstall: (projectId: string, name?: string) =>
    invoke<Instance>("modpack_install", { projectId, name }).then(camelize),

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

  // Аккаунты (M8): чип профиля в титлбаре.
  accountList: () => invoke<Account[]>("account_list"),
  accountActiveSet: (id: string) => invoke<{ ok: boolean }>("account_active_set", { id }),
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
