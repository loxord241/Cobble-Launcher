// Типы — зеркало Rust-структур (docs/UI-CONTRACT.md). Рассинхрон = ошибка ревью.

// Профиль аккаунта (зеркало crate::auth::Account, camelCase).
export type AccountKind = "offline" | "msa" | "ely";
export interface Account {
  id: string;
  kind: AccountKind;
  name: string;
  uuid: string;
  refreshRef?: string;
}

/** Откуда взят скин профиля (зеркало crate::auth::skins::SkinSource). */
export type SkinSource = "ely" | "mojang";

/** Скин профиля для показа в лаунчере (D34); None — скина нет, UI рисует дефолт. */
export interface SkinInfo {
  source: SkinSource;
  dataUrl: string;
}

export interface Instance {
  schemaVersion: number;
  id: string;
  name: string;
  icon?: string;
  mcVersion: string;
  loader?: string;
  loaderVersion?: string;
  versionId?: string;
  javaPath?: string;
  ramMb: number;
  jvmFlags: string[];
  gameArgsExtra: string[];
  createdAt: number;
  lastPlayed?: number;
  playSeconds: number;
  launchCount: number;
  notes: string;
  /** Быстрый запуск (MC 1.20+): имя мира в saves для --quickPlaySingleplayer. */
  quickPlayWorld?: string;
  /** Быстрый запуск (MC 1.20+): host:port для --quickPlayMultiplayer. */
  quickPlayServer?: string;
  /** Профиль для запуска именно этого инстанса (F11); нет — глобальный. */
  accountId?: string;
  /** Подряд упавших запусков (F30). */
  crashCount: number;
}

export interface Resolution {
  width: number;
  height: number;
}

export interface Settings {
  schemaVersion: number;
  theme: "dark" | "light";
  language: string;
  downloadParallelism: number;
  defaultRamMb: number;
  defaultJvmFlags: string[];
  defaultResolution?: Resolution;
  showSnapshots: boolean;
  showOldVersions: boolean;
  proxyUrl?: string;
  mirrorBase?: string;
  curseforgeApiKey?: string;
  azureClientId?: string;
  accountsActiveId?: string;
  onboardingDone: boolean;
  extraJavaPaths: string[];
  /** Масштаб интерфейса в процентах (80–150). */
  uiScale: number;
  /** Шрифт интерфейса: "" — из темы; иначе system|serif|mono|round. */
  uiFont: string;
  /** Акцентный цвет поверх палитры темы: "" — из темы; иначе #RRGGBB. */
  uiAccent: string;
  /** Режим «Работать офлайн» (F16): сеть ядра отключается, запуск из кэша. */
  workOffline: boolean;
}

// ---------- Волна D37 (фаза 2 аудита паритета) ----------

/** Мир инстанса (зеркало instances::worlds::WorldInfo). */
export interface WorldInfo {
  name: string;
  lastModified: number;
  sizeBytes: number;
  hasNether: boolean;
  hasEnd: boolean;
}

export type WorldScope = "all" | "nether" | "end";

/** Итог проверки файлов инстанса (зеркало instances::repair::RepairReport). */
export interface RepairReport {
  checked: number;
  redownloaded: number;
}

/** Результат запуска java -version (зеркало java::JavaTestInfo). */
export interface JavaTestInfo {
  major: number;
  bits: number;
  versionLine: string;
}

/** Архивный лог инстанса (зеркало mclogs::LogFileInfo). */
export interface LogFileInfo {
  name: string;
  bytes: number;
  modified: number;
}

/** Снапшот модов (зеркало instances::snapshots::SnapshotInfo). */
export interface SnapshotInfo {
  name: string;
  bytes: number;
  createdAt: number;
}

/** Размеры областей каталога данных (зеркало storage::Stats). */
export interface StorageStats {
  instancesBytes: number;
  cacheBytes: number;
  trashBytes: number;
  logsBytes: number;
}

export interface VersionEntry {
  id: string;
  type: string;
  releaseTime: string;
}

export interface LoaderVersionEntry {
  version: string;
  stable: boolean;
}

export interface JavaInstall {
  javaExe: string;
  major: number;
  archBits: number;
  origin: string;
}

export interface ErrorPayload {
  code: string;
  message: string;
  hint?: string;
}

export interface DlProgressEvent {
  event: "dl_progress";
  doneFiles: number;
  totalFiles: number;
  doneBytes: number;
  totalBytes: number;
  bytesPerSec: number;
  etaSecs?: number;
}

export type LaunchPhase = "preparing" | "downloading" | "launching" | "running" | "exited";

