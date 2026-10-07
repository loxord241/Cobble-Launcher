import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (err: unknown) {
    const errorObj = err as { code?: string; message?: string };
    const msg = errorObj?.message || (typeof err === "string" ? err : JSON.stringify(err));
    const error = new Error(msg);
    Object.assign(error, err);
    throw error;
  }
}
import type {
  Account,
  ArchiveResult,
  AuthlibServerInfo,
  ConfigContent,
  ConfigFile,
  ContentEntry,
  ContentKind,
  Diagnosis,
  GameResourceSample,
  RamGuide,
  CoreEvent,
  Instance,
  JavaInstall,
  WorldInfo,
  WorldScope,
  LoaderVersionEntry,
  LogoResult,
  ModrinthVersion,
  ProjectDetail,
  ProjectMeta,
  SearchResult,
  JavaTestInfo,
  LogFileInfo,
  ModMetadata,
  ModpackInstallResult,
  PackArtInfo,
  RepairReport,
  ShotInfo,
  Settings,
  SnapshotInfo,
  SkinInfo,
  SkinLibraryItem,
  SkinModel,
  SkinUserItem,
  StorageStats,
  UpdateCheck,
  UpdateInfo,
  VersionEntry,
} from "./types";

export const api = {
  instanceList: () => invoke<Instance[]>("instance_list"),
  instanceCreate: (mcVersion: string, name: string) =>
    invoke<Instance>("instance_create", { mcVersion, name }),
  instanceRename: (id: string, name: string) =>
    invoke<Instance>("instance_rename", { id, name }),
  /** Смена версии MC инстанса (как в Prism): ядро докачивает файлы новой версии. */
  instanceVersionChange: (id: string, newMcVersion: string) =>
    invoke<{ ok: boolean }>("instance_version_change", { id, newMcVersion }),
  instanceDuplicate: (id: string) => invoke<Instance>("instance_duplicate", { id }),
  instanceDelete: (id: string, wipe: boolean) =>
    invoke<{ ok: boolean }>("instance_delete", { id, wipe }),
  instanceOpenDir: (id: string) => invoke<{ ok: boolean }>("instance_open_dir", { id }),
  instanceSettingsGet: (id: string) => invoke<Instance>("instance_settings_get", { id }),
  instanceSettingsSet: (instance: Instance) =>
    invoke<Instance>("instance_settings_set", { instance }),
  /** D48: идентификатор инстанса из ярлыка, если окно открылось позже колбэка. */
  shortcutTakePending: () => invoke<string | null>("shortcut_take_pending"),
  instanceLaunch: (id: string, player: string) =>
    invoke<{ ok: boolean }>("instance_launch", { id, player }),
  instanceImport: (path: string, name?: string) =>
    invoke<Instance>("instance_import", { path, name }),
  /** D63: официальная панорама титульного экрана из собственных файлов
   * игры инстанса (data-URL PNG) — null, пока ассеты не скачаны. */
  panoramaArt: (id: string) => invoke<string | null>("panorama_art", { id }),

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
    index?: string,
    limit?: number,
    offset?: number,
    categories?: string[],
  ) =>
    invoke<SearchResult>("modrinth_search", {
      query,
      mcVersion,
      loader,
      projectType,
      index,
      limit,
      offset,
      categories,
    }),
  // Метаданные проектов батчем (иконка/название для списка установленного).
  modrinthProjects: (ids: string[]) => invoke<ProjectMeta[]>("modrinth_projects", { ids }),
  // Страница проекта целиком (D39): тело markdown, галерея, лицензия.
  modrinthProject: (idOrSlug: string) => invoke<ProjectDetail>("modrinth_project", { idOrSlug }),
  // Версии проекта с changelog (D39): табы «Версии»/«Журнал изменений».
  modrinthVersions: (projectId: string) =>
    invoke<ModrinthVersion[]>("modrinth_versions", { projectId }),
  contentInstall: (instanceId: string, projectId: string, versionId?: string) =>
    invoke<ContentEntry[]>("content_install", { instanceId, projectId, versionId }),
  contentInstalled: (instanceId: string) =>
    invoke<ContentEntry[]>("content_installed", { instanceId }),
  contentToggle: (instanceId: string, file: string) =>
    invoke<ContentEntry>("content_toggle", { instanceId, file }),
  contentRemove: (instanceId: string, file: string) =>
    invoke<void>("content_remove", { instanceId, file }),
  contentUpdateCheck: (instanceId: string) =>
    invoke<UpdateCheck[]>("content_update_check", { instanceId }),
  contentUpdateAll: (instanceId: string) => invoke<number>("content_update_all", { instanceId }),
  contentBackfillProjects: (instanceId: string) =>
    invoke<number>("content_backfill_projects", { instanceId }),
  // Установка модпака: группа загрузок приходит в ответе — по ней фильтруются
  // события dl_progress/dl_group_done и адресуется downloads_cancel_group.
  mrpackInstall: (path: string, name?: string) =>
    invoke<ModpackInstallResult>("mrpack_install", { path, name }),
  modpackInstall: (projectId: string, name?: string) =>
    invoke<ModpackInstallResult>("modpack_install", { projectId, name }),
  // Отменить активную группу загрузок (установка модпака/ремонт); для
  // завершённой группы — no-op.
  downloadsCancelGroup: (group: string) =>
    invoke<void>("downloads_cancel_group", { group }),

  instanceBackup: (id: string) => invoke<ArchiveResult>("instance_backup", { id }),
  instanceExport: (id: string) => invoke<ArchiveResult>("instance_export", { id }),
  instanceIconSet: (id: string, path: string) => invoke<Instance>("instance_icon_set", { id, path }),
  instanceIconRemove: (id: string) => invoke<Instance>("instance_icon_remove", { id }),

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

  // Проверка обновлений лаунчера (GitHub releases; авто-установки нет)
  updateCheck: () => invoke<UpdateInfo>("update_check"),

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
  // Скин профиля (D34): null — скина нет (offline/404), UI показывает дефолт
  accountSkin: (accountId: string) => invoke<SkinInfo | null>("account_skin", { accountId }),

  // ---------- Скины (D59): вкладка «Скины» ----------
  // Библиотека дефолтов (Steve/Alex classic/slim из клиент-jar, как D54).
  skinLibraryList: () => invoke<SkinLibraryItem[]>("skin_library_list"),
  // Скины пользователя из каталога данных (skins/).
  skinUserList: () => invoke<SkinUserItem[]>("skin_user_list"),
  // Загрузить PNG (выбор файла — системный диалог, как instance_icon_set).
  skinUserSave: (path: string, name: string) =>
    invoke<SkinUserItem>("skin_user_save", { path, name }),
  skinUserDelete: (skinId: string) => invoke<{ ok: boolean }>("skin_user_delete", { skinId }),
  // Применить скин к активному аккаунту (msa → Mojang, ely/authlib → Ely.by).
  skinApply: (accountId: string, skinId: string, model: SkinModel) =>
    invoke<{ ok: boolean }>("skin_apply", { accountId, skinId, model }),

  // Рекомендуемая мажорная версия Java для версии MC (F14)
  javaRecommended: (mcVersion: string) => invoke<number>("java_recommended", { mcVersion }),

  // Снять зависшую блокировку инстанса (F27): .lock без живого процесса
  instanceForceUnlock: (id: string) => invoke<{ ok: boolean }>("instance_force_unlock", { id }),

  // ---------- миры, скриншоты, конфиги (аудит паритета D37) ----------
  // Миры инстанса (F24)
  instanceWorlds: (id: string) => invoke<WorldInfo[]>("instance_worlds", { id }),
  worldBackup: (id: string, world: string) => invoke<ArchiveResult>("world_backup", { id, world }),
  worldDeleteData: (id: string, world: string, scope: WorldScope) =>
    invoke<{ ok: boolean }>("world_delete_data", { id, world, scope }),
  // Проверка целостности и починка файлов инстанса (F2)
  instanceRepair: (id: string) => invoke<RepairReport>("instance_repair", { id }),
  // Проверка java.exe (F15)
  javaTestPath: (javaExe: string) => invoke<JavaTestInfo>("java_test_path", { javaExe }),
  // Отправить последний лог на mclo.gs (F22)
  logShareMclogs: (id: string) => invoke<{ url: string }>("log_share_mclogs", { id }),
  // Архивные логи инстанса (F23)
  instanceLogsList: (id: string) => invoke<LogFileInfo[]>("instance_logs_list", { id }),
  instanceLogRead: (id: string, name: string) => invoke<string>("instance_log_read", { id, name }),
  // Снапшоты модов (F8)
  contentSnapshots: (id: string) => invoke<SnapshotInfo[]>("content_snapshots", { id }),
  contentRollback: (id: string, name: string) =>
    invoke<ContentEntry[]>("content_rollback", { id, name }),
  // Скопировать контент в другой инстанс (F10)
  contentCopy: (fromId: string, toId: string, file: string) =>
    invoke<ContentEntry>("content_copy", { fromId, toId, file }),
  // Дисковое пространство (F18)
  storageStats: () => invoke<StorageStats>("storage_stats"),
  storageClean: () => invoke<{ freedBytes: number }>("storage_clean"),
  // Экспорт/импорт настроек (F20; секреты не вывозятся)
  settingsExport: (path: string) => invoke<{ ok: boolean }>("settings_export", { path }),
  settingsImport: (path: string) => invoke<Settings>("settings_import", { path }),

  // ---------- скины и аккаунты (аудит паритета D37-B) ----------
  // Скриншоты инстанса (F3)
  instanceScreens: (id: string) => invoke<ShotInfo[]>("instance_screens", { id }),
  screenshotDelete: (id: string, name: string) =>
    invoke<{ ok: boolean }>("screenshot_delete", { id, name }),
  // Метаданные модов: зависимости/конфликты (F6/F9)
  contentMetadata: (id: string) => invoke<ModMetadata[]>("content_metadata", { id }),
  // Ярлык на рабочем столе (F4)
  instanceShortcut: (id: string) => invoke<{ path: string }>("instance_shortcut", { id }),
  // Мониторинг ресурсов запущенной игры (F28)
  gameResources: (id: string) => invoke<GameResourceSample | null>("game_resources", { id }),
  // Открыть папку скриншотов (F3)
  instanceScreensOpen: (id: string) => invoke<{ ok: boolean }>("instance_screens_open", { id }),
  // Запуск в безопасном режиме (F30): сброс JVM + отключение шейдеров
  instanceLaunchSafe: (id: string, player: string) =>
    invoke<{ ok: boolean }>("instance_launch_safe", { id, player }),
  // Обложки ресурспаков/шейдеров (F25)
  packArt: (id: string, kinds: ContentKind[]) =>
    invoke<PackArtInfo[]>("pack_art", { id, kinds }),

  // ---------- контент и процессы (аудит паритета D37-C) ----------
  // Конфиги модов (F29)
  instanceConfigs: (id: string) => invoke<ConfigFile[]>("instance_configs", { id }),
  configRead: (id: string, rel: string) => invoke<ConfigContent>("config_read", { id, rel }),
  configWrite: (id: string, rel: string, text: string) =>
    invoke<{ ok: boolean }>("config_write", { id, rel, text }),
  // Свой authlib-сервер (F13)
  authlibServerInfo: (serverUrl: string) =>
    invoke<AuthlibServerInfo>("authlib_server_info", { serverUrl }),
  accountAddAuthlib: (serverUrl: string, username: string, password: string) =>
    invoke<Account>("account_add_authlib", { serverUrl, username, password }),
  // Логотип (D38): свой PNG → каталог данных; применение к окну + data-URL
  logoSetCustom: (path: string) => invoke<{ ok: boolean }>("logo_set_custom", { path }),
  applyLogo: (logo: string) => invoke<LogoResult>("apply_logo", { logo }),
};

/** Подписка на события ядра (dl_progress, launch_state, game_log_line, dl_queue_state, account_refresh_failed, dl_group_done). */
export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  const unlisteners: Promise<UnlistenFn>[] = [
    listen<CoreEvent>("dl_progress", (e) => handler(e.payload)),
    listen<CoreEvent>("launch_state", (e) => handler(e.payload)),
    listen<CoreEvent>("game_log_line", (e) => handler(e.payload)),
    listen<CoreEvent>("dl_queue_state", (e) => handler(e.payload)),
    listen<CoreEvent>("account_refresh_failed", (e) => handler(e.payload)),
    listen<CoreEvent>("dl_group_done", (e) => handler(e.payload)),
  ];
  return Promise.all(unlisteners).then((uns) => () => uns.forEach((u) => u()));
}
