// Типы — зеркало Rust-структур (docs/UI-CONTRACT.md). Рассинхрон = ошибка ревью.
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

export type CoreEvent = DlProgressEvent | LaunchStateEvent | GameLogLineEvent;

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

export type ContentKind = "mod" | "resourcepack" | "shader";
export type ContentSource = "modrinth" | "curseforge" | "local";

export interface ContentEntry {
  kind: ContentKind;
  file: string;
  source: ContentSource;
  projectId?: string;
  versionId?: string;
  sha1?: string;
  enabled: boolean;
}

export interface UpdateCheck {
  file: string;
  projectId: string;
  currentVersionId: string;
  latestVersionId: string;
  latestVersionNumber: string;
  changelog?: string;
}