export interface LaunchStateEvent {
  event: "launch_state";
  instanceId: string;
  phase: LaunchPhase;
  exitCode?: number;
}

export interface GameLogLineEvent {
  event: "game_log_line";
  instanceId: string;
  line: string;
  stream: "stdout" | "stderr";
}

export interface AccountRefreshFailedEvent {
  event: "account_refresh_failed";
  accountId: string;
  reason: string;
}

/** Состояние очереди загрузок ядра (A30; зеркало Rust DlQueueState). */
export interface QueueState {
  pending: number;
  downloading: number;
  done: number;
  failed: number;
  cancelled: number;
  /** (url, причина) — для списка «повторить» в UI. */
  failedItems: [string, string][];
}

export interface QueueStateEvent extends QueueState {
  event: "dl_queue_state";
}

export type CoreEvent =
  | DlProgressEvent
  | LaunchStateEvent
  | GameLogLineEvent
  | AccountRefreshFailedEvent
  | QueueStateEvent;

// ---------- Modrinth (M5; зеркалит snake_case Rust-структур) ----------

export interface SearchHit {
  project_id: string;
  project_type: string;
  slug: string;
  author: string;
  title: string;
  description: string;
  categories: string[];
  versions: string[];
  downloads: number;
  icon_url?: string;
  date_modified: string;
}

export interface SearchResult {
  hits: SearchHit[];
  total_hits: number;
}

/**
 * Метаданные проекта Modrinth (зеркало `api::ProjectMeta`): в отличие от
 * SearchHit выше структура сериализуется в camelCase (D25) — ядро принимает
 * snake_case Modrinth и отдаёт UI уже нормализованным.
 */
export interface ProjectMeta {
  projectId: string;
  title: string;
  description: string;
  iconUrl?: string;
}

export type ContentKind = "mod" | "resourcepack" | "shader" | "datapack";
export type ContentSource = "modrinth" | "curseforge" | "local";

export interface ContentEntry {
  kind: ContentKind;
  file: string;
  source: ContentSource;
  projectId?: string;
  versionId?: string;
  sha1?: string;
  url?: string;
  enabled: boolean;
}

/** Результат архивной операции (бэкап zip / экспорт .mrpack). */
export interface ArchiveResult {
  path: string;
  files: number;
  bytes: number;
}

export interface UpdateCheck {
  file: string;
  projectId: string;
  currentVersionId: string;
  latestVersionId: string;
  latestVersionNumber: string;
  changelog?: string;
}

export interface RamGuide {
  totalMb: number;
  recommendedMb: number;
  warning?: string;
}

export interface Diagnosis {
  ruleId: string;
  title: string;
  advice: string;
}

// ---------- Обновления лаунчера (GitHub releases) ----------

/** Результат `update_check` (зеркало `commands::UpdateInfo`, camelCase). */
export interface UpdateInfo {
  /** Текущая версия лаунчера. */
  current: string;
  /** Тег последнего релиза (`v0.2.0`); null — версию узнать не удалось. */
  latest?: string;
  /** Страница релиза на github.com. */
  url?: string;
  /** Тег новее текущей версии. */
  updateAvailable: boolean;
  /** Проверка состоялась — «не удалось» тоже `checked`, это не ошибка команды. */
  checked: boolean;
  /** Код причины из errors.json, если версию узнать не удалось (UI переводит). */
  note?: string;
}

// ---------- Волна D37-B ----------

/** Скриншот инстанса (зеркало instances::screens::ShotInfo). */
export interface ShotInfo {
  name: string;
  bytes: number;
  modified: number;
  /** Миниатюра data-URL (ширина 320, генерируется ядром). */
  thumb?: string;
}

/** Метаданные fabric.mod.json/quilt для зависимостей и конфликтов (F6/F9). */
export interface ModMetadata {
  file: string;
  /** id мода из манифеста. */
  id?: string;
  /** Чего не хватает: id зависимостей, которых нет среди установленных. */
  missingDeps: string[];
  /** Выключенные зависимости (.disabled). */
  disabledDeps: string[];
  /** id модов, с которыми заявлен конфликт (breaks/conflicts). */
  conflicts: string[];
}

/** Мониторинг ресурсов запущенной игры (F28; зеркало process::monitor). */
export interface GameResourceSample {
  cpuPercent: number;
  ramBytes: number;
}

/** Обложка ресурспака/шейдера из zip (F25; зеркало instances::packart::PackArt). */
export interface PackArtInfo {
  file: string;
  description?: string;
  png?: string;
}
