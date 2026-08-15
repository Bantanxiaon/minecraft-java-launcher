export type Account = {
  id: number;
  accountType: "OFFLINE" | "MICROSOFT";
  displayName: string;
  createdAt: string;
  lastUsedAt?: string;
};
export type Instance = {
  id: number;
  name: string;
  rootPath: string;
  gameVersion: string;
  loaderType: string;
  memoryMb: number;
  status: string;
  source: string;
};
export type VersionSummary = {
  id: string;
  versionType: string;
  url: string;
  sha1: string;
  complianceLevel?: number;
};
export type VersionManifest = {
  latest: { release: string; snapshot: string };
  versions: VersionSummary[];
};
export type JavaRuntime = {
  path: string;
  vendor: string;
  version: string;
  majorVersion?: number;
  architecture: string;
  is64Bit: boolean;
};
export type DownloadProgress = {
  instanceId: number;
  downloadedBytes: number;
  totalBytes?: number;
  jobId?: number;
  sourceUrl?: string;
  fileName?: string;
  speedBytesPerSecond?: number;
  etaSeconds?: number;
};
export type ModInspection = {
  fileName: string;
  loaderType: string;
  modId?: string;
  name?: string;
  version?: string;
  sha256: string;
  fileSize: number;
  warnings: string[];
  gameVersionRequirements: string[];
  dependencies: string[];
  conflicts: string[];
};
export type ContentItem = {
  id: number;
  instanceId: number;
  kind: string;
  fileName: string;
  hash: string;
  metadataJson?: string;
  enabled: boolean;
  source: string;
  installedAt: string;
};
export type ModUpdateInfo = {
  contentId: number;
  projectId: string;
  installedVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
};
export type RemovedContent = { id: number; backupPath: string };
export type BackupItem = {
  kind: "mod" | "resourcepack" | "shaderpack" | "world";
  backupName: string;
  originalName: string;
  size: number;
};
export type ModpackInspection = {
  fileName: string;
  format: string;
  name?: string;
  version?: string;
  gameVersion?: string;
  loaderType?: string;
  modCount: number;
  overrideCount: number;
  warnings: string[];
};
export type ImportedModpack = {
  instance: Instance;
  downloadedFiles: number;
  overrideFiles: number;
};
export type ImportedLocalPack = {
  instanceId: number;
  importedFiles: number;
  importedMods: number;
  unresolvedRemoteFiles: number;
};
export type ExportResult = { path: string; files: number; bytes: number };
export type LauncherError = { message?: string };
export type LauncherSettings = {
  gameDirectory?: string;
  downloadConcurrency: number;
  closeLauncherAfterGameStart: boolean;
  language: string;
  defaultMemoryMb: number;
  microsoftClientId?: string;
  backupWorldsBeforeLaunch: boolean;
};
export type DownloadJob = {
  id: number;
  sourceUrl: string;
  targetPath: string;
  progressBytes: number;
  totalBytes?: number;
  retryCount: number;
  status: string;
  error?: string;
  recoveryAction?: string;
  expectedHash?: string;
  createdAt: string;
  startedAt?: string;
  updatedAt?: string;
  bytesPerSecond?: number;
  etaSeconds?: number;
};
export type CrashReport = {
  id: number;
  instanceId: number;
  occurredAt: string;
  exitCode?: number;
  logPath: string;
  suspectedCause: string;
  confidence: string;
  suggestion: string;
};
export type GameLog = {
  instanceId: number;
  instanceName: string;
  fileName: string;
  size: number;
  modifiedAt: number;
};
export type OnlineProject = {
  projectId: string;
  title: string;
  description: string;
  author: string;
  projectType: "mod" | "modpack";
  downloads: number;
  iconUrl?: string;
  versions: string[];
  categories: string[];
};
